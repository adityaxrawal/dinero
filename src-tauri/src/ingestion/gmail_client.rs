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

/// The one gate every Gmail request passes through, retries included.
///
/// Three controls, because each covers a failure the others miss:
/// - a token bucket keeps the average rate well under Gmail's per-user budget
///   (250 quota units/second);
/// - an in-flight cap keeps the number of concurrent requests low, since Gmail
///   also throttles on concurrency regardless of rate;
/// - a shared cooldown: when Gmail answers 429 (or 403 rateLimitExceeded),
///   *every* caller pauses, not just the one that was refused, and the rate is
///   halved. Without it the other workers keep firing into a limit that is
///   already tripped, and all of them back off and retry in lockstep.
///
/// Each success raises the rate a little again (additive increase,
/// multiplicative decrease), so a scan settles just below whatever this account
/// tolerates.
pub(crate) struct GmailGate {
    max_rate: f64,
    min_rate: f64,
    in_flight: tokio::sync::Semaphore,
    state: tokio::sync::Mutex<GateState>,
}

struct GateState {
    tokens: f64,
    rate: f64,
    last_refill: tokio::time::Instant,
    paused_until: Option<tokio::time::Instant>,
    /// The cooldown used for the last 429 that carried no Retry-After hint;
    /// doubles per consecutive 429 and resets on success.
    backoff: Duration,
}

/// Steady-state ceiling: 40% of Gmail's 250 units/second per-user budget, so
/// polling, statement downloads and a scan together still stay clear of it.
const GATE_MAX_UNITS_PER_SEC: f64 = 100.0;
/// The rate never drops below this however many 429s arrive (2 requests/s).
const GATE_MIN_UNITS_PER_SEC: f64 = 10.0;
/// Requests in flight at once, across every Gmail caller.
const GATE_MAX_IN_FLIGHT: usize = 4;
/// Burst ceiling, so an idle period cannot bank a large burst.
const GATE_MAX_BURST_UNITS: f64 = 20.0;
/// Rate regained per successful request after a slowdown.
const GATE_RECOVERY_UNITS: f64 = 1.0;
/// Bounds on one cooldown. The floor stops a `Retry-After: 0` from becoming a
/// tight loop; the ceiling stops a bogus hint from stalling ingestion for hours.
const GATE_MIN_COOLDOWN: Duration = if cfg!(test) {
    Duration::from_millis(10)
} else {
    Duration::from_secs(1)
};
const GATE_MAX_COOLDOWN: Duration = Duration::from_secs(15 * 60);
/// First and largest computed cooldown when Gmail gives no hint.
const GATE_FIRST_BACKOFF: Duration = if cfg!(test) {
    Duration::from_millis(10)
} else {
    Duration::from_secs(2)
};
const GATE_MAX_BACKOFF: Duration = Duration::from_secs(64);

impl GmailGate {
    fn new(max_rate: f64, min_rate: f64, max_in_flight: usize) -> Self {
        Self {
            max_rate,
            min_rate,
            in_flight: tokio::sync::Semaphore::new(max_in_flight),
            state: tokio::sync::Mutex::new(GateState {
                tokens: max_rate.min(GATE_MAX_BURST_UNITS),
                rate: max_rate,
                last_refill: tokio::time::Instant::now(),
                paused_until: None,
                backoff: Duration::ZERO,
            }),
        }
    }

