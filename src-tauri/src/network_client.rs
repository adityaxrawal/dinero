use crate::db::network_activity_log::{self, NetworkActivityLogRow};
use chrono::Utc;
use deadpool_sqlite::Pool;
use reqwest::{Client, RequestBuilder, Response};
use std::time::Duration;
use uuid::Uuid;

/// Every network call this app makes (Gmail API fetches, licensing,
/// GitHub/HuggingFace releases) previously ran on a bare `Client::new()`
/// with no timeout at all -- a stalled connection (dead peer, network
/// partition, DNS hang) blocked the call forever with no error surfaced.
/// For a historical Gmail scan specifically, this meant a single hung
/// request could freeze the whole scan indefinitely with the UI stuck
/// showing "Scanning..." and no way to tell the user anything went wrong.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

pub struct NetworkClient {
    client: Client,
    db_pool: Pool,
}

impl NetworkClient {
    pub fn new(db_pool: Pool) -> Self {
        Self::with_timeout(db_pool, REQUEST_TIMEOUT)
    }

    /// Real constructor behind `new()` -- takes an explicit timeout so
    /// tests can prove the timeout mechanism actually fires without waiting
    /// out the real 15s production value.
    pub(crate) fn with_timeout(db_pool: Pool, timeout: Duration) -> Self {
        Self {
            client: Client::builder()
                .timeout(timeout)
                .build()
                .expect("reqwest::Client::builder() with only a timeout set must always succeed"),
            db_pool,
        }
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Doc 30 TASK-API-006: `channel` is one of the 5 disclosed network
    /// channels (`gmail_api`/`licensing_backend`/`google_oauth`/
    /// `github_releases`/`huggingface`) -- written directly into
    /// `network_activity_log.channel` rather than inferred later from the
    /// destination hostname (`commands/network.rs`'s read-time fallback,
    /// which silently produces "unknown" for any host it doesn't recognize).
    pub async fn execute(
        &self,
        channel: &str,
        req_builder: RequestBuilder,
    ) -> reqwest::Result<Response> {
        let request = req_builder.build()?;

        let method = request.method().to_string();
        let url = request.url().clone();
        let domain = url.domain().unwrap_or("unknown").to_string();

        // Redact url query parameters if they contain sensitive tokens
        let mut redacted_url = url.clone();
        if url.query().is_some() {
            redacted_url.set_query(Some("redacted"));
        }

        let start_time = Utc::now().naive_utc();
        let bytes_sent = request
            .body()
            .map(|b| b.as_bytes().map(|b| b.len() as i64).unwrap_or(0))
            .unwrap_or(0);

        let response_result = self.client.execute(request).await;

        let (status_code, bytes_received) = match &response_result {
            Ok(res) => (
                Some(res.status().as_u16() as i64),
                res.content_length().map(|l| l as i64),
            ),
            Err(e) => (e.status().map(|s| s.as_u16() as i64), None),
        };

        let log_row = NetworkActivityLogRow {
            id: Uuid::new_v4().to_string(),
            timestamp: Some(start_time),
            method,
            domain,
            url_redacted: redacted_url.to_string(),
            bytes_sent: Some(bytes_sent),
            bytes_received,
            status_code,
            secret_fields_masked: Some("Authorization,Cookie".into()),
            channel: Some(channel.to_string()),
        };

        let pool = self.db_pool.clone();
        tokio::spawn(async move {
            if let Ok(conn) = pool.get().await {
                let _ = conn
                    .interact(move |c| network_activity_log::insert(c, &log_row))
                    .await;
            }
        });

        response_result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_pool() -> Pool {
        let mgr = deadpool_sqlite::Manager::from_config(
            &deadpool_sqlite::Config {
                path: ":memory:".into(),
                pool: Some(deadpool_sqlite::PoolConfig::new(1)),
            },
            deadpool_sqlite::Runtime::Tokio1,
        );
        Pool::builder(mgr).build().unwrap()
    }

    /// Proves the timeout mechanism actually fires end-to-end: a real TCP
    /// listener that accepts the connection but never sends a response
    /// must cause `execute()` to return an `Err` once the configured
    /// timeout elapses, rather than hanging forever -- this is the exact
    /// failure mode (a stalled Gmail API connection) that previously froze
    /// a historical scan indefinitely with no error surfaced at all.
    #[tokio::test]
    async fn test_request_times_out_on_stalled_connection() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        // Accept the connection in the background and then just hold it
        // open without ever writing a response.
        std::thread::spawn(move || {
            let _accepted = listener.accept();
            std::thread::sleep(Duration::from_secs(30));
        });

        let network = NetworkClient::with_timeout(dummy_pool(), Duration::from_millis(200));
        let req = network.client().get(format!("http://{addr}/"));

        let start = std::time::Instant::now();
        let result = network.execute("gmail_api", req).await;
        let elapsed = start.elapsed();

        assert!(
            result.is_err(),
            "a stalled connection must time out, not hang forever"
        );
        assert!(
            result.unwrap_err().is_timeout(),
            "the error must specifically be a timeout"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "must time out near the configured 200ms, not wait indefinitely, got {:?}",
            elapsed
        );
    }
}
