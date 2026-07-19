# Statement Password Review Table + Timer Removal — Design Spec

**Status:** Approved by Aditya (design phase), pending implementation plan.
**Date:** 2026-07-20
**Author:** Claude
**Origin:** User request — after a user manually enters a statement's PDF password, show the extracted transaction rows in an editable popup table before anything is committed, let the user submit whenever they're ready, and remove the 2.5-minute password-entry countdown timer entirely.

---

## 1. Problem

Today, `statements_submit_password` runs the full parse pipeline (`run_parse_pipeline`) synchronously to completion the moment a password is accepted: parse → metadata → Instrument Gate → duplicate check → write `statements` row → extract rows → map to `statement_entries` → reconcile into canonical `transactions` → classify upcoming bill. The user never sees the extracted rows and has no chance to correct a mis-parsed date, merchant name, or amount before it becomes a real transaction.

Separately, `PasswordPromptModal` shows a 2.5-minute countdown (`PASSWORD_TIMEOUT_SECONDS`) that auto-closes the modal client-side. Its backend counterpart, `statements::password::handle_password_timeout`, has zero production callers today (confirmed via crate-wide grep — this was already flagged as dead in that function's own doc comment). The timer is pure UX friction with no real enforcement behind it and should go.

## 2. Goals

- After a correct manual password entry, extract statement rows but do **not** commit them — show them in an editable table popup instead.
- User can edit date / merchant / amount / debit-credit direction per row, and delete rows outright, before submitting.
- "Submit" is not time-boxed. Closing the popup without submitting must not lose the extraction — it reappears in a "Pending Review" list on the Statements page, resumable any time, including after an app restart.
- Remove the password-entry countdown timer completely: frontend UI/state, and the backend's dead timeout-handling code path.
- Scope is limited to the interactive manual-password-entry flow (`statements_submit_password`). Background auto-unlock via a stored password, and email-detected statements processed via the Statement Queue worker, are untouched — they keep parsing straight through exactly as today.

## 3. Non-goals

- No review step for Instrument-Gate-resume (`statements_confirm_instrument`) or auto-unlock-via-stored-password paths. Those keep calling the existing `run_parse_pipeline` unchanged.
- No editing of `currency` (always matches the statement) or `reference_id`/`row_index` (internal bookkeeping, not user-facing).
- No row-adding UI (only edit existing rows / delete rows). Adding wholly new transactions the parser missed is out of scope.
- Does not change the 3-attempt wrong-password cap (`MAX_PASSWORD_ATTEMPTS`) — unrelated to the timeout, stays as-is.

## 4. Architecture

### 4.1 New table: `pending_statement_reviews`

Not layered onto `unprocessed_statements` — that table's documented meaning is "couldn't be processed" (`awaiting_password`, `awaiting_instrument_confirmation`, `pending_retry`, `failed`), and several call sites (`select_actionable`) already filter its status values by hand. A row here means the opposite: parsing succeeded, and it's waiting on user confirmation, not failure recovery.

```sql
CREATE TABLE IF NOT EXISTS pending_statement_reviews (
    id TEXT PRIMARY KEY,
    source_statement_id TEXT,           -- the unprocessed_statements.id this resolved from (audit link)
    instrument_id TEXT NOT NULL REFERENCES instruments(id),
    instrument_type TEXT NOT NULL,      -- "credit_card" | "bank_account"
    issuer_name TEXT NOT NULL,
    masked_identifier TEXT NOT NULL,
    network TEXT,
    file_hash TEXT NOT NULL,
    filename TEXT NOT NULL,
    meta_json JSONB NOT NULL,           -- serialized StatementMetadata
    rows_json JSONB NOT NULL,           -- serialized Vec<ReviewRow>, current edited state
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

`ReviewRow` (Rust struct, serialized into `rows_json`):

```rust
pub struct ReviewRow {
    pub row_id: String,        // uuid, stable React key + submit/delete tracking
    pub transaction_date: String,
    pub merchant_raw: String,
    pub amount_minor: i64,
    pub currency: String,
    pub direction: String,     // "debit" | "credit"
    pub reference_id: Option<String>,
    pub row_index: usize,
    pub llm_extracted: bool,
}
```

`StatementMetadata` (`src-tauri/src/statements/metadata_extractor.rs`) gains `Serialize`/`Deserialize` derives to round-trip through `meta_json`.

### 4.2 Pipeline split

`run_parse_pipeline` (`src-tauri/src/commands/mod.rs`) splits into two functions. Both are extracted from the existing function body — no logic changes to steps that move, only where they stop/resume.

**`extract_statement_for_review(...)`** — today's Steps 6–9 + Step 11, unchanged:
1. Parse PDF in-memory (Step 6)
2. Extract metadata (Step 7)
3. Statement Instrument Gate (Step 8) — same blocking behavior on missing issuer/masked (`PipelineOutcome::BlockedAwaitingInstrument`, unchanged)
4. Post-metadata duplicate check (Step 9) — same rejection behavior (`duplicate_billing_cycle` error, unchanged)
5. Extract statement rows (Step 11, moved earlier — no longer needs the `statements` row from old Step 10 first)

On success: insert the `pending_statement_reviews` row, save the password via `password::save_password` (instrument_id is already resolved at this point — no need to wait for a `statements` row to exist), emit `statement_review_ready { review_id, filename, row_count }`, delete the now-resolved `unprocessed_statements` row.

**`commit_reviewed_statement(review_id, edited_rows, pool, app)`** — today's Steps 10 + 12–14:
1. Write `statements` row (Step 10, using the stored `meta_json`/instrument context)
2. Map `edited_rows` → `statement_entries` (Step 12, using user's edited values instead of the original `rows`)
3. Build observations, reconcile each transactionally, spawn alert evaluation (Step 13, unchanged logic)
4. Classify upcoming bill (Step 14, unchanged)
5. Delete the `pending_statement_reviews` row

`run_parse_pipeline` itself stays as-is (calls both halves back to back) for every caller except `statements_submit_password` — the Statement Queue worker (`ingestion/queues.rs`) and `statements_confirm_instrument` keep using it unmodified.

### 4.3 `statements_submit_password` changes

On `PasswordResolutionResult::UnlockedWithUserInput`, replace the `run_parse_pipeline` call with `extract_statement_for_review`. Response shape gains a new outcome:

```json
{ "status": "awaiting_review", "review_id": "..." }
```

alongside the existing `unlocked` (now unused by this path), `awaiting_instrument_confirmation`, `wrong_password`, and `max_attempts_exceeded` outcomes — the Instrument Gate block and duplicate-reject paths behave exactly as before (they happen inside `extract_statement_for_review` before any review row is ever created).

### 4.4 New Tauri commands

- `statements_list_awaiting_review() -> Vec<PendingReviewSummary>` — `{ review_id, filename, issuer_name, masked_identifier, row_count, created_at }`, for the Statements page list.
- `statements_get_review(review_id: String) -> PendingReviewDetail` — full context + rows, used both right after password unlock and when reopening from the list.
- `statements_submit_review(review_id: String, rows: Vec<ReviewRow>) -> { statement_id }` — validates `rows` non-empty and each row's date/amount well-formed, calls `commit_reviewed_statement`, emits `statement_parsed`.
- `statements_discard_review(review_id: String) -> ()` — deletes the pending row. No transactions were ever created, so there's nothing else to unwind.

## 5. Frontend

- `PasswordPromptModal.tsx`: on `result.status === 'awaiting_review'`, close the password modal and open the new review modal (via `openReviewModal(reviewId)` in `GlobalStateContext`) instead of calling `onUnlocked()`. Toast becomes "Password Accepted — review extracted transactions."
- New `StatementReviewModal.tsx`: fetches full rows via `statements_get_review` on open. Table columns: Date (date input), Merchant/Description (text input), Amount (number input) + Debit/Credit toggle, per-row delete button. Header shows filename/issuer/masked/row count. Footer: "Discard" (confirmation dialog, destructive, calls `statements_discard_review`) and "Submit N Transactions" (calls `statements_submit_review` with the current edited/filtered row array, then closes and refreshes statement history).
- `GlobalStateContext.tsx`: add `reviewModalOpen`/`pendingReviewId` state + `openReviewModal`/`closeReviewModal`; listen for `statement_review_ready` so the modal can open even if triggered outside the currently-focused component tree.
- Statements page (`src/pages/Statements.tsx`): new "Pending Review" section, sibling to the existing unprocessed-items queue, backed by a `usePendingReviewList` query hook wrapping `statements_list_awaiting_review`. Clicking an entry opens `StatementReviewModal` for that `review_id` — this is the resume-later path.

## 6. Timer removal (end to end)

**Frontend:**
- `PasswordPromptModal.tsx`: remove `PASSWORD_TIMEOUT_TOTAL_SECONDS`, `formatCountdown`, the countdown/`countdownPct` derived values, the entire timer UI block (`Clock` icon, `role="timer"` div, progress bar), and the now-unused `Clock` import.
- `GlobalStateContext.tsx`: remove `PASSWORD_TIMEOUT_SECONDS`, `passwordTimeoutCountdown` state, `countdownRef`, `clearCountdown`, `startCountdown` (and its call inside `openPasswordModal`), the `statement_password_timeout` event listener (`unlistenTimeout`) and its cleanup, and `passwordTimeoutCountdown` from the context type/value.

**Backend:**
- `src-tauri/src/statements/password.rs`: delete `handle_password_timeout` and its test `test_password_timeout_creates_pending_retry`.
- Remove `NotificationKind::StatementPasswordTimeout` if grep during implementation confirms nothing else references it.
- Leave the `pending_retry` status value alone if other code paths (e.g. manual statement retry) also produce/consume it — only the timeout-specific function is dead code here, not the status itself. Verify with a grep pass during implementation before touching any shared status string.

## 7. Error handling

- `statements_get_review`/`statements_submit_review`/`statements_discard_review` on an unknown `review_id` (row already submitted/discarded elsewhere, or deleted): return a clear `AppError::NotFound`-style error; frontend closes the modal and toasts "This review is no longer available" rather than showing a broken table.
- `statements_submit_review` row validation failures (bad date format, non-numeric amount after edit) are rejected with a field-level error surfaced inline in the table row, not a generic toast — the user needs to know which row to fix.
- Duplicate-billing-cycle and Instrument-Gate-block outcomes are unchanged and happen before a review row ever exists, so they don't interact with this feature at all.

## 8. Testing

- Rust unit tests: `extract_statement_for_review` writes a `pending_statement_reviews` row and nothing to `statements`/`statement_entries`/`transactions`; `commit_reviewed_statement` reflects an edited amount/date and excludes a deleted row from the resulting entries/observations; `statements_discard_review` leaves no trace in `statements`/`transactions`.
- Existing `password.rs` test suite loses `test_password_timeout_creates_pending_retry`; everything else unchanged.
- Manual verification via the `run` skill: unlock a fixture PDF with a password, edit a row, delete a row, submit, confirm the resulting transaction reflects the edits; close the popup without submitting, confirm the statement reappears under "Pending Review," reopen it, submit; confirm no countdown UI appears anywhere in the password flow.

## 9. Follow-up: final-documents

After implementation, update (targeted edits, not full rewrites — these are large files):
- `12_Functional_Requirements_Specification_FRS.md` — password flow + new review step
- `11_Product_Requirements_Document_PRD.md` — feature description
- `13_User_Flow_UX_Design.md` — new modal + Pending Review section in the flow
- `18_Database_Schema_Design.md` — `pending_statement_reviews` table
- `30_Task_by_Task_Implementation_Plan.md` — mark timer-related tasks removed/superseded, add this feature's tasks
- `48_Architecture_Decision_Log.md` — record the pipeline-split decision and why (staged commit vs. immediate auto-save)
