//! Typed client for the Gmail API.
//!
//! Fetch format matters and is chosen per call: metadata-only requests are far
//! cheaper against the API quota and are used when deciding whether a message is
//! worth retrieving in full.
use anyhow::{Context, Result};
use base64::{
    alphabet::URL_SAFE, engine::GeneralPurpose, engine::GeneralPurposeConfig, Engine as _,
};

const URL_SAFE_IGNORE_PAD: GeneralPurpose = GeneralPurpose::new(
    &URL_SAFE,
    GeneralPurposeConfig::new()
        .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent),
);
use serde::{Deserialize, Serialize};

use futures_util::future::BoxFuture;
use std::sync::Arc;
use std::time::Duration;

use super::polling::jittered;

pub type TokenRefresher = Arc<dyn Fn() -> BoxFuture<'static, Result<String>> + Send + Sync>;

use crate::network_client::NetworkClient;

pub const GMAIL_API_BASE_URL: &str = "https://gmail.googleapis.com";

pub(crate) struct QuotaLimiter {
    burst_ceiling: f64,
    refill_per_sec: f64,
    state: tokio::sync::Mutex<QuotaState>,
}

struct QuotaState {
    tokens: f64,
    last_refill: tokio::time::Instant,
}

impl QuotaLimiter {
    const MAX_BURST_UNITS: f64 = 30.0;
/// Builds a token-bucket limiter at the given refill rate.
///
/// Burst capacity is capped separately from the refill rate, so a long idle period
/// cannot accumulate enough tokens to fire a burst large enough to trip Gmail's
/// own rate limiting.

    fn new(units_per_sec: f64) -> Self {
        let burst_ceiling = units_per_sec.min(Self::MAX_BURST_UNITS);
        Self {
            burst_ceiling,
            refill_per_sec: units_per_sec,
            state: tokio::sync::Mutex::new(QuotaState {
                tokens: burst_ceiling,
                last_refill: tokio::time::Instant::now(),
            }),
        }
    }
/// Waits until enough quota is available, then consumes it.
///
/// Tokens refill continuously from elapsed time rather than on a timer, so the
/// limiter needs no background task. Loops rather than sleeping once, because
/// several callers may wake to compete for the same refilled tokens.

    pub(crate) async fn acquire(&self, cost: f64) {
        loop {
            let wait = {
                let mut state = self.state.lock().await;
                let now = tokio::time::Instant::now();
                let elapsed = now.duration_since(state.last_refill).as_secs_f64();
                state.tokens =
                    (state.tokens + elapsed * self.refill_per_sec).min(self.burst_ceiling);
                state.last_refill = now;

                if state.tokens >= cost {
                    state.tokens -= cost;
                    None
                } else {
                    let deficit = cost - state.tokens;
                    Some(std::time::Duration::from_secs_f64(
                        deficit / self.refill_per_sec,
                    ))
                }
            };
            match wait {
                None => return,
                Some(d) => tokio::time::sleep(d).await,
            }
        }
    }
}

#[cfg(test)]
impl QuotaLimiter {
    pub(crate) fn new_for_test(units_per_sec: f64) -> Self {
        Self::new(units_per_sec)
    }
}

