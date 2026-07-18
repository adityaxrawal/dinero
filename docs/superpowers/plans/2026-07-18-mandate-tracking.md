# Explicit Mandate Tracking Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Capture explicit bank mandate/AutoPay registration and cancellation emails as a persistent, cancellable `recurring_payments` record (in addition to the existing ₹0.00 transaction row), instead of losing them as Gate 2 `Unknown` false negatives or, worse, having no way to represent a cancellation at all.

**Architecture:** A third ingestion queue (Mandate Queue), parallel to the existing Transaction/Statement Queues, fed by two new `ContentClass` variants. Its consumer does mandate-specific DB work (upsert-active / match-and-cancel) and then reuses the *existing, unmodified* Transaction Queue for the ₹0 transaction side effect — no reconciliation-internal function is called directly, so there is exactly one write path for canonical transactions, unchanged by this feature.

**Tech Stack:** Rust (Tauri backend), `rusqlite`, `tokio::sync::mpsc`, existing `regex` crate for extraction — no new dependencies.

## Global Constraints

- No second reconciliation/canonical-write function (Ground Rule from the originating false-negative remediation session, still applies to the whole pipeline per Doc 15's single-pipeline invariant).
- No new UI in this pass (spec §3, non-goals) — `unresolved_mandate_cancellations` is queryable, not resolvable via any screen yet.
- All new SQL is additive (`ALTER TABLE ADD COLUMN`, `CREATE TABLE IF NOT EXISTS`) — no existing column/table is altered destructively.
- Doc edits are surgical and additive per every prior task in this session's `audit.md` — preserve numbering, propagate to every companion document listed in the spec's §7 table.
- Every new regex/keyword addition ships with a test built from the real false-negative body text already saved in `gmail_export/segregated_emails/false_negative/transaction/fn_transaction.json` (tx idx 55, 61, 62), not synthetic placeholders — this matched the whole originating session's own verification discipline and caught two real bugs (leftmost-match ambiguity, missing date formats) that synthetic tests would have hidden.

---

### Task 1: Migration — `recurring_payments` columns + `unresolved_mandate_cancellations` table

**Files:**
- Create: `src-tauri/migrations/20260101000042_add_mandate_tracking.sql`
- Test: manual (migrations are exercised by every existing DB test via the migration runner; no dedicated migration test file exists in this codebase — confirmed by `src-tauri/src/db/migrations.rs` convention).

**Interfaces:**
- Produces: `recurring_payments.source TEXT NOT NULL DEFAULT 'inferred'`, `recurring_payments.external_mandate_id TEXT`, table `unresolved_mandate_cancellations(id, raw_signal, candidate_ids, status, created_at, resolved_at)`.

- [ ] **Step 1: Write the migration file**

```sql
-- Explicit mandate tracking (docs/superpowers/specs/2026-07-18-mandate-tracking-design.md).
-- `source` distinguishes rows written by recurring_detector.rs's statistical
-- inference (existing rows backfill 'inferred', the default) from rows
-- written directly from an explicit bank mandate-registration email
-- ('explicit'). `external_mandate_id` is the bank's own mandate reference
-- (e.g. SBI Card's "SiHub ID"), nullable since not every bank prints one.
ALTER TABLE recurring_payments ADD COLUMN source TEXT NOT NULL DEFAULT 'inferred';
ALTER TABLE recurring_payments ADD COLUMN external_mandate_id TEXT;

-- Mirrors unprocessed_statements' blocking-on-user-input shape (Doc 18
-- §4.16-4.21): a cancellation email that couldn't be matched to exactly one
-- active recurring_payments row (by external_mandate_id, else by
-- merchant+instrument) is never guessed at -- it's logged here instead.
CREATE TABLE IF NOT EXISTS unresolved_mandate_cancellations (
    id TEXT PRIMARY KEY,
    raw_signal TEXT NOT NULL,
    candidate_ids TEXT,
    status TEXT NOT NULL DEFAULT 'unresolved',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    resolved_at DATETIME
);
```

- [ ] **Step 2: Run the app's test suite to confirm the migration applies cleanly**

Run: `cd src-tauri && cargo test --lib db::migrations`
Expected: existing migration tests pass (they run every migration in order against a fresh in-memory DB — a syntax error in the new file fails this immediately).

- [ ] **Step 3: Commit**

```bash
cd src-tauri && git add migrations/20260101000042_add_mandate_tracking.sql
git commit -m "feat(mandate): add recurring_payments columns and unresolved_mandate_cancellations table"
```

---

### Task 2: DB layer — `recurring_payments.rs` additions

**Files:**
- Modify: `src-tauri/src/db/recurring_payments.rs`
- Test: `src-tauri/src/db/recurring_payments.rs` (`mod tests` — file has none today; add one)

**Interfaces:**
- Consumes: `RecurringPaymentsRow` (existing struct, `recurring_payments.rs:6-19`), extended with `source: String` and `external_mandate_id: Option<String>`.
- Produces: `upsert_explicit(conn, &RecurringPaymentsRow) -> Result<String>` (returns the row id, inserting or updating by `(instrument_id, merchant_entity_id, source='explicit')`), `find_active_candidates_for_cancellation(conn, instrument_id: Option<&str>, merchant_entity_id: Option<&str>, external_mandate_id: Option<&str>) -> Result<Vec<RecurringPaymentsRow>>`, `mark_cancelled(conn, id: &str) -> Result<()>`.

- [ ] **Step 1: Extend `RecurringPaymentsRow` and existing `insert`/`update`/row-mapping**

In `src-tauri/src/db/recurring_payments.rs`, add the two fields to the struct (after `pub status: Option<String>,`):

```rust
    pub status: Option<String>,
    pub source: String,
    pub external_mandate_id: Option<String>,
    pub created_at: DateTime<Utc>,
```

Update `insert()` (existing, lines 23-42) to include the two new columns:

```rust
pub fn insert(conn: &Connection, row: &RecurringPaymentsRow) -> Result<()> {
    conn.execute(
        "INSERT INTO recurring_payments (
            id, merchant_entity_id, instrument_id, amount_minor, currency, cadence,
            next_billing_date, next_predicted_date, next_predicted_amount, confidence, status,
            source, external_mandate_id
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            row.id,
            row.merchant_entity_id,
            row.instrument_id,
            row.amount_minor,
            row.currency,
            row.cadence,
            row.next_billing_date,
            row.next_predicted_date,
            row.next_predicted_amount,
            row.confidence,
            row.status,
            row.source,
            row.external_mandate_id,
        ],
    )?;
    Ok(())
}
```

Update every existing `RecurringPaymentsRow { ... }` struct literal in this file's `get`/`select_active`/`find_by_instrument_and_merchant` row-mapping closures to also `r.get()` the two new columns (append `source: r.get(N)?, external_mandate_id: r.get(N+1)?,` matching each `SELECT` column list, which must also gain `, source, external_mandate_id` at the end). Update `insert_transaction`-style callers elsewhere in the codebase that construct `RecurringPaymentsRow` literals (only `recurring_detector.rs` does — confirmed via `grep -rn "RecurringPaymentsRow {" src-tauri/src`) to set `source: "inferred".to_string(), external_mandate_id: None,`.

- [ ] **Step 2: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE recurring_payments (
                id TEXT PRIMARY KEY, merchant_entity_id TEXT, instrument_id TEXT,
                amount_minor INTEGER, currency TEXT, cadence TEXT,
                next_billing_date TEXT, next_predicted_date TEXT, next_predicted_amount REAL,
                confidence REAL, status TEXT,
                source TEXT NOT NULL DEFAULT 'inferred', external_mandate_id TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_upsert_explicit_inserts_new_row_when_none_exists() {
        let conn = setup();
        let id = upsert_explicit(
            &conn,
            "instr-1",
            "merchant-1",
            Some(0),
            "INR",
            Some("monthly"),
            Some("SIHUB123"),
        )
        .unwrap();
        let row = get(&conn, &id).unwrap().unwrap();
        assert_eq!(row.status, Some("active".to_string()));
        assert_eq!(row.source, "explicit");
        assert_eq!(row.external_mandate_id, Some("SIHUB123".to_string()));
    }

    #[test]
    fn test_upsert_explicit_updates_existing_explicit_row_not_duplicate() {
        let conn = setup();
        let id1 = upsert_explicit(&conn, "instr-1", "merchant-1", Some(0), "INR", Some("monthly"), Some("SIHUB123")).unwrap();
        let id2 = upsert_explicit(&conn, "instr-1", "merchant-1", Some(0), "INR", Some("monthly"), Some("SIHUB123")).unwrap();
        assert_eq!(id1, id2, "second registration for the same instrument+merchant must update, not duplicate");
    }

    #[test]
    fn test_find_active_candidates_matches_by_external_mandate_id_first() {
        let conn = setup();
        upsert_explicit(&conn, "instr-1", "merchant-1", Some(0), "INR", Some("monthly"), Some("SIHUB123")).unwrap();
        let candidates = find_active_candidates_for_cancellation(&conn, Some("instr-1"), Some("merchant-1"), Some("SIHUB123")).unwrap();
        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn test_find_active_candidates_zero_when_no_active_row() {
        let conn = setup();
        let candidates = find_active_candidates_for_cancellation(&conn, Some("instr-1"), Some("merchant-1"), None).unwrap();
        assert_eq!(candidates.len(), 0);
    }

    #[test]
    fn test_mark_cancelled_sets_status() {
        let conn = setup();
        let id = upsert_explicit(&conn, "instr-1", "merchant-1", Some(0), "INR", Some("monthly"), None).unwrap();
        mark_cancelled(&conn, &id).unwrap();
        let row = get(&conn, &id).unwrap().unwrap();
        assert_eq!(row.status, Some("cancelled".to_string()));
    }
}
```

- [ ] **Step 3: Run to verify failure**

Run: `cd src-tauri && cargo test --lib db::recurring_payments`
Expected: FAIL — `upsert_explicit`, `find_active_candidates_for_cancellation`, `mark_cancelled` not defined.

- [ ] **Step 4: Implement the three functions**

Add to `src-tauri/src/db/recurring_payments.rs`:

```rust
/// Inserts or updates the explicit-source recurring_payments row for this
/// (instrument, merchant) pair. Explicit and inferred rows never share an
/// identity even for the same instrument+merchant -- the WHERE clause below
/// pins `source = 'explicit'` so an inferred row (recurring_detector.rs)
/// is never silently overwritten by an explicit registration, or vice versa.
pub fn upsert_explicit(
    conn: &Connection,
    instrument_id: &str,
    merchant_entity_id: &str,
    amount_minor: Option<i64>,
    currency: &str,
    cadence: Option<&str>,
    external_mandate_id: Option<&str>,
) -> Result<String> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM recurring_payments
             WHERE instrument_id = ?1 AND merchant_entity_id = ?2 AND source = 'explicit'",
            params![instrument_id, merchant_entity_id],
            |r| r.get(0),
        )
        .optional()?;

    if let Some(id) = existing {
        conn.execute(
            "UPDATE recurring_payments SET
                amount_minor = ?2, currency = ?3, cadence = ?4, status = 'active',
                external_mandate_id = ?5, updated_at = CURRENT_TIMESTAMP
             WHERE id = ?1",
            params![id, amount_minor, currency, cadence, external_mandate_id],
        )?;
        Ok(id)
    } else {
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO recurring_payments (
                id, merchant_entity_id, instrument_id, amount_minor, currency, cadence,
                status, source, external_mandate_id
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', 'explicit', ?7)",
            params![id, merchant_entity_id, instrument_id, amount_minor, currency, cadence, external_mandate_id],
        )?;
        Ok(id)
    }
}

