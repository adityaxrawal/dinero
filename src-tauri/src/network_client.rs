//! HTTP client wrapper that records every outbound request.
//!
//! All network access from the backend goes through here, which is what makes
//! the privacy disclosure enforceable rather than aspirational: each call is
//! tagged with a channel and written to the network activity log, so the
//! settings screen can show exactly what left the machine and when.
//!
//! Only request metadata is logged -- destination, timing, status -- never
//! request or response bodies.

use crate::db::network_activity_log::{self, NetworkActivityLogRow};
use chrono::Utc;
use deadpool_sqlite::Pool;
use reqwest::{Client, RequestBuilder, Response};
use std::time::Duration;
use uuid::Uuid;

// Every outbound request is bounded. An unbounded request would hang a
// background task indefinitely against an unresponsive host.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

pub struct NetworkClient {
    client: Client,
    db_pool: Pool,
}

impl NetworkClient {
    /// Builds a client with the default request timeout.
    pub fn new(db_pool: Pool) -> Self {
        Self::with_timeout(db_pool, REQUEST_TIMEOUT)
    }

    /// Builds a client with an explicit timeout, for tests.
    pub(crate) fn with_timeout(db_pool: Pool, timeout: Duration) -> Self {
        Self {
            client: Client::builder()
                .timeout(timeout)
                .pool_idle_timeout(Duration::from_secs(30))
                .http2_keep_alive_interval(Duration::from_secs(10))
                .http2_keep_alive_timeout(Duration::from_secs(5))
                .http2_keep_alive_while_idle(true)
                .build()
                .expect("reqwest::Client::builder() with only a timeout set must always succeed"),
            db_pool,
        }
    }

    /// The underlying reqwest client.
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Execute a request, logging its outcome to the network activity log.
    ///
    /// The `channel` names the disclosed purpose (Gmail, licensing, updates) and
    /// is what ties a logged entry back to a row in the privacy disclosure.
    pub async fn execute(
        &self,
        channel: &str,
        req_builder: RequestBuilder,
    ) -> reqwest::Result<Response> {
        let request = req_builder.build()?;

        let method = request.method().to_string();
        let url = request.url().clone();
        let domain = url.domain().unwrap_or("unknown").to_string();

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

        let latency_ms = (Utc::now().naive_utc() - start_time).num_milliseconds();
        match &response_result {
            Ok(res) => {
                tracing::info!(
                    target: "network",
                    method = %log_row.method,
                    domain = %log_row.domain,
                    url = %log_row.url_redacted,
                    status = res.status().as_u16(),
                    latency_ms = latency_ms,
                    bytes_sent = bytes_sent,
                    channel = channel,
                    "Network request succeeded"
                );
            }
            Err(e) => {
                tracing::error!(
                    target: "network",
                    method = %log_row.method,
                    domain = %log_row.domain,
                    url = %log_row.url_redacted,
                    error = %e,
                    latency_ms = latency_ms,
                    channel = channel,
                    "Network request failed"
                );
            }
        }

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

    #[tokio::test]
    async fn test_request_times_out_on_stalled_connection() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
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