    /// Waits for an in-flight slot, any shared cooldown, and `cost` quota units.
    ///
    /// Hold the returned permit until the response status is known; dropping it
    /// frees the slot.
    pub(crate) async fn acquire(&self, cost: f64) -> tokio::sync::SemaphorePermit<'_> {
        let permit = self
            .in_flight
            .acquire()
            .await
            .expect("the Gmail gate semaphore is never closed");
        loop {
            let wait = {
                let mut st = self.state.lock().await;
                let now = tokio::time::Instant::now();
                match st.paused_until {
                    Some(until) if until > now => Some(until - now),
                    _ => {
                        let ceiling = st.rate.min(GATE_MAX_BURST_UNITS);
                        let elapsed = now.saturating_duration_since(st.last_refill).as_secs_f64();
                        st.tokens = (st.tokens + elapsed * st.rate).min(ceiling);
                        st.last_refill = now;
                        if st.tokens >= cost {
                            st.tokens -= cost;
                            None
                        } else {
                            Some(Duration::from_secs_f64((cost - st.tokens) / st.rate))
                        }
                    }
                }
            };
            match wait {
                None => return permit,
                Some(d) => tokio::time::sleep(d).await,
            }
        }
    }

    /// Records a rate-limit response: pauses every caller and halves the rate.
    ///
    /// A burst of 429s from requests that were already in flight is one event,
    /// not several: while a cooldown is running, a later 429 can only extend it
    /// to honour a longer `Retry-After`, never halve the rate again.
    pub(crate) async fn on_rate_limited(&self, retry_after: Option<Duration>) {
        let mut st = self.state.lock().await;
        let now = tokio::time::Instant::now();
        let cooling = st.paused_until.is_some_and(|until| until > now);
        let cooldown = if cooling {
            match retry_after {
                Some(hint) => hint.clamp(GATE_MIN_COOLDOWN, GATE_MAX_COOLDOWN),
                None => return,
            }
        } else {
            st.backoff = if st.backoff.is_zero() {
                GATE_FIRST_BACKOFF
            } else {
                (st.backoff * 2).min(GATE_MAX_BACKOFF)
            };
            st.rate = (st.rate / 2.0).max(self.min_rate);
            retry_after
                .unwrap_or_else(|| jittered(st.backoff))
                .clamp(GATE_MIN_COOLDOWN, GATE_MAX_COOLDOWN)
        };
        let until = now + cooldown;
        if st.paused_until.is_none_or(|current| until > current) {
            st.paused_until = Some(until);
            // Restart from an empty bucket once the cooldown ends, so the first
            // requests after it trickle out instead of bursting.
            st.tokens = 0.0;
            st.last_refill = until;
        }
        tracing::warn!(
            cooldown_ms = cooldown.as_millis() as u64,
            rate_units_per_sec = st.rate,
            hinted = retry_after.is_some(),
            "Gmail rate limit: pausing all Gmail requests"
        );
    }

    /// Records a successful response: clears the backoff and regains some rate.
    pub(crate) async fn on_success(&self) {
        let mut st = self.state.lock().await;
        st.backoff = Duration::ZERO;
        st.rate = (st.rate + GATE_RECOVERY_UNITS).min(self.max_rate);
    }
}

#[cfg(test)]
impl GmailGate {
    pub(crate) fn new_for_test(max_rate: f64, min_rate: f64, max_in_flight: usize) -> Self {
        Self::new(max_rate, min_rate, max_in_flight)
    }

    pub(crate) async fn rate_for_test(&self) -> f64 {
        self.state.lock().await.rate
    }
}

/// The process-wide gate: scans, polling and statement downloads share one
/// Gmail budget, so they must share one gate.
pub(crate) fn gmail_gate() -> &'static GmailGate {
    static GATE: std::sync::OnceLock<GmailGate> = std::sync::OnceLock::new();
    GATE.get_or_init(|| {
        // Tests share this one gate across parallel mock servers; production
        // pacing there only makes unrelated tests wait on each other's 429s.
        // Pacing itself is tested on dedicated gates (`new_for_test`).
        if cfg!(test) {
            GmailGate::new(1_000_000.0, 1_000_000.0, 64)
        } else {
            GmailGate::new(GATE_MAX_UNITS_PER_SEC, GATE_MIN_UNITS_PER_SEC, GATE_MAX_IN_FLIGHT)
        }
    })
}