/// Candidates for a cancellation email to match, in precedence order: an
/// exact `external_mandate_id` match (if the cancellation email carried one)
/// short-circuits to a single-element result; otherwise falls back to every
/// active row for the same (instrument, merchant) pair. Never guesses beyond
/// what's returned here -- the caller decides zero/one/many.
pub fn find_active_candidates_for_cancellation(
    conn: &Connection,
    instrument_id: Option<&str>,
    merchant_entity_id: Option<&str>,
    external_mandate_id: Option<&str>,
) -> Result<Vec<RecurringPaymentsRow>> {
    if let Some(mandate_id) = external_mandate_id {
        let mut stmt = conn.prepare(
            "SELECT id, merchant_entity_id, instrument_id, amount_minor, currency, cadence,
                    next_billing_date, next_predicted_date, next_predicted_amount, confidence, status,
                    source, external_mandate_id, created_at, updated_at
             FROM recurring_payments WHERE external_mandate_id = ?1 AND status = 'active'",
        )?;
        let rows = stmt
            .query_map(params![mandate_id], row_from_sql)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if !rows.is_empty() {
            return Ok(rows);
        }
    }

    let (instrument_id, merchant_entity_id) = match (instrument_id, merchant_entity_id) {
        (Some(i), Some(m)) => (i, m),
        _ => return Ok(vec![]),
    };
    let mut stmt = conn.prepare(
        "SELECT id, merchant_entity_id, instrument_id, amount_minor, currency, cadence,
                next_billing_date, next_predicted_date, next_predicted_amount, confidence, status,
                source, external_mandate_id, created_at, updated_at
         FROM recurring_payments
         WHERE instrument_id = ?1 AND merchant_entity_id = ?2 AND status = 'active'",
    )?;
    let rows = stmt
        .query_map(params![instrument_id, merchant_entity_id], row_from_sql)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn row_from_sql(r: &rusqlite::Row) -> rusqlite::Result<RecurringPaymentsRow> {
    Ok(RecurringPaymentsRow {
        id: r.get(0)?,
        merchant_entity_id: r.get(1)?,
        instrument_id: r.get(2)?,
        amount_minor: r.get(3)?,
        currency: r.get(4)?,
        cadence: r.get(5)?,
        next_billing_date: r.get(6)?,
        next_predicted_date: r.get(7)?,
        next_predicted_amount: r.get(8)?,
        confidence: r.get(9)?,
        status: r.get(10)?,
        source: r.get(11)?,
        external_mandate_id: r.get(12)?,
        created_at: r.get(13)?,
        updated_at: r.get(14)?,
    })
}

pub fn mark_cancelled(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE recurring_payments SET status = 'cancelled', updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}
```

- [ ] **Step 5: Run to verify all pass**

Run: `cd src-tauri && cargo test --lib db::recurring_payments`
Expected: PASS, all 5 new tests + any pre-existing ones in this module.

- [ ] **Step 6: Commit**

```bash
cd src-tauri && git add src/db/recurring_payments.rs
git commit -m "feat(mandate): add upsert_explicit/find_active_candidates_for_cancellation/mark_cancelled"
```

---

### Task 3: DB layer — `unresolved_mandate_cancellations.rs`

**Files:**
- Create: `src-tauri/src/db/unresolved_mandate_cancellations.rs`
- Modify: `src-tauri/src/db/mod.rs` (register module)
- Test: same file, `mod tests`

**Interfaces:**
- Produces: `insert_unresolved(conn, raw_signal: &str, candidate_ids: &[String]) -> Result<String>`.

- [ ] **Step 1: Write the failing test**

```rust
use anyhow::Result;
use rusqlite::{params, Connection};

pub fn insert_unresolved(conn: &Connection, raw_signal: &str, candidate_ids: &[String]) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let candidate_ids_json = serde_json::to_string(candidate_ids)?;
    conn.execute(
        "INSERT INTO unresolved_mandate_cancellations (id, raw_signal, candidate_ids) VALUES (?1, ?2, ?3)",
        params![id, raw_signal, candidate_ids_json],
    )?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE unresolved_mandate_cancellations (
                id TEXT PRIMARY KEY, raw_signal TEXT NOT NULL, candidate_ids TEXT,
                status TEXT NOT NULL DEFAULT 'unresolved',
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, resolved_at TEXT
            )",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_insert_unresolved_with_zero_candidates() {
        let conn = setup();
        let id = insert_unresolved(&conn, r#"{"merchant":"ScribdInc"}"#, &[]).unwrap();
        let raw: String = conn
            .query_row("SELECT raw_signal FROM unresolved_mandate_cancellations WHERE id = ?1", params![id], |r| r.get(0))
            .unwrap();
        assert_eq!(raw, r#"{"merchant":"ScribdInc"}"#);
    }

    #[test]
    fn test_insert_unresolved_with_multiple_candidates() {
        let conn = setup();
        let id = insert_unresolved(&conn, "{}", &["id-1".to_string(), "id-2".to_string()]).unwrap();
        let candidates_json: String = conn
            .query_row("SELECT candidate_ids FROM unresolved_mandate_cancellations WHERE id = ?1", params![id], |r| r.get(0))
            .unwrap();
        let candidates: Vec<String> = serde_json::from_str(&candidates_json).unwrap();
        assert_eq!(candidates, vec!["id-1", "id-2"]);
    }
}
```

- [ ] **Step 2: Register the module**

In `src-tauri/src/db/mod.rs`, add alphabetically among the existing `pub mod` lines:

```rust
pub mod unresolved_mandate_cancellations;
```

- [ ] **Step 3: Run to verify pass**

Run: `cd src-tauri && cargo test --lib db::unresolved_mandate_cancellations`
Expected: PASS, 2 tests.

- [ ] **Step 4: Commit**

```bash
cd src-tauri && git add src/db/unresolved_mandate_cancellations.rs src/db/mod.rs
git commit -m "feat(mandate): add unresolved_mandate_cancellations DB module"
```

---

### Task 4: Gate 2 — `MandateRegistration`/`MandateCancellation` content classes

**Files:**
- Modify: `src-tauri/src/ingestion/content_classifier.rs`
- Modify: `src-tauri/src/ingestion/content_classifier_tests.rs`

**Interfaces:**
- Produces: `ContentClass::MandateRegistration`, `ContentClass::MandateCancellation` (new enum variants).

- [ ] **Step 1: Write the failing tests**

Add to `content_classifier_tests.rs`:

```rust
#[test]
fn test_mandate_registration_classified_correctly() {
    // tx idx 61 (real body, gmail false-negative corpus).
    assert_eq!(
        ContentClassifier::classify(
            "Registration Success: e-Mandate set at merchant using SBI Credit Card",
            "Your e-Mandate set at merchant with SBI Credit Card ending 7603 has been registered. Merchant: ScribdInc. Also, please note that you have authorised debit of INR. 0.00 from your account towards the first Trxn. against this e-Mandate."
        ),
        ContentClass::MandateRegistration
    );
    // tx idx 55 (Axis AutoPay, migrated from Cluster D).
    assert_eq!(
        ContentClassifier::classify(
            "AutoPay for ScribdInc: ACTIVATED",
            "Here's the summary of your successful AutoPay transaction: Transaction Amount: INR 0.00 Merchant Name: ScribdInc"
        ),
        ContentClass::MandateRegistration
    );
}

#[test]
fn test_mandate_cancellation_classified_correctly() {
    // tx idx 62 (real body, gmail false-negative corpus).
    assert_eq!(
        ContentClassifier::classify(
            "e-mandate Cancellation on your SBI Credit Card",
            "We observe that you have cancelled your E-mandate for SiHub ID: YPCojLhIn2 on SBI Credit Card ending 7603. The below E-mandate stands cancelled: Merchant: ScribdInc"
        ),
        ContentClass::MandateCancellation
    );
}

#[test]
fn test_mandate_registration_not_swallowed_by_transaction_verb_check() {
    // The registration body contains "authorised debit of INR. 0.00" --
    // must classify as MandateRegistration, not fall through to
    // TransactionAlert via the debit-verb check.
    assert_eq!(
        ContentClassifier::classify(
            "Registration Success: e-Mandate set at merchant",
            "you have authorised debit of INR. 0.00 from your account towards the first Trxn. against this e-Mandate."
        ),
        ContentClass::MandateRegistration
    );
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri && cargo test --lib content_classifier`
Expected: FAIL — `ContentClass::MandateRegistration`/`MandateCancellation` don't exist.

- [ ] **Step 3: Add the variants and detection, remove the superseded Cluster D branch**

In `content_classifier.rs`, extend the enum:

```rust
pub enum ContentClass {
    TransactionAlert,
    BalanceUpdate,
    StatementEmail,
    MandateRegistration,
    MandateCancellation,
    Noise,
    Otp,
    Kyc,
    Marketing,
    Reminder,
    Unknown,
}
```

Remove the `"successful autopay transaction"` branch added for Cluster D from `has_transaction_verb()` (superseded — routing now happens earlier):

```rust
fn has_transaction_verb(content: &str) -> bool {
    content.contains("spent")
        || content.contains("debited")
        || content.contains("credited")
        || content.contains("transaction alert")
        || content.contains("payment of")
        || content.contains("purchase of")
        || content.contains("you paid")
}
```

Add two new detection functions and call them in `classify()` immediately after the existing Statement check (step 3) and before the settled-transaction computation (step 3.5, so mandate language is checked before any transaction-verb logic runs at all):

```rust
fn is_mandate_cancellation(content: &str) -> bool {
    content.contains("mandate cancelled")
        || content.contains("mandate cancellation")
        || content.contains("e-mandate cancellation")
        || content.contains("mandate stands cancelled")
        || content.contains("autopay deactivated")
        || content.contains("autopay cancelled")
}

fn is_mandate_registration(content: &str) -> bool {
    content.contains("mandate registered")
        || content.contains("mandate set at merchant")
        || content.contains("e-mandate created")
        || content.contains("registration success")
        || content.contains("autopay activated")
        || content.contains("successful autopay transaction")
}
```

In `classify()`, after the existing statement check (`if subject_lower.contains("statement") ...`):

```rust
        // gmail false-negative remediation / mandate-tracking design
        // (docs/superpowers/specs/2026-07-18-mandate-tracking-design.md):
        // checked before any transaction-verb logic, since mandate emails
        // legitimately contain debit-shaped language ("authorised debit of
        // INR 0.00") that must not fall through to TransactionAlert.
        if is_mandate_cancellation(&content) {
            return ContentClass::MandateCancellation;
        }
        if is_mandate_registration(&content) {
            return ContentClass::MandateRegistration;
        }
```

- [ ] **Step 4: Run to verify pass**

Run: `cd src-tauri && cargo test --lib content_classifier`
Expected: PASS — includes the 3 new tests, plus `test_autopay_activation_classified_as_transaction` now needs updating (it currently asserts `ContentClass::TransactionAlert`; change its assertion to `ContentClass::MandateRegistration` since routing changed, per spec §6).

- [ ] **Step 5: Update the superseded Cluster D test**

In `content_classifier_tests.rs`, change `test_autopay_activation_classified_as_transaction`'s expected value:

```rust
#[test]
fn test_autopay_activation_classified_as_mandate_registration() {
    // Supersedes Cluster D's original "captured as TransactionAlert"
    // decision -- docs/superpowers/specs/2026-07-18-mandate-tracking-design.md
    // §6 migrates this to the Mandate Queue instead.
    assert_eq!(
        ContentClassifier::classify(
            "AutoPay for ScribdInc: ACTIVATED",
            "Here's the summary of your successful AutoPay transaction: Transaction Amount: INR 0.00 Merchant Name: ScribdInc"
        ),
        ContentClass::MandateRegistration
    );
}
```

(Renamed from `test_autopay_activation_classified_as_transaction` — delete the old function, this replaces it, not adds alongside it.)

- [ ] **Step 6: Run full content_classifier suite**

Run: `cd src-tauri && cargo test --lib content_classifier`
Expected: PASS, 0 failures.

- [ ] **Step 7: Commit**

```bash
cd src-tauri && git add src/ingestion/content_classifier.rs src/ingestion/content_classifier_tests.rs
git commit -m "feat(mandate): add MandateRegistration/MandateCancellation content classes"
```

---

### Task 5: `mandate_extractor.rs` — field extraction

**Files:**
- Create: `src-tauri/src/extraction/mandate_extractor.rs`
- Modify: `src-tauri/src/extraction/mod.rs` (register module)

**Interfaces:**
- Consumes: nothing external — pure string-in, struct-out, mirrors `GenericRegexLayer`'s merchant-keyword heuristic (reuses the same alternation as `ladder.rs`'s `GENERIC_MERCHANT_RE_STRICT`, not reimplemented).
- Produces: `pub struct MandateExtraction { pub merchant: Option<String>, pub cadence: Option<String>, pub max_limit_amount: Option<i64>, pub external_mandate_id: Option<String>, pub instrument_type: Option<String>, pub issuer_name: Option<String>, pub masked_identifier: Option<String> }`, `pub fn extract_mandate_fields(bank_name: &str, body: &str) -> Option<MandateExtraction>` (returns `None` if `merchant` can't be found — the mandatory-field gate for this queue).

- [ ] **Step 1: Write the failing tests**

```rust
use crate::extraction::mandate_extractor::extract_mandate_fields;

#[tokio::test]
async fn test_extract_mandate_fields_sbi_card_registration() {
    let body = "Dear Cardholder, Thank you for registering for a recurring e-Mandate at merchant platform using your SBI Credit Card. Your e-Mandate set at merchant with SBI Credit Card ending 7603 has been registered. Merchant: ScribdInc Description: PremiumMonthlyMembership e-Mandate Limit Amount (INR): 1000.00 Frequency: monthly Start date: 21/04/2026 End date: 21/04/2046 SiHub ID: YPCojLhIn2 Also, please note that you have authorised debit of INR. 0.00 from your account towards the first Trxn. against this e-Mandate.";
    let result = extract_mandate_fields("SBI Card", body).unwrap();
    assert_eq!(result.merchant, Some("ScribdInc".to_string()));
    assert_eq!(result.cadence, Some("monthly".to_string()));
    assert_eq!(result.max_limit_amount, Some(100000));
    assert_eq!(result.external_mandate_id, Some("YPCojLhIn2".to_string()));
    assert_eq!(result.instrument_type, Some("credit_card".to_string()));
    assert_eq!(result.masked_identifier, Some("7603".to_string()));
}

#[tokio::test]
async fn test_extract_mandate_fields_sbi_card_cancellation() {
    let body = "Dear Cardholder, Thank you for registering for a recurring E-mandate at merchant platform using your SBI Credit Card. We observe that you have cancelled your E-mandate for SiHub ID: YPCojLhIn2 on SBI Credit Card ending 7603. The below E-mandate stands cancelled: Merchant: ScribdInc Description: PremiumMonthlyMembership";
    let result = extract_mandate_fields("SBI Card", body).unwrap();
    assert_eq!(result.merchant, Some("ScribdInc".to_string()));
    assert_eq!(result.external_mandate_id, Some("YPCojLhIn2".to_string()));
    assert_eq!(result.masked_identifier, Some("7603".to_string()));
}

#[tokio::test]
async fn test_extract_mandate_fields_returns_none_without_merchant() {
    let body = "This email has no merchant label at all.";
    assert!(extract_mandate_fields("Any Bank", body).is_none());
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri && cargo test --lib extraction::mandate_extractor`
Expected: FAIL — module doesn't exist.

- [ ] **Step 3: Implement**

```rust
use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MandateExtraction {
    pub merchant: Option<String>,
    pub cadence: Option<String>,
    pub max_limit_amount: Option<i64>,
    pub external_mandate_id: Option<String>,
    pub instrument_type: Option<String>,
    pub issuer_name: Option<String>,
    pub masked_identifier: Option<String>,
}

static MERCHANT_RE: OnceLock<Regex> = OnceLock::new();
static CADENCE_RE: OnceLock<Regex> = OnceLock::new();
static AMOUNT_RE: OnceLock<Regex> = OnceLock::new();
static MANDATE_ID_RE: OnceLock<Regex> = OnceLock::new();
static CARD_LAST4_RE: OnceLock<Regex> = OnceLock::new();

/// Extracts mandate-lifecycle fields from a mandate registration/cancellation
/// email body. `merchant` is the only mandatory field (mirrors Gate 3's
/// precision-over-recall discipline, Doc 12 §6.2) -- returns `None` entirely
/// if it can't be found, same "reject rather than guess" posture as every
/// other gate in this pipeline.
pub fn extract_mandate_fields(bank_name: &str, body: &str) -> Option<MandateExtraction> {
    // Reuses GenericRegexLayer's merchant-keyword alternation
    // (ladder.rs GENERIC_MERCHANT_RE_STRICT) rather than reimplementing it --
    // "Merchant Name:"/"Merchant:" labels are the same convention.
    let merchant_re = MERCHANT_RE.get_or_init(|| {
        Regex::new(r"(?i)\b(?:merchant name|merchant):?\s+([A-Za-z0-9\s*]{2,40}?)(?:\s+description\b|\s+on\b|[,.\n\-]|$)").unwrap()
    });
    let merchant = merchant_re
        .captures(body)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .filter(|m| !m.is_empty())?;

    let cadence_re = CADENCE_RE.get_or_init(|| {
        Regex::new(r"(?i)frequency:?\s+(monthly|weekly|daily|yearly|quarterly)").unwrap()
    });
    let cadence = cadence_re
        .captures(body)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_lowercase());

    let amount_re = AMOUNT_RE.get_or_init(|| {
        Regex::new(r"(?i)(?:limit amount|max limit)\s*(?:\(inr\))?:?\s*(?:inr)?\s*([\d,]+(?:\.\d{1,2})?)").unwrap()
    });
    let max_limit_amount = amount_re
        .captures(body)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().replace(',', "").parse::<f64>().ok())
        .map(|f| (f * 100.0).round() as i64);

    let mandate_id_re = MANDATE_ID_RE.get_or_init(|| {
        Regex::new(r"(?i)(?:sihub id|mandate id|mandate reference|umrn):?\s+([A-Za-z0-9]{4,20})").unwrap()
    });
    let external_mandate_id = mandate_id_re
        .captures(body)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string());

    let card_last4_re = CARD_LAST4_RE.get_or_init(|| {
        Regex::new(r"(?i)ending\s+(\d{4})").unwrap()
    });
    let masked_identifier = card_last4_re
        .captures(body)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string());

    let instrument_type = if body.to_lowercase().contains("credit card") {
        Some("credit_card".to_string())
    } else {
        None
    };

    Some(MandateExtraction {
        merchant: Some(merchant),
        cadence,
        max_limit_amount,
        external_mandate_id,
        instrument_type,
        issuer_name: Some(bank_name.to_string()),
        masked_identifier,
    })
}
```

- [ ] **Step 4: Register the module**

In `src-tauri/src/extraction/mod.rs`, add alphabetically:

```rust
pub mod mandate_extractor;
```

- [ ] **Step 5: Run to verify pass**

Run: `cd src-tauri && cargo test --lib extraction::mandate_extractor`
Expected: PASS, 3 tests. (If `test_extract_mandate_fields_sbi_card_registration`'s `max_limit_amount` assertion fails because the regex matched "1000.00" from "e-Mandate Limit Amount (INR): 1000.00" incorrectly, adjust the regex's label alternation order — verify by printing the actual captured value, don't guess a fix blind.)

- [ ] **Step 6: Commit**

```bash
cd src-tauri && git add src/extraction/mandate_extractor.rs src/extraction/mod.rs
git commit -m "feat(mandate): add mandate_extractor for registration/cancellation field extraction"
```

---

### Task 6: `message_processor.rs` — `ProcessResult::MandateEvent` + Gate 2 dispatch

**Files:**
- Modify: `src-tauri/src/ingestion/message_processor.rs`

**Interfaces:**
- Consumes: `ContentClass::MandateRegistration`/`MandateCancellation` (Task 4), `extract_mandate_fields` (Task 5).
- Produces: `ProcessResult::MandateEvent(MandateExtraction, MandateEventType, ExtractedMessage)` where `MandateEventType` is a new small enum `{ Registration, Cancellation }`.

- [ ] **Step 1: Add the new `ProcessResult` variant and `MandateEventType`**

In `message_processor.rs`, extend the existing enum (currently `TransactionAlert`/`StatementEmail`, lines 14-21):

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum MandateEventType {
    Registration,
    Cancellation,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProcessResult {
    TransactionAlert(
        ExtractedMessage,
        Box<crate::extraction::ladder::ExtractionResult>,
    ),
    StatementEmail(ExtractedMessage),
    MandateEvent(
        ExtractedMessage,
        crate::extraction::mandate_extractor::MandateExtraction,
        MandateEventType,
    ),
}
```

