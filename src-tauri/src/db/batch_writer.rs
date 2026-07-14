//! TASK-DB-023: Transaction Bounding for Background Writes (Prevent SQLITE_BUSY).
//!
//! Every long-running background Tokio task (historical scan, poll worker,
//! statement parser) writes many rows over time. Wrapping the *entire* job
//! in one giant transaction would hold a write lock for the job's whole
//! duration, starving any other connection (including the foreground UI's
//! own reads/writes) with `SQLITE_BUSY` — a real problem given the Power
//! User persona's bulk-backfill case (≈20 cards × 5 years ≈ 1,200
//! statements). `BatchWriter` groups writes into short-lived
//! `BEGIN IMMEDIATE ... COMMIT` transactions bounded by size (max ~20 rows)
//! or duration (max ~500ms), and retries `SQLITE_BUSY` with exponential
//! backoff (max 3 retries) before surfacing an error.
//!
//! `BatchWriter` is deliberately synchronous — it never holds a write
//! transaction open across an `await` that could block on network I/O.
//! Callers queue writes as they produce them (e.g. while iterating parsed
//! Gmail messages) and call `flush()` only from inside a
//! `deadpool_sqlite::Connection::interact(...)` closure, which already runs
//! on rusqlite's own blocking thread, not the async task's.

use anyhow::Result;
use rusqlite::{Connection, ErrorCode};
use std::time::{Duration, Instant};

/// A batch is flushed once it reaches this many queued writes...
pub const MAX_BATCH_ROWS: usize = 20;
/// ...or once this much time has passed since the first write was queued,
/// whichever comes first.
pub const MAX_BATCH_DURATION: Duration = Duration::from_millis(500);
/// `SQLITE_BUSY` retries before a batch's error is surfaced to the caller.
pub const MAX_BUSY_RETRIES: u32 = 3;

type Write = Box<dyn Fn(&Connection) -> rusqlite::Result<()> + Send>;

/// Accumulates write operations and flushes them as a single bounded
/// transaction once either limit is reached, or when `flush()` is called
/// explicitly (e.g. at the end of a job, to commit a partial batch).
pub struct BatchWriter {
    pending: Vec<Write>,
    batch_started_at: Option<Instant>,
}

impl Default for BatchWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl BatchWriter {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            batch_started_at: None,
        }
    }

    /// Queues a write. Returns `true` once the batch has reached either
    /// bound (size or duration) and should be flushed by the caller —
    /// callers own the actual `flush()` call so a connection is only ever
    /// acquired at a point the caller controls, never mid-`.await`.
    pub fn push(&mut self, write: impl Fn(&Connection) -> rusqlite::Result<()> + Send + 'static) -> bool {
        if self.pending.is_empty() {
            self.batch_started_at = Some(Instant::now());
        }
        self.pending.push(Box::new(write));
        self.is_full()
    }

    pub fn is_full(&self) -> bool {
        self.pending.len() >= MAX_BATCH_ROWS
            || self
                .batch_started_at
                .map(|t| t.elapsed() >= MAX_BATCH_DURATION)
                .unwrap_or(false)
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// Flushes all pending writes as a single `BEGIN IMMEDIATE ... COMMIT`
    /// transaction. On `SQLITE_BUSY`, rolls back and retries the *whole*
    /// batch with exponential backoff (up to `MAX_BUSY_RETRIES` times)
    /// before surfacing the error — any other error rolls back and returns
    /// immediately, without retrying. Synchronous: call only from inside a
    /// `conn.interact(...)` closure, never across an `.await`.
    pub fn flush(&mut self, conn: &mut Connection) -> Result<usize> {
        if self.pending.is_empty() {
            return Ok(0);
        }
        let writes = std::mem::take(&mut self.pending);
        self.batch_started_at = None;
        let count = writes.len();

        let mut attempt = 0;
        loop {
            match Self::run_batch(conn, &writes) {
                Ok(()) => return Ok(count),
                Err(e) if Self::is_sqlite_busy(&e) && attempt < MAX_BUSY_RETRIES => {
                    attempt += 1;
                    let backoff = Duration::from_millis(50 * 2u64.pow(attempt - 1));
                    std::thread::sleep(backoff);
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    fn run_batch(conn: &mut Connection, writes: &[Write]) -> rusqlite::Result<()> {
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        for write in writes {
            write(&tx)?;
        }
        tx.commit()
    }

    fn is_sqlite_busy(err: &rusqlite::Error) -> bool {
        matches!(
            err,
            rusqlite::Error::SqliteFailure(e, _) if e.code == ErrorCode::DatabaseBusy
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> Connection {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute(
            "INSERT INTO instruments (id, type, issuer_name, masked_identifier, status) \
             VALUES ('inst_1', 'credit_card', 'HDFC', '1234', 'active')",
            [],
        )
        .unwrap();
        conn
    }

    fn insert_tx_write(id: impl Into<String>) -> impl Fn(&Connection) -> rusqlite::Result<()> {
        let id = id.into();
        move |c: &Connection| {
            c.execute(
                "INSERT INTO transactions (id, instrument_id, amount_minor, currency, direction, is_deleted) \
                 VALUES (?1, 'inst_1', 100, 'INR', 'debit', 0)",
                [&id],
            )
            .map(|_| ())
        }
    }

    #[test]
    fn push_reports_full_once_row_limit_reached() {
        let mut writer = BatchWriter::new();
        let mut was_full = false;
        for i in 0..MAX_BATCH_ROWS {
            was_full = writer.push(insert_tx_write(format!("tx_{}", i)));
        }
        assert!(was_full, "batch must report full once it reaches MAX_BATCH_ROWS");
        assert_eq!(writer.len(), MAX_BATCH_ROWS);
    }

    #[test]
    fn push_does_not_report_full_below_limits() {
        let mut writer = BatchWriter::new();
        let was_full = writer.push(insert_tx_write("tx_solo"));
        assert!(!was_full);
    }

    #[test]
    fn flush_commits_all_queued_writes_in_one_transaction() {
        let mut conn = setup_db();
        let mut writer = BatchWriter::new();
        writer.push(insert_tx_write("tx_a"));
        writer.push(insert_tx_write("tx_b"));
        writer.push(insert_tx_write("tx_c"));

        let flushed = writer.flush(&mut conn).unwrap();
        assert_eq!(flushed, 3);
        assert!(writer.is_empty());

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn flush_on_empty_batch_is_a_no_op() {
        let mut conn = setup_db();
        let mut writer = BatchWriter::new();
        assert_eq!(writer.flush(&mut conn).unwrap(), 0);
    }

    #[test]
    fn flush_rolls_back_the_whole_batch_on_a_real_error() {
        let mut conn = setup_db();
        let mut writer = BatchWriter::new();
        writer.push(insert_tx_write("tx_ok"));
        // Duplicate primary key -- a real, non-retryable constraint error.
        writer.push(insert_tx_write("tx_ok"));

        let result = writer.flush(&mut conn);
        assert!(result.is_err());

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "the whole batch must roll back, not just the failing write");
    }
}