/// Quota cost of each Gmail method used, in Gmail's quota units.
pub(crate) mod quota_cost {
    pub const MESSAGES_GET: f64 = 5.0;
    pub const MESSAGES_LIST: f64 = 5.0;
    pub const ATTACHMENTS_GET: f64 = 5.0;
    pub const HISTORY_LIST: f64 = 2.0;
    pub const GET_PROFILE: f64 = 1.0;
}

/// Timeout for one attachment download (review L1).
const ATTACHMENT_TIMEOUT: Duration = Duration::from_secs(120);

/// How long a request keeps waiting out rate limits before it is reported as
/// failed. A rate limit says nothing about the message, so it is waited out
/// rather than counted against it; this cap only stops a persistently refused
/// account from hanging a scan forever.
const RATE_LIMIT_GIVE_UP_AFTER: Duration = Duration::from_secs(15 * 60);

/// Whether a response is Gmail asking the client to slow down.
///
/// Gmail signals per-user limits as 403 with reason `rateLimitExceeded` or
/// `userRateLimitExceeded`, not only as 429. Any other 403 (for example
/// insufficient permissions) is permanent and must not be retried.
pub(crate) fn is_rate_limited(status: reqwest::StatusCode, body: &str) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || (status == reqwest::StatusCode::FORBIDDEN
            && (body.contains("rateLimitExceeded") || body.contains("userRateLimitExceeded")))
}

