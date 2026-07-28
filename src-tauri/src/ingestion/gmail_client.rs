use anyhow::{Context, Result};
use base64::{
    alphabet::URL_SAFE, engine::GeneralPurpose, engine::GeneralPurposeConfig, Engine as _,
};

// Define an engine that handles both padded and unpadded URL-safe base64
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

/// Production Gmail API base URL. Tests inject a mockito server URL instead
/// (same pattern as `LicensingClient::new`).
pub const GMAIL_API_BASE_URL: &str = "https://gmail.googleapis.com";

/// Doc 2026-07-26 mail scan performance: paces total Gmail quota-unit
/// consumption (metadata + full fetches + search pages all draw from the
/// same bucket) instead of only capping full-fetch *concurrency* — a
/// concurrency cap assumes ~1s/request, which real Gmail requests (a few
/// hundred ms) violate constantly, causing bursts well past the real
/// 250-units/sec budget and the 429-storm this replaces.
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
    /// Doc 2026-07-28 dev-scan-log-issues: caps how many units can ever be
    /// available at once, regardless of the configured per-second budget --
    /// a full-capacity bucket (the old behavior) let a cold start or a long
    /// idle gap (e.g. mid-scan retry backoff) release the *entire*
    /// per-second budget in one instant, which is exactly what a
    /// steady-rate token bucket is supposed to prevent. 45 units is ~9
    /// Full-format fetches (5 units each) worth of simultaneous
    /// connections -- enough to keep throughput high without synchronizing
    /// a burst large enough to trigger Gmail-side 429s/connection resets
    /// (observed in production logs as "error sending request" storms
    /// immediately after "Spawning fetch" bursts).
    const MAX_BURST_UNITS: f64 = 30.0;

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

    /// Blocks until `cost` units are available, refilling based on elapsed
    /// wall-clock (or, under test, virtual-paused) time since the last call.
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

/// Doc 30 TASK-GMAIL-002's 250/sec budget, kept at a 90% safety margin
/// (225/sec) so normal jitter and clock granularity don't still clip the
/// real ceiling.
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

/// Wrapper for Gmail API operations
pub struct GmailClient {
    network: NetworkClient,
    access_token: tokio::sync::RwLock<String>,
    base_url: String,
    refresher: Option<TokenRefresher>,
}

impl GmailClient {
    /// Doc 01 §10.4 (BG-02): every Gmail API call must route through
    /// `NetworkClient` so it's captured in the local Network Activity audit
    /// trail — this used to build its own bare `reqwest::Client`, making
    /// every Gmail call invisible to that log.
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

    /// Same as `new`, but with an injectable base URL — used by tests to point
    /// at a mockito server instead of the real Gmail API (same pattern as
    /// `LicensingClient::new`).
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

    /// Fetches full message payload by ID with specified format.
    ///
    /// Full-format fetches acquire a permit from the shared quota semaphore
    /// (Doc 30 TASK-GMAIL-002) before hitting the network; metadata-only
    /// fetches are cheap (1 quota unit) and skip the gate entirely.

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
                    tracing::warn!(operation, attempts, error = %e, ?backoff, "gmail request failed, retrying");
                    tokio::time::sleep(jittered(backoff)).await;
                    backoff *= 2;
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
                            // Retry immediately with new token
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

            // Other client errors
            let error_text = res.text().await.unwrap_or_default();
            anyhow::bail!(
                "{} failed with status {}: {}",
                operation,
                status,
                error_text
            );
        }
    }

    /// Wraps `execute_with_retry` with a retry around body-read + JSON-parse.
    /// A successful status doesn't guarantee a clean body — the connection
    /// can still drop mid-transfer (`res.text()` fails) or Google can
    /// occasionally serve a truncated/malformed payload behind a 200
    /// (`serde_json::from_str` fails) — both transient, so on retryable
    /// attempts remaining this re-issues the whole request (a consumed
    /// `Response` can't be re-read) rather than failing outright.
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
        let cost = match format {
            FetchFormat::Full => 5.0,
            FetchFormat::Metadata => 1.0,
        };
        gmail_quota_limiter().acquire(cost).await;

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

    /// Executes a general search (e.g. q=after:YYYY/MM/DD before:YYYY/MM/DD) and returns all message IDs, handling pagination.
    ///
    /// This whole method is one long `await` from the caller's perspective —
    /// for a wide date range on a large mailbox it can take many sequential
    /// page fetches before returning at all, and the historical-scan UI has
    /// no other progress signal during this phase (`scan_progress` normally
    /// only fires once the full ID list is known), so the counters would sit
    /// frozen at "0 / 0" for however long pagination takes. `on_page` is
    /// invoked after every page with the running total found so far, so the
    /// caller can emit a progress update the user can actually see moving.
    /// `MAX_SEARCH_PAGES` additionally bounds how long an unreasonably wide
    /// range (e.g. a decade) can page for at all, rather than continuing
    /// indefinitely.
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

    /// Downloads a Gmail attachment by its `attachmentId` and returns the raw bytes.
    ///
    /// Gmail API returns large attachment data via a separate endpoint — the `fetch_message(Full)`
    /// call only carries an `attachmentId` reference, not the bytes themselves (Doc 12 §7.2 step 1).
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

    /// Doc 01 §8.1 C-05 (Doc 22 §5.2): the account's own email address, needed
    /// to identify a connected account, obtained via the Gmail API's own
    /// `users.getProfile` endpoint — which `gmail.readonly` alone already
    /// grants — rather than Google's separate `oauth2/v2/userinfo` endpoint,
    /// which requires the `openid`/`email`/`profile` scopes this app must
    /// never request ("Gmail scope must remain `gmail.readonly` only").
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
        let limiter = QuotaLimiter::new(10.0); // 10 units/sec, capacity 10
        limiter.acquire(10.0).await; // drains the full starting bucket
        let start = tokio::time::Instant::now();
        limiter.acquire(5.0).await; // needs 0.5s of refill at 10/sec
        assert!(start.elapsed() >= std::time::Duration::from_millis(500));
    }

    #[tokio::test(start_paused = true)]
    async fn acquire_never_exceeds_capacity_even_after_long_idle() {
        let limiter = QuotaLimiter::new(10.0);
        limiter.acquire(10.0).await; // drain
        tokio::time::advance(std::time::Duration::from_secs(3600)).await; // idle an hour
        let start = tokio::time::Instant::now();
        limiter.acquire(10.0).await; // must not wait — capacity caps the refill
        assert_eq!(start.elapsed(), std::time::Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn acquire_caps_the_burst_below_capacity_for_a_large_budget() {
        // Doc 2026-07-28 dev-scan-log-issues: production's real budget
        // (225 units/sec) is far above MAX_BURST_UNITS (45) -- this proves
        // the *starting* bucket for a large budget is capped at 45, not the
        // full 225, by requesting one unit more than the cap and confirming
        // it still has to wait for a sliver of refill instead of being
        // instantly available (which would only be possible if the ceiling
        // weren't actually being enforced). The other tests in this module
        // all use capacities <= 10, already under the cap, so they can't
        // catch a regression here.
        let limiter = QuotaLimiter::new(225.0);
        let start = tokio::time::Instant::now();
        limiter.acquire(31.0).await; // 1 unit above the 30-unit ceiling
        assert!(
            start.elapsed() > std::time::Duration::ZERO,
            "a request above the burst ceiling must still be paced, even for a large budget"
        );
    }
}
