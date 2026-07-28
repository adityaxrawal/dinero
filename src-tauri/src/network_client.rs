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
                // Google's frontend closes idle keep-alive connections well
                // under reqwest's 90s default pool_idle_timeout. A scan's
                // bursty concurrency (many requests firing at once after a
                // quiet moment) kept grabbing those stale pooled connections
                // and failing instantly with "error sending request" --
                // 306 of ~630 fetches in one logged scan -- each eating a
                // 2-6s retry backoff. Recycling connections before Google's
                // side does avoids the failure instead of retrying around it.
                .pool_idle_timeout(Duration::from_secs(30))
                // `pool_idle_timeout` alone only stops reqwest from reusing a
                // connection idle *longer* than 30s -- one that Google closed
                // at, say, 20s is still inside that window and still handed
                // out, so the request writes fine but the server never
                // answers, and we only notice once our own 15s REQUEST_TIMEOUT
                // fires (logged as "gmail request timed out" despite the
                // connection being the actual problem). HTTP/2 keepalive pings
                // idle connections proactively and evicts ones that don't
                // answer, so a dead connection is caught before being handed
                // to a real request at all, regardless of Google's actual
                // close threshold.
                .http2_keep_alive_interval(Duration::from_secs(10))
                .http2_keep_alive_timeout(Duration::from_secs(5))
                .http2_keep_alive_while_idle(true)
                // Doc 2026-07-28 mail scan performance (root cause of the
                // 35-minute scan): HTTP/2 multiplexes every concurrent
                // request over ONE physical connection. A historical scan
                // fires `MAX_CONCURRENT_FETCHES` requests at once -- under
                // h2 they all rode that single connection, so one hiccup
                // (Google rotating the connection, a Wi-Fi/VPN blip) killed
                // every in-flight request simultaneously and all of them
                // re-contended to re-establish the same one connection --
                // observed in logs as ~19 lockstep ~100s stalls (dozens of
                // "Spawning fetch" lines go silent, then everything finishes
                // in one burst) accounting for ~88% of total scan time. The
                // keepalive settings above stop *idle* connections going
                // stale but can't stop a *live* connection's hiccup from
                // taking every concurrent request down with it. Forcing
                // HTTP/1.1 makes reqwest open one connection per concurrent
                // request instead, so a dead connection only costs the one
                // request riding it (bounded by REQUEST_TIMEOUT + retry
                // below), not all of them. Do not remove this without
                // re-confirming the thundering-herd behavior is gone.
                .http1_only()
                .build()
                .expect("reqwest::Client::builder() with only a timeout set must always succeed"),
            db_pool,
        }
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Doc 30 TASK-API-006: `channel` is written directly into
    /// `network_activity_log.channel` rather than inferred later from the
    /// destination hostname (`commands/network.rs`'s read-time fallback,
    /// which silently produces "unknown" for any host it doesn't recognize).
    ///
    /// Doc 30 TASK-QA-007 finding: only 3 channels actually route through
    /// this client today (`gmail_api`/`licensing_backend`/`google_oauth`) --
    /// `llm_manager.rs`'s GGUF model download and `llama_sidecar.rs`'s
    /// llama.cpp release download both build their own bare `reqwest::Client`
    /// (huggingface.co / github.com), invisible to the Network Activity
    /// audit trail, and the auto-updater goes through `tauri-plugin-updater`
    /// entirely outside this module. A previous version of this comment
    /// claimed "5 disclosed channels" including `github_releases`/
    /// `huggingface` as if already wired -- they were not. Routing those
    /// through `NetworkClient` would need threading a `deadpool_sqlite::Pool`
    /// down through `download_file_with_hash`'s two call sites (one of which,
    /// `llama_sidecar::ensure_binary`, sits in the LLM sidecar startup path)
    /// -- flagged as real follow-up work, not fixed here to avoid risking
    /// that startup path in the same pass as an unrelated audit task.
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
