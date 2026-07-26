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
use std::sync::OnceLock;
use tokio::sync::Semaphore;

use futures_util::future::BoxFuture;
use std::sync::Arc;
use std::time::Duration;

use super::polling::jittered;

pub type TokenRefresher = Arc<dyn Fn() -> BoxFuture<'static, Result<String>> + Send + Sync>;

use crate::network_client::NetworkClient;

/// Production Gmail API base URL. Tests inject a mockito server URL instead
/// (same pattern as `LicensingClient::new`).
pub const GMAIL_API_BASE_URL: &str = "https://gmail.googleapis.com";

/// Doc 30 TASK-GMAIL-002: caps concurrent full-message (`format=FULL`) fetches
/// at 50, the mechanism approximating Gmail's 250 quota-units/second budget
/// across every caller (real-time poll and historical scan alike). Metadata-only
/// fetches (1 quota unit) are cheap and are not gated by this semaphore.
pub(crate) fn full_fetch_semaphore() -> &'static Semaphore {
    static SEM: OnceLock<Semaphore> = OnceLock::new();
    SEM.get_or_init(|| Semaphore::new(50))
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
        let _permit = match format {
            FetchFormat::Full => Some(
                full_fetch_semaphore()
                    .acquire()
                    .await
                    .expect("full_fetch_semaphore never closed"),
            ),
            FetchFormat::Metadata => None,
        };

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