- [ ] **Step 2: Add the Gate 2 dispatch branch**

In `process_message` (around line 105-152, the `match content_class` block), add two new arms before the existing `ContentClass::TransactionAlert | ContentClass::BalanceUpdate` arm:

```rust
                ContentClass::MandateRegistration | ContentClass::MandateCancellation => {
                    let event_type = if content_class == ContentClass::MandateRegistration {
                        MandateEventType::Registration
                    } else {
                        MandateEventType::Cancellation
                    };
                    match crate::extraction::mandate_extractor::extract_mandate_fields(
                        &current_bank_name,
                        body_text,
                    ) {
                        Some(extraction) => {
                            Self::append_to_scan_log(
                                message_id,
                                "SELECTED",
                                "mandate_event",
                                Some(&serde_json::to_value(&full_msg).unwrap_or_default()),
                                Some(body_text),
                            )
                            .await;
                            return Ok(Some(ProcessResult::MandateEvent(
                                extracted, extraction, event_type,
                            )));
                        }
                        None => {
                            crate::ingestion::gmail_telemetry::gmail_telemetry()
                                .record_gate_rejection("gate3");
                            Self::log_rejection(pool, message_id, "mandate_missing_merchant").await?;
                            Self::append_to_scan_log(
                                message_id,
                                "REJECTED",
                                "mandate_missing_merchant",
                                Some(&serde_json::to_value(&full_msg).unwrap_or_default()),
                                Some(body_text),
                            )
                            .await;
                            return Ok(None);
                        }
                    }
                }
```