/// How long Gmail asked us to wait, if it said.
///
/// Either a `Retry-After` header in seconds, or the timestamp Gmail puts in a
/// `userRateLimitExceeded` message ("... Retry after 2026-11-25T10:00:00.000Z").
pub(crate) fn retry_after_hint(headers: &reqwest::header::HeaderMap, body: &str) -> Option<Duration> {
    if let Some(secs) = headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
    {
        return Some(Duration::from_secs(secs));
    }
    const MARKER: &str = "Retry after ";
    let rest = &body[body.find(MARKER)? + MARKER.len()..];
    let stamp = rest
        .split(|c: char| c.is_whitespace() || c == '"')
        .next()?
        .trim_end_matches([',', '.']);
    let at = chrono::DateTime::parse_from_rfc3339(stamp).ok()?;
    let millis = (at.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_milliseconds();
    Some(Duration::from_millis(millis.max(0) as u64))
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

    /// Sends one Gmail request through the shared gate, retrying what is worth
    /// retrying.
    ///
    /// Every attempt, retries included, waits for the gate and pays `cost`
    /// quota units: a retry is a real request against Gmail's budget.
    /// Rate limits are waited out (the gate pauses every caller) and do not
    /// count as failed attempts; timeouts, dropped connections, 401s and 5xx
    /// get `MAX_FAILED_ATTEMPTS`.
    async fn execute_with_retry<F, Fut>(
        &self,
        operation: &str,
        cost: f64,
        make_req: F,
    ) -> Result<reqwest::Response>
    where
        F: Fn(String) -> Fut,
        Fut: std::future::Future<Output = reqwest::RequestBuilder>,
    {
        const MAX_FAILED_ATTEMPTS: u32 = 3;
        let gate = gmail_gate();
        let mut failed_attempts = 0;
        let mut backoff = Duration::from_secs(2);
        let mut throttled_since: Option<std::time::Instant> = None;

        loop {
            let permit = gate.acquire(cost).await;
            let token = self.access_token.read().await.clone();
            let builder = make_req(token).await;

            let res = match self.network.execute("gmail_api", builder).await {
                Ok(r) => r,
                Err(e) => {
                    drop(permit);
                    failed_attempts += 1;
                    if failed_attempts >= MAX_FAILED_ATTEMPTS {
                        tracing::warn!(operation, failed_attempts, error = %e, "gmail request failed, retries exhausted");
                        return Err(e).context(format!("Failed to send {} request", operation));
                    }
                    tracing::warn!(operation, failed_attempts, error = %e, ?backoff, "gmail request failed to send, retrying");
                    tokio::time::sleep(jittered(backoff)).await;
                    backoff *= 2;
                    continue;
                }
            };

            let status = res.status();
            if status.is_success() {
                drop(permit);
                gate.on_success().await;
                return Ok(res);
            }

            if status == reqwest::StatusCode::UNAUTHORIZED {
                drop(permit);
                failed_attempts += 1;
                if failed_attempts >= MAX_FAILED_ATTEMPTS {
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

            let headers = res.headers().clone();
            let error_text = res.text().await.unwrap_or_default();
            drop(permit);

            if is_rate_limited(status, &error_text) {
                let since = *throttled_since.get_or_insert_with(std::time::Instant::now);
                if since.elapsed() >= RATE_LIMIT_GIVE_UP_AFTER {
                    tracing::error!(operation, %status, waited_secs = since.elapsed().as_secs(), "gmail still rate-limiting after the give-up window");
                    anyhow::bail!(
                        "{} failed with status {} after {:?} of rate limiting: {}",
                        operation,
                        status,
                        RATE_LIMIT_GIVE_UP_AFTER,
                        error_text
                    );
                }
                crate::ingestion::gmail_telemetry::gmail_telemetry().record_quota_exhausted();
                gate.on_rate_limited(retry_after_hint(&headers, &error_text)).await;
                continue;
            }

            if status.is_server_error() {
                failed_attempts += 1;
                if failed_attempts >= MAX_FAILED_ATTEMPTS {
                    tracing::warn!(operation, failed_attempts, %status, "gmail request failed, retries exhausted");
                    anyhow::bail!(
                        "{} failed with status {}: {}",
                        operation,
                        status,
                        error_text
                    );
                }
                tracing::warn!(operation, failed_attempts, %status, ?backoff, "gmail server error, retrying");
                tokio::time::sleep(jittered(backoff)).await;
                backoff *= 2;
                continue;
            }

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
        cost: f64,
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
            let res = self.execute_with_retry(operation, cost, make_req.clone()).await?;

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
        let url = format!(
            "{}/gmail/v1/users/me/messages/{}?format={}",
            self.base_url,
            message_id,
            format.as_str()
        );
        self.execute_with_retry_and_parse("fetch_message", quota_cost::MESSAGES_GET, |token| {
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

            let search_response = self
                .execute_with_retry_and_parse::<_, _, SearchResponse>("search_messages", quota_cost::MESSAGES_LIST, |token| {
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
            .execute_with_retry_and_parse("fetch_attachment", quota_cost::ATTACHMENTS_GET, |token| {
                let url = url.clone();
                // Statement PDFs can take longer than the 15s default.
                async move {
                    self.network
                        .client()
                        .get(&url)
                        .bearer_auth(token)
                        .timeout(ATTACHMENT_TIMEOUT)
                }
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
        self.execute_with_retry_and_parse("get_profile", quota_cost::GET_PROFILE, |token| {
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
mod gmail_gate_tests {
    use super::{retry_after_hint, GmailGate, GATE_MAX_BURST_UNITS};
    use std::time::Duration;
    use tokio::time::Instant;

    #[tokio::test(start_paused = true)]
    async fn acquire_does_not_wait_when_tokens_available() {
        let gate = GmailGate::new_for_test(100.0, 10.0, 4);
        let start = Instant::now();
        let _p = gate.acquire(5.0).await;
        assert_eq!(start.elapsed(), Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn acquire_waits_for_refill_when_bucket_drained() {
        let gate = GmailGate::new_for_test(10.0, 1.0, 4);
        drop(gate.acquire(10.0).await);
        let start = Instant::now();
        let _p = gate.acquire(5.0).await;
        assert!(start.elapsed() >= Duration::from_millis(500));
    }

    #[tokio::test(start_paused = true)]
    async fn an_idle_period_cannot_bank_more_than_the_burst_ceiling() {
        let gate = GmailGate::new_for_test(100.0, 10.0, 4);
        drop(gate.acquire(GATE_MAX_BURST_UNITS).await);
        tokio::time::advance(Duration::from_secs(3600)).await;
        drop(gate.acquire(GATE_MAX_BURST_UNITS).await);
        let start = Instant::now();
        drop(gate.acquire(1.0).await);
        assert!(start.elapsed() > Duration::ZERO, "the bucket must cap at the burst ceiling");
    }

    #[tokio::test(start_paused = true)]
    async fn in_flight_requests_are_capped() {
        let gate = GmailGate::new_for_test(1000.0, 10.0, 2);
        let _a = gate.acquire(1.0).await;
        let _b = gate.acquire(1.0).await;
        let third = tokio::time::timeout(Duration::from_secs(1), gate.acquire(1.0)).await;
        assert!(third.is_err(), "a third request must wait while two are in flight");
    }

    #[tokio::test(start_paused = true)]
    async fn a_rate_limit_pauses_every_caller_for_the_hinted_time() {
        let gate = GmailGate::new_for_test(100.0, 10.0, 4);
        gate.on_rate_limited(Some(Duration::from_secs(30))).await;
        let start = Instant::now();
        drop(gate.acquire(5.0).await);
        assert!(start.elapsed() >= Duration::from_secs(30));
    }

    #[tokio::test(start_paused = true)]
    async fn a_burst_of_429s_halves_the_rate_once_and_success_recovers_it() {
        let gate = GmailGate::new_for_test(100.0, 10.0, 4);
        // Several in-flight requests refused together are one event.
        for _ in 0..4 {
            gate.on_rate_limited(None).await;
        }
        assert_eq!(gate.rate_for_test().await, 50.0);
        for _ in 0..100 {
            gate.on_success().await;
        }
        assert_eq!(gate.rate_for_test().await, 100.0, "recovery stops at the ceiling");
    }

    #[tokio::test(start_paused = true)]
    async fn the_rate_never_drops_below_the_floor() {
        let gate = GmailGate::new_for_test(100.0, 10.0, 4);
        for _ in 0..10 {
            gate.on_rate_limited(None).await;
            tokio::time::advance(Duration::from_secs(120)).await;
        }
        assert_eq!(gate.rate_for_test().await, 10.0);
    }

    #[tokio::test(start_paused = true)]
    async fn a_longer_hint_during_a_cooldown_extends_it() {
        let gate = GmailGate::new_for_test(100.0, 10.0, 4);
        gate.on_rate_limited(Some(Duration::from_secs(5))).await;
        gate.on_rate_limited(Some(Duration::from_secs(40))).await;
        let start = Instant::now();
        drop(gate.acquire(5.0).await);
        assert!(start.elapsed() >= Duration::from_secs(40));
        assert_eq!(gate.rate_for_test().await, 50.0, "still one halving");
    }

    #[test]
    fn retry_after_header_in_seconds_is_read() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::RETRY_AFTER, "17".parse().unwrap());
        assert_eq!(retry_after_hint(&headers, ""), Some(Duration::from_secs(17)));
    }

    #[test]
    fn gmail_retry_after_timestamp_in_the_body_is_read() {
        let at = chrono::Utc::now() + chrono::Duration::seconds(90);
        let body = format!(
            r#"{{"error":{{"code":429,"message":"User-rate limit exceeded.  Retry after {}","errors":[{{"reason":"rateLimitExceeded"}}]}}}}"#,
            at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        );
        let hint = retry_after_hint(&reqwest::header::HeaderMap::new(), &body).unwrap();
        assert!(hint > Duration::from_secs(80) && hint <= Duration::from_secs(90), "{hint:?}");
    }

    #[test]
    fn no_hint_when_gmail_gives_none() {
        assert_eq!(retry_after_hint(&reqwest::header::HeaderMap::new(), "rateLimitExceeded"), None);
    }
}