pub(crate) fn gmail_quota_limiter() -> &'static QuotaLimiter {
    static LIMITER: std::sync::OnceLock<QuotaLimiter> = std::sync::OnceLock::new();
    LIMITER.get_or_init(|| QuotaLimiter::new(225.0))
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MessagePartHeader {
    pub name: String,
    pub value: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MessagePartBody {
    pub size: Option<i32>,
    pub data: Option<String>,
    #[serde(rename = "attachmentId")]
    pub attachment_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MessagePart {
    #[serde(rename = "partId")]
    pub part_id: Option<String>,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    pub filename: Option<String>,
    pub headers: Option<Vec<MessagePartHeader>>,
    pub body: Option<MessagePartBody>,
    pub parts: Option<Vec<MessagePart>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Message {
    pub id: String,
    #[serde(rename = "threadId")]
    pub thread_id: String,
    #[serde(rename = "historyId")]
    pub history_id: Option<String>,
    pub payload: Option<MessagePart>,
    #[serde(rename = "internalDate")]
    pub internal_date: Option<String>,
    pub snippet: Option<String>,
}

#[derive(Deserialize, Debug)]
#[allow(non_snake_case)]
struct SearchResponse {
    messages: Option<Vec<SearchMessageId>>,
    nextPageToken: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
struct SearchMessageId {
    id: String,
}

pub enum FetchFormat {
    Metadata,
    Full,
}

impl FetchFormat {
    fn as_str(&self) -> &'static str {
        match self {
            FetchFormat::Metadata => "metadata",
            FetchFormat::Full => "full",
        }
    }
}

pub struct GmailClient {
    network: NetworkClient,
    access_token: tokio::sync::RwLock<String>,
    base_url: String,
    refresher: Option<TokenRefresher>,
}

impl GmailClient {
    pub fn new(
        access_token: String,
        db_pool: deadpool_sqlite::Pool,
        refresher: Option<TokenRefresher>,
    ) -> Self {
        Self::new_with_base_url(
            access_token,
            db_pool,
            GMAIL_API_BASE_URL.to_string(),
            refresher,
        )
    }

    pub fn new_with_base_url(
        access_token: String,
        db_pool: deadpool_sqlite::Pool,
        base_url: String,
        refresher: Option<TokenRefresher>,
    ) -> Self {
        Self {
            network: NetworkClient::new(db_pool),
            access_token: tokio::sync::RwLock::new(access_token),
            base_url,
            refresher,
        }
    }

    async fn execute_with_retry<F, Fut>(
        &self,
        operation: &str,
        make_req: F,
    ) -> Result<reqwest::Response>
    where
        F: Fn(String) -> Fut,
        Fut: std::future::Future<Output = reqwest::RequestBuilder>,
    {
        let max_retries = 3;
        let mut attempts = 0;
        let mut backoff = Duration::from_secs(2);

        loop {
            attempts += 1;
            let token = self.access_token.read().await.clone();
            let builder = make_req(token).await;

            let res = match self.network.execute("gmail_api", builder).await {
                Ok(r) => r,
                Err(e) => {
                    if attempts >= max_retries {
                        tracing::warn!(operation, attempts, error = %e, "gmail request failed, retries exhausted");
                        return Err(e).context(format!("Failed to send {} request", operation));
                    }
                    if e.is_timeout() {
                        tracing::warn!(operation, attempts, error = %e, ?backoff, "gmail request timed out, retrying");
                        tokio::time::sleep(jittered(backoff)).await;
                        backoff *= 2;
                    } else {
                        tracing::warn!(operation, attempts, error = %e, "gmail request failed (stale connection), retrying immediately");
                    }
                    continue;
                }
            };

            let status = res.status();
            if status.is_success() {
                return Ok(res);
            }

            if status == reqwest::StatusCode::UNAUTHORIZED {
                if attempts >= max_retries {
                    anyhow::bail!(
                        "{} failed with status 401 Unauthorized (retries exhausted)",
                        operation
                    );
                }
                if let Some(refresher) = &self.refresher {
                    match refresher().await {
                        Ok(new_token) => {
                            *self.access_token.write().await = new_token;
                            continue;
                        }
                        Err(e) => {
                            tracing::error!("Token refresh failed during {}: {}", operation, e);
                            anyhow::bail!(
                                "{} failed with status 401 and token refresh failed: {}",
                                operation,
                                e
                            );
                        }
                    }
                } else {
                    anyhow::bail!(
                        "{} failed with status 401 Unauthorized (no refresher available)",
                        operation
                    );
                }
            }

            if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                if attempts >= max_retries {
                    let error_text = res.text().await.unwrap_or_default();
                    tracing::warn!(operation, attempts, %status, "gmail request failed, retries exhausted");
                    anyhow::bail!(
                        "{} failed with status {}: {}",
                        operation,
                        status,
                        error_text
                    );
                }
                tracing::warn!(operation, attempts, %status, ?backoff, "gmail request throttled/errored, retrying");
                tokio::time::sleep(jittered(backoff)).await;
                backoff *= 2;
                continue;
            }

            let error_text = res.text().await.unwrap_or_default();
            anyhow::bail!(
                "{} failed with status {}: {}",
                operation,
                status,
                error_text
            );
        }
    }

    async fn execute_with_retry_and_parse<F, Fut, T>(
        &self,
        operation: &str,
        make_req: F,
    ) -> Result<T>
    where
        F: Fn(String) -> Fut + Clone,
        Fut: std::future::Future<Output = reqwest::RequestBuilder>,
        T: serde::de::DeserializeOwned,
    {
        let max_retries = 3;
        let mut attempts = 0;
        let mut backoff = Duration::from_secs(2);

        loop {
            attempts += 1;
            let res = self.execute_with_retry(operation, make_req.clone()).await?;

            let text = match res.text().await {
                Ok(t) => t,
                Err(e) => {
                    if attempts >= max_retries {
                        return Err(e).context("Failed to read response body");
                    }
                    tracing::warn!(operation, attempts, error = %e, "gmail response body read failed, retrying");
                    tokio::time::sleep(jittered(backoff)).await;
                    backoff *= 2;
                    continue;
                }
            };

            match serde_json::from_str::<T>(&text) {
                Ok(v) => return Ok(v),
                Err(e) => {
                    if attempts >= max_retries {
                        let snippet: String = text.chars().take(200).collect();
                        return Err(e).context(format!(
                            "Failed to parse {} JSON. Raw snippet: {}",
                            operation, snippet
                        ));
                    }
                    tracing::warn!(operation, attempts, error = %e, "gmail response JSON parse failed, retrying");
                    tokio::time::sleep(jittered(backoff)).await;
                    backoff *= 2;
                }
            }
        }
    }

    pub async fn fetch_message(&self, message_id: &str, format: FetchFormat) -> Result<Message> {
        gmail_quota_limiter().acquire(5.0).await;

        let url = format!(
            "{}/gmail/v1/users/me/messages/{}?format={}",
            self.base_url,
            message_id,
            format.as_str()
        );
        self.execute_with_retry_and_parse("fetch_message", |token| {
            let url = url.clone();
            async move { self.network.client().get(&url).bearer_auth(token) }
        })
        .await
    }

    pub async fn search_messages(
        &self,
        query: &str,
        mut on_page: impl FnMut(usize),
    ) -> Result<Vec<String>> {
        const MAX_SEARCH_PAGES: usize = 500;
        let mut all_message_ids = Vec::new();
        let mut page_token: Option<String> = None;
        let mut pages_fetched: usize = 0;

        loop {
            let encoded_query: String =
                url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
            let mut url = format!(
                "{}/gmail/v1/users/me/messages?q={}",
                self.base_url, encoded_query
            );
            if let Some(token) = &page_token {
                url.push_str(&format!("&pageToken={}", token));
            }

            gmail_quota_limiter().acquire(5.0).await;
            let search_response = self
                .execute_with_retry_and_parse::<_, _, SearchResponse>("search_messages", |token| {
                    let url = url.clone();
                    async move { self.network.client().get(&url).bearer_auth(token) }
                })
                .await?;

            if let Some(messages) = search_response.messages {
                for msg in messages {
                    all_message_ids.push(msg.id);
                }
            }
            pages_fetched += 1;
            on_page(all_message_ids.len());

            page_token = search_response.nextPageToken;
            if page_token.is_none() {
                break;
            }
            if pages_fetched >= MAX_SEARCH_PAGES {
                tracing::warn!(
                    pages_fetched,
                    message_count = all_message_ids.len(),
                    "search_messages: hit MAX_SEARCH_PAGES cap, stopping pagination early -- \
                     date range may be too wide for one scan"
                );
                break;
            }
        }

        Ok(all_message_ids)
    }

    pub async fn fetch_attachment(&self, message_id: &str, attachment_id: &str) -> Result<Vec<u8>> {
        let url = format!(
            "{}/gmail/v1/users/me/messages/{}/attachments/{}",
            self.base_url, message_id, attachment_id
        );
        #[derive(serde::Deserialize)]
        struct AttachmentResponse {
            data: Option<String>,
        }
        let att: AttachmentResponse = self
            .execute_with_retry_and_parse("fetch_attachment", |token| {
                let url = url.clone();
                async move { self.network.client().get(&url).bearer_auth(token) }
            })
            .await?;

        let data_str = att.data.unwrap_or_default();
        let bytes = URL_SAFE_IGNORE_PAD
            .decode(&data_str)
            .context("Failed to base64url-decode attachment data")?;
        Ok(bytes)
    }

    pub async fn get_profile(&self) -> Result<GmailProfile> {
        let url = format!("{}/gmail/v1/users/me/profile", self.base_url);
        self.execute_with_retry_and_parse("get_profile", |token| {
            let url = url.clone();
            async move { self.network.client().get(&url).bearer_auth(token) }
        })
        .await
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct GmailProfile {
    #[serde(rename = "emailAddress")]
    pub email_address: String,
    #[serde(rename = "historyId")]
    pub history_id: Option<String>,
}

#[cfg(test)]
mod quota_limiter_tests {
    use super::QuotaLimiter;

    #[tokio::test(start_paused = true)]
    async fn acquire_does_not_wait_when_tokens_available() {
        let limiter = QuotaLimiter::new(250.0);
        let start = tokio::time::Instant::now();
        limiter.acquire(5.0).await;
        assert_eq!(start.elapsed(), std::time::Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn acquire_waits_for_refill_when_bucket_drained() {
        let limiter = QuotaLimiter::new(10.0);
        limiter.acquire(10.0).await;
        let start = tokio::time::Instant::now();
        limiter.acquire(5.0).await;
        assert!(start.elapsed() >= std::time::Duration::from_millis(500));
    }

    #[tokio::test(start_paused = true)]
    async fn acquire_never_exceeds_capacity_even_after_long_idle() {
        let limiter = QuotaLimiter::new(10.0);
        limiter.acquire(10.0).await;
        tokio::time::advance(std::time::Duration::from_secs(3600)).await;
        let start = tokio::time::Instant::now();
        limiter.acquire(10.0).await;
        assert_eq!(start.elapsed(), std::time::Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn acquire_caps_the_burst_below_capacity_for_a_large_budget() {
        let limiter = QuotaLimiter::new(225.0);
        let start = tokio::time::Instant::now();
        limiter.acquire(30.0).await;
        assert_eq!(start.elapsed(), std::time::Duration::ZERO);

        let start = tokio::time::Instant::now();
        limiter.acquire(1.0).await;
        assert!(
            start.elapsed() > std::time::Duration::ZERO,
            "starting bucket must be capped at MAX_BURST_UNITS, not the full 225/sec budget"
        );
    }
}
