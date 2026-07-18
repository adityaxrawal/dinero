# Explicit Mandate Tracking — Design Spec

**Status:** Approved by Aditya (design phase), pending implementation plan.
**Date:** 2026-07-18
**Author:** Claude (Gmail false-negative remediation session)
**Origin:** Surfaced mid-session while fixing the gmail ingestion false-negative pipeline (see `audit.md`, `fix-log.md`, `gmail_export/discovery_report_false_negatives.md`). Two SBI Card emails (tx idx 61: e-Mandate registration, tx idx 62: e-Mandate cancellation) turned out to need infrastructure — a persistent, cancellable mandate/subscription record — that does not exist anywhere in the current schema. This spec designs that infrastructure.

---

## 1. Problem

Banks send explicit "you registered/cancelled a recurring mandate" emails (SBI Card's "e-Mandate", Axis Bank's "AutoPay", and by convention every other Indian bank's UPI-Autopay/NACH-mandate equivalent). Today the pipeline has no concept of a mandate as a first-class, explicit, cancellable thing:

- `recurring_payments` exists (`src-tauri/migrations/20260101000008_metadata_and_logging_tables.sql`) but is populated **only** by statistical inference (`extraction/recurring_detector.rs`, Doc 30 TASK-TXN-011) after 3+ matching transactions — it has no path for "the bank just told us directly, before any transaction history exists."
- There is no mandate identifier field (SBI Card calls theirs a "SiHub ID"), no cancellation mechanism, no way to distinguish an inferred row from an explicitly-registered one.
- Three real emails in the false-negative corpus need this: Axis Bank AutoPay activation (tx idx 55, already fixed as a bare ₹0 transaction in Cluster D — this spec supersedes that), SBI Card e-Mandate registration (tx idx 61), SBI Card e-Mandate cancellation (tx idx 62, currently unresolvable — no amount exists at all, and no record to cancel).

## 2. Goals

- Capture explicit mandate registration/cancellation signals from any bank (bank-agnostic detection, not hardcoded to SBI Card/Axis Bank specifically — no other bank's real example exists in the corpus yet, so this is necessarily built against a small evidence set and should be treated as a first version, hardened as real examples surface).
- Registration creates/updates an `active` `recurring_payments` row *and* a ₹0.00 transaction row, both via the pipeline's single existing canonical-write function.
- Cancellation marks the matching `recurring_payments` row `cancelled`, or — if it can't be matched with confidence — blocks and surfaces for manual resolution. Never guesses which mandate to cancel.
- No second reconciliation/canonical-write function. No second sender-verification or content-classification pipeline. Everything routes through the existing single pipeline (Doc 15 invariant), extended, not duplicated.

## 3. Non-goals

- No new UI. `recurring_payments` has shipped with no frontend page for over a full task cycle already (confirmed: no `src/pages/*Recurring*` or `*Subscription*` file exists) — this spec keeps that precedent. The new `unresolved_mandate_cancellations` table is queryable but has no resolution screen yet; that's a follow-up.
- No attempt to detect mandates from bank templates not evidenced anywhere in the false-negative corpus or this doc. Keyword sets are written generically where the underlying concept (mandate/autopay/standing-instruction language) is genuinely bank-agnostic, but are not validated against real examples beyond SBI Card and Axis Bank.
- Does not touch `resolve_instrument()`/`ingest_observation()` naming (Finding F3, Phase 1 Discovery Report) — that gap is pre-existing and out of scope here; this spec uses the real function name (`create_canonical_transaction`) throughout.

## 4. Architecture

### 4.1 Gate 2 routing

`ContentClassifier::classify()` (`src-tauri/src/ingestion/content_classifier.rs`) gains two new variants:

```rust
pub enum ContentClass {
    TransactionAlert,
    BalanceUpdate,
    StatementEmail,
    MandateRegistration,   // new
    MandateCancellation,   // new
    Noise,
    Otp,
    Kyc,
    Marketing,
    Reminder,
    Unknown,
}
```

Checked **before** the existing transaction-verb checks (mandate language can co-occur with debit/credit words — e.g. SBI Card's registration email says "authorised debit of INR 0.00" — and must not fall through to `TransactionAlert`):

```text
function classify(subject, body):
    ...existing OTP/KYC/Statement checks unchanged...

    if contains_mandate_cancellation_language(content):
        return MandateCancellation
    if contains_mandate_registration_language(content):
        return MandateRegistration

    ...existing Marketing/Reminder/TransactionAlert/BalanceUpdate checks, unchanged...
```

`contains_mandate_registration_language`: "mandate registered", "mandate set at merchant", "e-mandate created", "registration success... mandate", "autopay activated", "autopay for .* activated" (subject pattern), "successful autopay transaction" (moved here from `has_transaction_verb`, superseding the Cluster D fix).

`contains_mandate_cancellation_language`: "mandate cancelled", "mandate cancellation", "e-mandate cancellation", "mandate stands cancelled", "autopay deactivated", "autopay cancelled".

### 4.2 Mandate Queue

A third bounded `mpsc` channel, parallel to `TRANSACTION_QUEUE` and `STATEMENT_QUEUE` (Doc 12 §6.2a), constructed the same way. Routing (extending §6.2a's `route_message`):

```text
function route_message(classification_result):
    match classification_result:
        verified_transaction_candidate + gate3_pass →
            push to TRANSACTION_QUEUE
        verified_statement_candidate →
            push to STATEMENT_QUEUE
        mandate_registration | mandate_cancellation →
            push to MANDATE_QUEUE
        _ →
            hard_reject (no queue entry, logged to audit_log as gate_rejected)
```

A message is still enqueued to **exactly one** queue — the "never both" invariant (§6.2a) is preserved at the routing level. §4.4 below is how a Mandate Queue message still produces a transaction row without being double-routed.

### 4.3 Mandate field extraction

New module `src-tauri/src/extraction/mandate_extractor.rs` — not a 7th extraction-ladder layer (the fields don't match `ExtractionResult`'s shape: no `direction`/settlement `amount_minor` semantics apply here). Extracts:

| Field | Required | Notes |
|---|---|---|
| `merchant` | yes | Same merchant-labeling heuristics as `GenericRegexLayer` (reuse the keyword alternation, don't reimplement) |
| `cadence` | no | Raw text ("monthly", "weekly") — matches `recurring_payments.cadence`'s existing free-text convention |
| `max_limit_amount` | no | Optional ceiling amount, if the bank prints one |
| `external_mandate_id` | no | Bank-specific mandate reference (SBI Card: "SiHub ID"). Bank-agnostic pattern: an alphanumeric token near "mandate ID"/"reference"/bank-specific label variants |
| `event_type` | yes, implicit | `registration` or `cancellation`, already known from which `ContentClass` routed here — not re-derived |

Mandatory-field gate for this queue: `merchant` present, or reject to `non_transaction` (mirrors Gate 3's precision-over-recall stance, Doc 12 §6.2).

### 4.4 Mandate Queue consumer — writes both artifacts

```text
function process_mandate_event(extracted, event_type, instrument_id):
    match event_type:
        registration:
            upsert recurring_payments
                key: (merchant_entity_id, instrument_id, source='explicit')
                set: status='active', source='explicit',
                     external_mandate_id, cadence, amount_minor=max_limit_amount

        cancellation:
            candidates = SELECT * FROM recurring_payments
                         WHERE status='active'
                         AND (external_mandate_id = extracted.external_mandate_id
                              OR (merchant_entity_id = extracted.merchant_entity_id
                                  AND instrument_id = extracted.instrument_id))
            if candidates.len() == 1:
                UPDATE recurring_payments SET status='cancelled' WHERE id = candidates[0].id
            else:
                # zero or multiple -- never guess (same discipline as
                # Statement Instrument Gate, Doc 12 §7.2a)
                INSERT INTO unresolved_mandate_cancellations
                    (raw_signal, candidate_ids, created_at)
                return BLOCKED

    # Both registration and (successfully matched) cancellation also write
    # the ₹0 transaction -- NOT by calling create_canonical_transaction
    # directly (that would skip fingerprint computation, the
    # insert_observation_idempotent dedup check, and match-precedence
    # scoring inside reconcile_transactionally -- a second, incomplete
    # write path). Instead, send a synthesized TransactionJob onto the
    # *existing* Transaction Queue channel and let the unmodified
    # process_transaction_job (queues.rs:358) do exactly what it already
    # does for every other transaction.
    send_to_transaction_queue(TransactionJob {
        obs: ExtractionResult {
            amount_minor: Some(0),
            direction: Some("debit"),
            merchant_raw: Some(extracted.merchant),
            currency: Some("INR"),
            event_time: extracted.event_time,
            extraction_method: "mandate_event",
            ...
        },
        source_pipeline: "gmail_transaction",
        source_record_id,
        connected_account_id,
        raw_body,
    })
```

Corrected during implementation planning (verified against the real call chain, not assumed from this spec's first draft): the actual single entry point Doc 12 §8.2a's pseudocode calls `ingest_observation()` is `reconcile_transactionally()` (`src-tauri/src/reconciliation/engine.rs:133`), reached only through `process_transaction_job` (`queues.rs:358`) — `create_canonical_transaction` (`canonical.rs:31`) is one internal decision branch *inside* that call, not the entry point itself. Routing the mandate-generated transaction through the *existing* Transaction Queue, rather than calling any reconciliation-internal function directly, is both simpler and correctly reuses the one real pipeline (Finding F3, Phase 1 Discovery Report — the spec's named `ingest_observation()`/`resolve_instrument()` don't exist as such; the actual pipeline is this queue-and-worker chain).

## 5. Data model changes

**`recurring_payments`** (`src-tauri/migrations/20260101000008_metadata_and_logging_tables.sql`), additive migration:

```sql
ALTER TABLE recurring_payments ADD COLUMN source TEXT NOT NULL DEFAULT 'inferred';
ALTER TABLE recurring_payments ADD COLUMN external_mandate_id TEXT;
```

Existing rows backfill `source='inferred'` (the default), correctly reflecting that every row written before this change came from `recurring_detector.rs`'s statistical path. `status` is already unconstrained `TEXT` (no `CHECK`), so `'cancelled'` needs no schema change beyond the two columns above.

**New table**, `unresolved_mandate_cancellations`, mirroring `unprocessed_statements`'s shape (Doc 18 §4.16-4.21) for a blocking-on-user-input state:

```sql
CREATE TABLE IF NOT EXISTS unresolved_mandate_cancellations (
    id TEXT PRIMARY KEY,
    raw_signal TEXT NOT NULL,           -- JSON: extracted fields from the cancellation email
    candidate_ids TEXT,                 -- JSON array of recurring_payments.id, empty if zero matches
    status TEXT NOT NULL DEFAULT 'unresolved',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    resolved_at DATETIME
);
```

No resolution UI in this pass (§3, non-goals) — queryable via a future admin/debug command, same maturity level `recurring_payments` itself currently has.

## 6. Migrating Cluster D / idx 61 / idx 62

- `content_classifier.rs`'s `has_transaction_verb()` loses the `"successful autopay transaction"` branch added for Cluster D — routing now happens earlier, via `contains_mandate_registration_language`, before `has_transaction_verb` is ever consulted for these emails.
- `GenericRegexLayer`'s `"merchant name"` keyword addition (also added for Cluster D, `ladder.rs`) is **not** removed — mandate extraction reuses the same merchant-labeling heuristic (§4.3), so this keyword still earns its keep for the new path.
- The existing Cluster D/idx 61 tests (`test_autopay_activation_classified_as_transaction`, `test_generic_merchant_heuristic_two_word_label_next_line`) get superseded by new Mandate Queue tests in the implementation plan — not deleted silently, replaced with a documented reason in the same file.

## 7. Doc edits required (surgical, additive, propagated)

| Document | Section | Edit |
|---|---|---|
| Doc 12 (FRS) | §6.2 | Add `mandate_registration`/`mandate_cancellation` to the Gate 2 class table |
| Doc 12 (FRS) | §6.2a | Add Mandate Queue branch to `route_message` pseudocode; amend "never both queues" note to clarify it governs routing, not downstream writes |
| Doc 12 (FRS) | §6.3 | Note the Mandate Queue as a third consumer alongside Transaction/Statement Queues |
| Doc 18 (DB Schema) | new subsection near §4.14 (`recurring_payments`) | Document `source`/`external_mandate_id` columns and the new `unresolved_mandate_cancellations` table, following existing §4.16-4.21 conventions for blocking-state tables |
| Doc 30 (Task Plan) | new TASK-TXN-0xx | Mandate detection/extraction/reconciliation task, modeled on TASK-TXN-011's existing entry |
| Doc 48 (ADR log) | new ADR | Records this as an architectural decision (third queue, explicit-vs-inferred `recurring_payments` split) — same convention as ADR-019's Layer 5 insertion |

Exact insertion text drafted in the implementation plan, not here (per this session's own Ground Rule 6 discipline: doc edits are drafted at fix-design time, not speculatively).

## 8. Testing plan (implementation-plan detail, outlined here)

- Gate 2: `test_mandate_registration_classified_correctly`, `test_mandate_cancellation_classified_correctly`, `test_mandate_language_not_swallowed_by_transaction_verb_check` (the "authorised debit of INR 0.00" co-occurrence case).
- Extraction: `test_mandate_extractor_sbi_card_registration` (tx idx 61 real body), `test_mandate_extractor_sbi_card_cancellation` (tx idx 62 real body), `test_mandate_extractor_axis_autopay_registration` (tx idx 55 real body, migrated from Cluster D).
- Reconciliation: `test_mandate_registration_upserts_active_row_and_transaction`, `test_mandate_cancellation_single_match_marks_cancelled`, `test_mandate_cancellation_zero_matches_blocks`, `test_mandate_cancellation_multiple_matches_blocks`.
- Full-corpus regression: re-run all 69 false-negative records (same Python-simulation method used in the gmail remediation session, or a real `email_segregate.rs` run if DB/key access becomes available) — target 66/69 resolved (idx 3, 9 Cluster C and idx 25 Cluster H remain deliberately unresolved).

## 9. Open risks / follow-ups

- Bank-agnostic keyword sets are built from exactly 2 banks' real templates (SBI Card, Axis Bank). Doc 34 corpus-hardening should flag "mandate registration/cancellation" as a category needing more real bank examples before the ≥95% accuracy claim could extend to it.
- `unresolved_mandate_cancellations` has no resolution UI — if mandate volume turns out to be non-trivial, this becomes a real backlog with no way for the user to clear it except direct DB access. Worth revisiting once real usage data exists.
- The `(merchant_entity_id, instrument_id)` upsert key for registration assumes a merchant only has one active mandate per instrument at a time. If a user has two different subscriptions with the same merchant on the same card, they'd collide. Not evidenced in the corpus; flagged, not solved here.
