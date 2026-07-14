use anyhow::{Context, Result};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use tokio::sync::Semaphore;

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
    access_token: String,
    base_url: String,
}

impl GmailClient {
    /// Doc 01 §10.4 (BG-02): every Gmail API call must route through
    /// `NetworkClient` so it's captured in the local Network Activity audit
    /// trail — this used to build its own bare `reqwest::Client`, making
    /// every Gmail call invisible to that log.
    pub fn new(access_token: String, db_pool: deadpool_sqlite::Pool) -> Self {
        Self::new_with_base_url(access_token, db_pool, GMAIL_API_BASE_URL.to_string())
    }

    /// Same as `new`, but with an injectable base URL — used by tests to point
    /// at a mockito server instead of the real Gmail API (same pattern as
    /// `LicensingClient::new`).
    pub fn new_with_base_url(
        access_token: String,
        db_pool: deadpool_sqlite::Pool,
        base_url: String,
    ) -> Self {
        Self {
            network: NetworkClient::new(db_pool),
            access_token,
            base_url,
        }
    }

    /// Fetches full message payload by ID with specified format.
    ///
    /// Full-format fetches acquire a permit from the shared quota semaphore
    /// (Doc 30 TASK-GMAIL-002) before hitting the network; metadata-only
    /// fetches are cheap (1 quota unit) and skip the gate entirely.
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
        let builder = self.network.client().get(&url).bearer_auth(&self.access_token);
        let res = self
            .network
            .execute(builder)
            .await
            .context("Failed to send fetch_message request")?;

        let status = res.status();
        if !status.is_success() {
            let error_text = res.text().await.unwrap_or_default();
            anyhow::bail!(
                "fetch_message failed with status {}: {}",
                status,
                error_text
            );
        }

        let message = res
            .json::<Message>()
            .await
            .context("Failed to parse Message JSON")?;
        Ok(message)
    }

    /// Executes a general search (e.g. q=after:YYYY/MM/DD before:YYYY/MM/DD) and returns all message IDs, handling pagination.
    pub async fn search_messages(&self, query: &str) -> Result<Vec<String>> {
        let mut all_message_ids = Vec::new();
        let mut page_token: Option<String> = None;

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

            let builder = self.network.client().get(&url).bearer_auth(&self.access_token);
            let res = self
                .network
                .execute(builder)
                .await
                .context("Failed to send search request")?;

            let status = res.status();
            if !status.is_success() {
                let error_text = res.text().await.unwrap_or_default();
                anyhow::bail!(
                    "search_messages failed with status {}: {}",
                    status,
                    error_text
                );
            }

            let search_response = res
                .json::<SearchResponse>()
                .await
                .context("Failed to parse SearchResponse JSON")?;

            if let Some(messages) = search_response.messages {
                for msg in messages {
                    all_message_ids.push(msg.id);
                }
            }

            page_token = search_response.nextPageToken;
            if page_token.is_none() {
                break;
            }
        }

        Ok(all_message_ids)
    }

    /// Downloads a Gmail attachment by its `attachmentId` and returns the raw bytes.
    ///
    /// Gmail API returns large attachment data via a separate endpoint — the `fetch_message(Full)`
    /// call only carries an `attachmentId` reference, not the bytes themselves (Doc 12 §7.2 step 1).
    pub async fn fetch_attachment(
        &self,
        message_id: &str,
        attachment_id: &str,
    ) -> Result<Vec<u8>> {
        let url = format!(
            "{}/gmail/v1/users/me/messages/{}/attachments/{}",
            self.base_url, message_id, attachment_id
        );
        let builder = self.network.client().get(&url).bearer_auth(&self.access_token);
        let res = self
            .network
            .execute(builder)
            .await
            .context("Failed to send fetch_attachment request")?;

        let status = res.status();
        if !status.is_success() {
            let error_text = res.text().await.unwrap_or_default();
            anyhow::bail!(
                "fetch_attachment failed with status {}: {}",
                status,
                error_text
            );
        }

        #[derive(serde::Deserialize)]
        struct AttachmentResponse {
            data: Option<String>,
        }
        let att: AttachmentResponse = res
            .json()
            .await
            .context("Failed to parse attachment JSON")?;

        let data_str = att.data.unwrap_or_default();
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
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
        let builder = self.network.client().get(&url).bearer_auth(&self.access_token);
        let res = self
            .network
            .execute(builder)
            .await
            .context("Failed to send get_profile request")?;

        let status = res.status();
        if !status.is_success() {
            let error_text = res.text().await.unwrap_or_default();
            anyhow::bail!("get_profile failed with status {}: {}", status, error_text);
        }

        res.json::<GmailProfile>()
            .await
            .context("Failed to parse GmailProfile JSON")
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct GmailProfile {
    #[serde(rename = "emailAddress")]
    pub email_address: String,
    #[serde(rename = "historyId")]
    pub history_id: Option<String>,
}
