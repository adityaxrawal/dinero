//! Batches row writes to keep ingestion from thrashing the database.
//!
//! A mailbox scan produces transactions far faster than they should be committed
//! one statement at a time. Writes are therefore accumulated and flushed either
//! when the batch fills or when it ages out, so a slow trickle is still
//! persisted promptly rather than waiting for a batch that may never fill.
//!
//! Retries exist because SQLite returns `SQLITE_BUSY` under concurrent access
//! rather than blocking, and a busy database during a scan is expected, not
//! exceptional.

use anyhow::Result;
use rusqlite::{Connection, ErrorCode};
use std::time::{Duration, Instant};

// Flush triggers: whichever comes first. The row cap bounds transaction size,
// and the duration cap bounds latency so a partial batch is not held
// indefinitely waiting for rows that never arrive.
pub const MAX_BATCH_ROWS: usize = 20;
pub const MAX_BATCH_DURATION: Duration = Duration::from_millis(500);
// SQLITE_BUSY is expected during a scan rather than exceptional, so contention
// is retried a few times before the write is treated as failed.
pub const MAX_BUSY_RETRIES: u32 = 3;

type Write = Box<dyn Fn(&Connection) -> rusqlite::Result<()> + Send>;

pub struct BatchWriter {
    pending: Vec<Write>,
    batch_started_at: Option<Instant>,
}

impl Default for BatchWriter {
    /// An empty writer with the standard batch limits.
    fn default() -> Self {
        Self::new()
    }
}

impl BatchWriter {
    /// Creates an empty batch writer.
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            batch_started_at: None,
        }
    }

    /// Queues one write, returning whether the batch is now ready to flush.
    pub fn push(
        &mut self,
        write: impl Fn(&Connection) -> rusqlite::Result<()> + Send + 'static,
    ) -> bool {
        if self.pending.is_empty() {
            self.batch_started_at = Some(Instant::now());
        }
        self.pending.push(Box::new(write));
        self.is_full()
    }

    /// Whether the row cap has been reached.
    pub fn is_full(&self) -> bool {
        self.pending.len() >= MAX_BATCH_ROWS
            || self
                .batch_started_at
                .map(|t| t.elapsed() >= MAX_BATCH_DURATION)
                .unwrap_or(false)
    }

    /// Whether anything is queued.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Number of queued writes.
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// Commits the queued writes in one transaction.
    ///
    /// Retries on SQLITE_BUSY, which is expected rather than exceptional during a
    /// scan: contention with the ingestion writers is normal, and failing the batch
    /// on first contact would drop rows that would have succeeded a moment later.
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

    /// Executes the queued writes inside a single transaction.
    ///
    /// One transaction rather than one per row is the entire point of batching --
    /// it is what keeps database contention from becoming the limit on scan speed.
    fn run_batch(conn: &mut Connection, writes: &[Write]) -> rusqlite::Result<()> {
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        for write in writes {
            write(&tx)?;
        }
        tx.commit()
    }

    /// Whether an error is SQLITE_BUSY, and therefore worth retrying.
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
        assert!(
            was_full,
            "batch must report full once it reaches MAX_BATCH_ROWS"
        );
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
        writer.push(insert_tx_write("tx_ok"));

        let result = writer.flush(&mut conn);
        assert!(result.is_err());

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            count, 0,
            "the whole batch must roll back, not just the failing write"
        );
    }
}