(`current_bank_name` is already in scope — the existing Gate 1 binding, same variable `ContentClass::StatementEmail`'s arm above it uses implicitly via `extracted`.)

- [ ] **Step 3: Run existing message_processor tests to confirm no regression**

Run: `cd src-tauri && cargo test --lib message_processor`
Expected: PASS, all pre-existing tests unaffected (new arm is additive to an exhaustive `match`, so this only compiles if `ContentClass::MandateRegistration | ContentClass::MandateCancellation` weren't already implicitly covered by `_` — confirm no compile warning about unreachable patterns).

- [ ] **Step 4: Commit**

```bash
cd src-tauri && git add src/ingestion/message_processor.rs
git commit -m "feat(mandate): add ProcessResult::MandateEvent and Gate 2 dispatch"
```

---

### Task 7: `queues.rs` — Mandate Queue

**Files:**
- Modify: `src-tauri/src/ingestion/queues.rs`

**Interfaces:**
- Consumes: `ProcessResult::MandateEvent` (Task 6), `upsert_explicit`/`find_active_candidates_for_cancellation`/`mark_cancelled` (Task 2), `insert_unresolved` (Task 3), `get_or_create_instrument` (existing, `db/instruments.rs:240`), `normalize_merchant_sync` (existing, `extraction/merchant_normalizer.rs`), `TransactionJob` (existing, `queues.rs:18-30`).
- Produces: `MandateJob` struct, `QueueHandles.mandate_tx: mpsc::Sender<MandateJob>`, `spawn_mandate_workers`.

- [ ] **Step 1: Add `MandateJob` and extend `QueueHandles`**

```rust
/// One classified, Gate-3-equivalent mandate registration/cancellation
/// event, ready for recurring_payments upsert/cancellation-matching (Doc
/// 12 §6.2a extension, docs/superpowers/specs/2026-07-18-mandate-tracking-design.md).
pub struct MandateJob {
    pub extraction: crate::extraction::mandate_extractor::MandateExtraction,
    pub event_type: crate::ingestion::message_processor::MandateEventType,
    pub source_pipeline: String,
    pub source_record_id: String,
    pub connected_account_id: String,
    pub raw_body: Option<String>,
}
```

Extend `QueueHandles` (existing, lines 92-96):

```rust
#[derive(Clone)]
pub struct QueueHandles {
    pub transaction_tx: mpsc::Sender<TransactionJob>,
    pub statement_tx: mpsc::Sender<StatementJob>,
    pub mandate_tx: mpsc::Sender<MandateJob>,
}
```

Add a capacity constant near the existing two (line 103-104):

```rust
pub(crate) const MANDATE_QUEUE_CAPACITY: usize = 64;
```

- [ ] **Step 2: Implement `process_mandate_job`**

```rust
/// Processes one mandate event: upserts/matches-and-cancels the
/// recurring_payments row, then sends a synthesized TransactionJob onto the
/// *existing* Transaction Queue for the ₹0.00 transaction side effect --
/// reusing process_transaction_job unmodified rather than calling
/// reconciliation internals directly (spec §4.4 correction: the real single
/// entry point is reconcile_transactionally via process_transaction_job,
/// not create_canonical_transaction alone).
async fn process_mandate_job(job: MandateJob, pool: &Pool, transaction_tx: &mpsc::Sender<TransactionJob>) {
    let extraction = job.extraction.clone();
    let event_type = job.event_type.clone();

    let Ok(conn) = pool.get().await else { return };
    let merchant_raw = extraction.merchant.clone();

    let outcome = conn
        .interact(move |c| -> Option<String> {
            let instrument_id = if let (Some(itype), Some(iname), Some(masked)) = (
                &extraction.instrument_type,
                &extraction.issuer_name,
                &extraction.masked_identifier,
            ) {
                crate::db::instruments::get_or_create_instrument(c, itype, iname, masked, None).ok()
            } else {
                None
            };
            let merchant_entity_id = extraction
                .merchant
                .as_deref()
                .and_then(|m| crate::extraction::merchant_normalizer::normalize_merchant_sync(c, m).ok())
                .map(|(entity_id, _)| entity_id);

            match event_type {
                crate::ingestion::message_processor::MandateEventType::Registration => {
                    if let (Some(instrument_id), Some(merchant_entity_id)) = (&instrument_id, &merchant_entity_id) {
                        let _ = crate::db::recurring_payments::upsert_explicit(
                            c,
                            instrument_id,
                            merchant_entity_id,
                            extraction.max_limit_amount,
                            "INR",
                            extraction.cadence.as_deref(),
                            extraction.external_mandate_id.as_deref(),
                        );
                    }
                }
                crate::ingestion::message_processor::MandateEventType::Cancellation => {
                    let candidates = crate::db::recurring_payments::find_active_candidates_for_cancellation(
                        c,
                        instrument_id.as_deref(),
                        merchant_entity_id.as_deref(),
                        extraction.external_mandate_id.as_deref(),
                    )
                    .unwrap_or_default();
                    match candidates.len() {
                        1 => {
                            let _ = crate::db::recurring_payments::mark_cancelled(c, &candidates[0].id);
                        }
                        _ => {
                            let raw_signal = serde_json::json!({
                                "merchant": extraction.merchant,
                                "external_mandate_id": extraction.external_mandate_id,
                                "instrument_id": instrument_id,
                            })
                            .to_string();
                            let candidate_ids: Vec<String> = candidates.iter().map(|r| r.id.clone()).collect();
                            let _ = crate::db::unresolved_mandate_cancellations::insert_unresolved(
                                c,
                                &raw_signal,
                                &candidate_ids,
                            );
                        }
                    }
                }
            }
            instrument_id
        })
        .await
        .ok()
        .flatten();

    // Both registration and (successfully matched) cancellation also
    // produce the ₹0 transaction, via the unmodified Transaction Queue.
    let tx_job = TransactionJob {
        obs: crate::extraction::ladder::ExtractionResult {
            amount_minor: Some(0),
            currency: Some("INR".to_string()),
            direction: Some("debit".to_string()),
            merchant_raw,
            extraction_method: "mandate_event".to_string(),
            instrument_type: job.extraction.instrument_type.clone(),
            issuer_name: job.extraction.issuer_name.clone(),
            masked_identifier: job.extraction.masked_identifier.clone(),
            ..Default::default()
        },
        source_pipeline: job.source_pipeline,
        source_record_id: job.source_record_id,
        connected_account_id: job.connected_account_id,
        raw_body: job.raw_body,
    };
    let _ = outcome; // instrument_id already threaded into tx_job.obs above via instrument_type/issuer_name/masked_identifier for re-resolution in process_transaction_job.
    if transaction_tx.send(tx_job).await.is_err() {
        tracing::error!("Transaction Queue closed — dropping mandate-generated ₹0 transaction job");
    }
}
```

- [ ] **Step 3: Spawn the queue in `spawn_queues`**

Modify `spawn_queues` (existing, lines 221-237):

```rust
pub fn spawn_queues<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    pool: Pool,
    pending_bytes: PendingStatementBytes,
) -> QueueHandles {
    let (transaction_tx, transaction_rx) =
        mpsc::channel::<TransactionJob>(TRANSACTION_QUEUE_CAPACITY);
    let (statement_tx, statement_rx) = mpsc::channel::<StatementJob>(STATEMENT_QUEUE_CAPACITY);
    let (mandate_tx, mandate_rx) = mpsc::channel::<MandateJob>(MANDATE_QUEUE_CAPACITY);

    spawn_transaction_workers(transaction_rx, pool.clone(), app.clone());
    spawn_statement_dispatcher(statement_rx, pool.clone(), app, pending_bytes);
    spawn_mandate_workers(mandate_rx, pool, transaction_tx.clone());

    QueueHandles {
        transaction_tx,
        statement_tx,
        mandate_tx,
    }
}

/// Single dispatcher for the Mandate Queue -- mandate volume is expected to
/// be far lower than transaction volume (registrations/cancellations, not
/// every transaction), so one sequential consumer is sufficient; no worker
/// pool needed unlike the Transaction Queue's 4 parallel workers.
fn spawn_mandate_workers(
    mut rx: mpsc::Receiver<MandateJob>,
    pool: Pool,
    transaction_tx: mpsc::Sender<TransactionJob>,
) {
    tauri::async_runtime::spawn(async move {
        while let Some(job) = rx.recv().await {
            process_mandate_job(job, &pool, &transaction_tx).await;
        }
    });
}
```

- [ ] **Step 4: Run to verify it compiles**

Run: `cd src-tauri && cargo check`
Expected: clean compile. Fix any borrow-checker issues around `extraction` being moved into the `conn.interact` closure while also used afterward for `tx_job` (clone `extraction` before the closure, as already written above via `let extraction = job.extraction.clone();` at the top and `job.extraction.clone()` again for the three `tx_job.obs` fields — verify these don't conflict; if they do, clone once more explicitly rather than fighting the borrow checker with `unsafe` or excessive `Arc` wrapping).

- [ ] **Step 5: Commit**

```bash
cd src-tauri && git add src/ingestion/queues.rs
git commit -m "feat(mandate): add Mandate Queue, spawn_mandate_workers, process_mandate_job"
```

---

### Task 8: Wire `ProcessResult::MandateEvent` into `polling.rs` and `historical_scan.rs`

**Files:**
- Modify: `src-tauri/src/ingestion/polling.rs`
- Modify: `src-tauri/src/ingestion/historical_scan.rs`

**Interfaces:**
- Consumes: `ProcessResult::MandateEvent` (Task 6), `QueueHandles.mandate_tx` (Task 7).

- [ ] **Step 1: Add the match arm in `polling.rs`**

After the existing `Ok(Some(ProcessResult::StatementEmail(extracted))) => { ... }` arm (ends around line 430s, before the closing of the outer `match`), add:

```rust
                            Ok(Some(crate::ingestion::message_processor::ProcessResult::MandateEvent(extracted, mandate_extraction, event_type))) => {
                                let job = crate::ingestion::queues::MandateJob {
                                    extraction: mandate_extraction,
                                    event_type,
                                    source_pipeline: "gmail_transaction".to_string(),
                                    source_record_id: msg_id.clone(),
                                    connected_account_id: account.id.clone(),
                                    raw_body: extracted.text_body.clone(),
                                };
                                let tx = app
                                    .state::<crate::ingestion::queues::QueueHandles>()
                                    .mandate_tx
                                    .clone();
                                if tx.send(job).await.is_err() {
                                    tracing::error!("Mandate Queue closed — dropping job for msg_id='{}'", msg_id);
                                }
                            }
```

- [ ] **Step 2: Add the equivalent match arm in `historical_scan.rs`**

First, add a `mandate_events_found: usize` field alongside the existing `transactions_found`/`statements_found` fields in all three places that already carry that pair together (same shape, same default, same serde attributes as their `statements_found` neighbor in each):
- `ScanStatusResponse` (`historical_scan.rs:49-57`) — plus both its constructors at `historical_scan.rs:61-68` (`None` branch, set to `0`) and `historical_scan.rs:72-79` (`Some(cp)` branch, set to `state.mandate_events_found`).
- `ScanProgressPayload` (`historical_scan.rs:145-155`) — plus wherever it's constructed for the progress-event emission (`grep -n "ScanProgressPayload {" src-tauri/src/ingestion/historical_scan.rs` to find the exact construction site before editing).
- `ScanCheckpointState` (`historical_scan.rs:157-171`, with `#[serde(default)]` matching its neighbors so old checkpoint JSON without this field still deserializes).

Then, after the existing `Ok(Some(ProcessResult::StatementEmail(extracted))) => { ... }` arm (starts at line 550, ends before the next `match` arm), add — this file uses `account_id` (not `account.id` like `polling.rs`) and increments `state.transactions_found`/`state.statements_found` per arm, so increment the new `state.mandate_events_found` the same way:

```rust
                    Ok(Some(ProcessResult::MandateEvent(extracted, mandate_extraction, event_type))) => {
                        state.mandate_events_found += 1;
                        let job = crate::ingestion::queues::MandateJob {
                            extraction: mandate_extraction,
                            event_type,
                            source_pipeline: "gmail_transaction".to_string(),
                            source_record_id: msg_id.clone(),
                            connected_account_id: account_id.clone(),
                            raw_body: extracted.text_body.clone(),
                        };
                        let tx = app
                            .state::<crate::ingestion::queues::QueueHandles>()
                            .mandate_tx
                            .clone();
                        if tx.send(job).await.is_err() {
                            tracing::error!(
                                "Mandate Queue closed — dropping job for msg_id='{}'",
                                msg_id
                            );
                        }
                    }
```

- [ ] **Step 3: Run to verify compile**

Run: `cd src-tauri && cargo check`
Expected: clean compile — an unhandled `ProcessResult::MandateEvent` variant in either file's `match` would be a compile error (non-exhaustive match) if the match isn't already using a catch-all `_` arm; confirm neither file has one before this task (if either does, the "compile error" signal this task relies on to catch a missed wire-up won't fire — check manually instead).

- [ ] **Step 4: Commit**

```bash
cd src-tauri && git add src/ingestion/polling.rs src/ingestion/historical_scan.rs
git commit -m "feat(mandate): wire ProcessResult::MandateEvent into polling and historical scan"
```

---

### Task 9: Full-corpus regression — re-verify all 69 false-negative records

**Files:**
- None modified — verification only.

- [ ] **Step 1: Run the full Rust test suite**

Run: `cd src-tauri && cargo test --lib`
Expected: same pre-existing 4 failures as every prior task in this session (`test_seed_and_fetch`, `test_benchmark_corpus_processes`, `test_beta_onboarding_guide_limitations`, `test_documentation_completeness` — confirmed unrelated via `git stash` isolation earlier this session), plus every new test from Tasks 1-8 passing. Total pass count should be the prior session's 602 plus this feature's ~15 new tests.

- [ ] **Step 2: Re-run the Python full-corpus simulation, extended for mandate detection**

Extend `/private/tmp/.../scratchpad/verify_all_69.py`'s `classify()` function with the two new mandate-language checks (mirroring Task 4's Rust logic exactly), and its `process()` function to treat a `MandateRegistration`/`MandateCancellation` classification as resolving to `'transaction'` (matching `manual_review.type` for tx idx 55, 61 — idx 62's cancellation has no amount in `manual_review` at all, so it should be scored as correctly handled if it reaches the mandate path, not by checking `is_valid()`, which doesn't apply to mandate events the same way).

Run: `python3 /private/tmp/.../scratchpad/verify_all_69.py`
Expected: 66/69 (up from 64/69 before this feature) — idx 55, 61 now resolve via the mandate path instead of Cluster D's superseded fix; idx 62 resolves via cancellation matching (single candidate, since idx 61 registered it first in event order — verify the script processes idx 61 before idx 62, matching real chronological order). Remaining 3 (idx 3, 9 Cluster C, idx 25 Cluster H) stay deliberately unresolved per Aditya's decisions.

- [ ] **Step 3: Update `audit.md` and `fix-log.md`**

Append a new dated entry to both files (matching this session's established format — `## <cluster-or-feature-name>` header, `**Found:**`/`**What I did:**`/`**Verified:**` structure) documenting: the mandate-tracking feature's implementation, the migration of Cluster D/idx 61/62 to the new model, and the final 66/69 (or better) regression result.

- [ ] **Step 4: Commit**

```bash
git add audit.md fix-log.md
git commit -m "docs: record mandate-tracking feature completion and final regression results"
```

---

### Task 10: Doc edits (Doc 12, Doc 18, Doc 30, Doc 48)

**Files:**
- Modify: `dinero-docs/final-documents/12_Functional_Requirements_Specification_FRS.md` (§6.2, §6.2a, §6.3)
- Modify: `dinero-docs/final-documents/18_Database_Schema_Design.md` (new subsection near §4.14)
- Modify: `dinero-docs/final-documents/30_Task_by_Task_Implementation_Plan.md` (new TASK-TXN-0xx)
- Modify: `dinero-docs/final-documents/48_Architecture_Decision_Log.md` (new ADR)

- [ ] **Step 1: Read each target section's exact current text and changelog format**

Run: `grep -n "^### 6.2\|^### 6.3\|^## Changelog\|^| [0-9]" dinero-docs/final-documents/12_Functional_Requirements_Specification_FRS.md | head -20` (and the equivalent for Docs 18/30/48) to confirm exact heading text, version-numbering convention, and changelog-table format before writing insertions — every prior doc edit in this session followed this discipline (Ground Rule 6: preserve existing numbering/cross-reference conventions).

- [ ] **Step 2: Draft and apply each edit**

For Doc 12 §6.2: add a `mandate_registration` / `mandate_cancellation` row to the Gate 2 classes table (spec §7 table gives the exact wording to adapt). For §6.2a: add the Mandate Queue branch to the `route_message` pseudocode block, and append a clarifying sentence to the "never enqueued to both queues" note (spec §4.2's exact wording). For §6.3: note the Mandate Queue as a third consumer.

For Doc 18: add a new subsection immediately after the existing `recurring_payments` documentation (§4.14) documenting the two new columns and the `unresolved_mandate_cancellations` table, in the same column-table format §4.16-4.21 already use for `unprocessed_statements`.

For Doc 30: add a new `TASK-TXN-0xx` entry (next sequential number after the highest existing `TASK-TXN-0NN` — check via `grep -n "TASK-TXN-0" dinero-docs/final-documents/30_Task_by_Task_Implementation_Plan.md | tail -5` first), modeled on TASK-TXN-011's existing structure (Depends On / Effort / Files / description / Acceptance criteria naming the tests from Tasks 4-7 above).

For Doc 48: add a new ADR entry recording the third-queue decision and the explicit-vs-inferred `recurring_payments` split, modeled on ADR-019's existing entry format (the one that inserted Layer 5 into the extraction ladder — same "insert a new deterministic mechanism into an existing enumerated list" shape).

Each edit gets its own changelog-table row (version bump, date, author "Aditya Rawal" per this session's established convention, one-line description) in whichever document it touches, per that document's own changelog format.

- [ ] **Step 3: Commit**

```bash
git add dinero-docs/final-documents/12_Functional_Requirements_Specification_FRS.md \
        dinero-docs/final-documents/18_Database_Schema_Design.md \
        dinero-docs/final-documents/30_Task_by_Task_Implementation_Plan.md \
        dinero-docs/final-documents/48_Architecture_Decision_Log.md
git commit -m "docs: document mandate-tracking feature across FRS/schema/task-plan/ADR"
```
