# Audit Log — Gmail False-Negative Remediation

Append-only permanent record. Never rewrite history — corrections get a new entry, not an edit to an old one.

---

## Phase 1 — Discovery & Clustering (read-only)

**Date:** 2026-07-18
**Scope:** 69 manually-labeled false-negative sidecar records (`gmail_export/segregated_emails/false_negative/{transaction,statement}/*.json`) — 66 transaction + 3 statement (prompt stated 65 + 3 = 68; actual count is 69, one more than stated — flagged, not resolved).
**Action taken:** Read-only. No code or spec-doc file modified. Discovery Report written to `gmail_export/discovery_report_false_negatives.md`.

**Clusters identified (8, covering all 69 records):**
- A (42 records, tx+stmt) — verified-sender registry missing `sbicard.com`/`idfcfirst.bank.in`/`slice.bank.in`. Bucket: business-owner-decision-required.
- B (16 records) — Gate 2 `has_transaction_verb()` missing "paid"/"you paid". Bucket: resolvable-convention.
- C (2 records) — EMI-booking confirmation, amount only in password-protected PDF attachment. Bucket: business-owner-decision-required.
- D (1 record) — ₹0.00 AutoPay mandate activation. Bucket: business-owner-decision-required.
- E (4 records) — `GenericRegexLayer` merchant regex excludes `*` (card-network descriptors). Bucket: resolvable-convention.
- F (1 record) — `GenericRegexLayer` date regex missing space-separated "DD Mon YYYY". Bucket: resolvable-convention.
- G (2 records) — `GenericRegexLayer` merchant regex fails on "keyword:" (colon before value). Bucket: resolvable-convention.
- H (1 record) — declined/failed USD transaction, no distinct "declined" status concept confirmed to exist. Bucket: business-owner-decision-required.

**Architecture findings deferred per §9 (not resolved, not blocking any cluster above):**
- F1: `email_segregate.rs` hand-mirrors `pub(crate)` gate-3 logic from `MessageProcessor`; mirror is currently correct but incomplete (missing the live pipeline's balance-update default-fill step).
- F2: batch/export harness cannot exercise Layer 6 (LLM) at all — no CLI flag exists.
- F3: `resolve_instrument()` / `ingest_observation()` / `statement_instrument_gate()` (Doc 12 §7.2a/§8.2a/§10.4a) do not exist as named functions anywhere in the codebase; equivalent behavior is inlined elsewhere.
- F4: Doc 30 cites a `src-tauri/src/gmail/` module tree that doesn't exist; actual code is under `src-tauri/src/ingestion/`.
- F5: Doc 26 §8.4 / Doc 30 describe the sender registry as "signed"; no signing/integrity-check code exists.

**Anchor-example hypothesis status:** the prompt's own worked example speculated Gate 1 treats ESP tracking-link domains in the email body as a sender-identity signal. Read `verify_sender()` in full — it never inspects the body at all, only the `From:` header. **Disproven.** Real root cause for that record (and 41 others) is Cluster A (missing registry entry), not a spoof-heuristic defect.

**Verification evidence for Phase 1 claims:** ad hoc Python simulations of the actual Rust regex/logic (`amount_regex`, `has_transaction_verb`, `GENERIC_DATE_RE`, `GENERIC_MERCHANT_RE`, `SenderValidator::verify_sender`'s exact-match/substring/Levenshtein/display-name branches) run directly against the 69 records' real `raw.subject`/`raw.body_html` text, cross-checked against `raw.from` domains and the live registry JSON's actual contents (202 domain entries / 138 unique bank names). Full command history is in this session's transcript, not reproduced here.

**Next step:** present Discovery Report to Aditya (Stop-and-Confirm Gate 1). No cluster proceeds to Phase 2 (fix design) until Aditya approves the clustering/classification, per Ground Rule 2 (one cluster at a time) and Ground Rule 3 (no silent resolution of business-owner-decision-required or genuinely-open findings).

**Gate 1 approved 2026-07-18.** Aditya authorized proceeding through all `resolvable-convention` clusters (B, E, F, G) without a per-cluster Stop Gate 2 pause; `business-owner-decision-required` clusters (A, C, D, H) remain blocked and will be presented, not auto-resolved.

---

## Cluster B — Gate 2 verb-list gap ("paid"/"you paid") — 16 records

**Root cause:** `has_transaction_verb()` (`src-tauri/src/ingestion/content_classifier.rs:32-39`) had no entry matching Jupiter/neobank UPI-app confirmation phrasing ("You paid ₹300.00. Paid to <merchant>"). Full detail in Discovery Report Cluster B.

**Fix:** Added `|| content.contains("you paid")` to `has_transaction_verb()`. Deliberately the two-word phrase, not bare `"paid"`, to avoid new false positives on "not paid"/"already paid"/"please pay" (Reminder-class emails legitimately use "pay"/"paid" too). This single function feeds all three of its call sites (`settled_transaction` gate at line 75, the Reminder-routing negation at line 89, and the final TransactionAlert fallback at line 119) — no duplicated edit needed.

**Doc edits:** None. Checked Doc 12 §6.2, Doc 30 TASK-GMAIL-005 (line 551-558) — neither enumerates Gate 2's literal verb list; Gate 2's keyword set is an implementation detail no doc claims ownership of, so there's nothing to correct or keep in sync. (Note: Doc 30 line 717, describing Layer 3/`GenericRegexLayer`, already lists "paid" as an expected direction-keyword for a *different* function — consistent with, not contradicted by, this fix.)

**Tests added:** `test_neobank_you_paid_phrasing_classified_as_transaction` (both `subj` variants seen in the false-negative set), `test_bare_paid_without_you_paid_not_treated_as_transaction_verb` (guards the false-positive risk) — `src-tauri/src/ingestion/content_classifier_tests.rs`.

**Verification:**
- `cargo test --lib content_classifier` — 10 passed (8 pre-existing + 2 new), 0 failed.
- `cargo test --lib` (full suite) — 594 passed, 4 failed. All 4 failures confirmed pre-existing and unrelated (`commands::data::tests::test_seed_and_fetch` — DB schema missing `bank_ifsc` column; `phase10_rigorous_tests::tests::{test_benchmark_corpus_processes,test_beta_onboarding_guide_limitations,test_documentation_completeness}` — missing files) — reproduced identically with `content_classifier.rs`/`content_classifier_tests.rs` stashed out, confirming zero relation to this change.
- Did not re-run the 16 false-negative sidecar files through `email_segregate.rs` end-to-end (that binary requires a live DB copy + Gmail-export directory wiring beyond this session's scope to invoke standalone) — correctness verified instead via the added unit tests exercising the exact real phrasing from the false-negative corpus (`"You paid ₹300.00. Paid to <merchant>"`, both subject variants), which is what `ContentClassifier::classify()` actually operates on regardless of caller.

**Status:** Complete.

---

## Cluster A — verified-sender registry gap — 42 records

**Decision (Aditya, 2026-07-18):** add all three domains; SPF/DKIM not separately verified.

**Fix:** added `sbicard.com` (transaction_candidate, "SBI Card"), `idfcfirst.bank.in` (statement_candidate, "IDFC FIRST Bank"), `slice.bank.in` (statement_candidate, "Slice (North East Small Finance Bank)") to `src-tauri/src/ingestion/verified_senders_registry.json` (202 → 205 entries). Purely additive — no spoof-heuristic code touched; each new domain now resolves at the exact-match branch (`verified_senders.rs:58-68`) before any heuristic runs.

**New finding, not in original Discovery Report — found only by re-running all 69 records after this fix, not just the 39+3 nominally in Cluster A:** two `sbicard.com` transaction records (tx idx 61, 62) that were invisible behind the Gate 1 rejection turn out to be **SBI Card e-Mandate registration/cancellation notices**, not ordinary transaction alerts:
- idx 61 ("Registration Success: e-Mandate set at merchant..."): manual ground truth has `expected_amount: "0"`, `expected_direction: ""` (blank, not "debit") — a subscription-mandate authorization, similar in shape to Cluster D's AutoPay activation but with no direction assigned at all.
- idx 62 ("e-mandate Cancellation..."): manual ground truth has `expected_amount: ""` (**not even "0" — genuinely no amount**), `expected_direction: ""`. A mandate cancellation has no amount concept; `is_valid()`'s amount requirement can't be satisfied by design, not by a fixable gap.

These don't resolve under any approved cluster's fix (Gate 2 correctly returns `Unknown` for both — no verb list addition was ever scoped for e-Mandate lifecycle language, and Cluster D's decision was specifically about AutoPay *activation* with a genuine ₹0.00 authorization line, not cancellation-with-no-amount-at-all). Not auto-resolved. Presented to Aditya as a new decision point.

**Verification approach:** built a faithful Python re-implementation of the exact new Rust logic (registry lookup, `has_transaction_verb`, `GenericRegexLayer`'s amount/direction/merchant-two-tier/date regexes, `is_valid()`) and ran it against the real `raw.subject`/`raw.body_html`/`raw.from` text of all 69 false-negative records (not just the ones nominally in this cluster), cross-checked against `manual_review.type`. Result: 64/69 resolve correctly (37 of 39 sbicard.com tx + all 3 statement records for this cluster; plus Clusters B/D/E/F/G's records). Remaining 5: idx 3, 9 (Cluster C, correctly still non-transaction per decision), idx 25 (Cluster H, correctly still non-transaction per decision), idx 61, 62 (new e-Mandate finding above, unresolved).

**Status:** 40/42 complete. 2 records (idx 61, 62) held open pending a new decision.

---

## Cluster D — AutoPay ₹0.00 activation — 1 record

**Decision (Aditya, 2026-07-18):** capture as a ₹0.00 debit.

**Fix:**
1. Gate 2: added `"successful autopay transaction"` to `has_transaction_verb()` (`content_classifier.rs`) — the full phrase, not bare "autopay", to avoid matching AutoPay-enrollment marketing copy.
2. Gate 3: added `"merchant name"` to `GENERIC_MERCHANT_RE_STRICT`'s keyword alternation, listed before bare `"merchant"` (`regex` crate prefers first-listed alternative at a given position, not longest) — Axis Bank's template labels the counterparty "Merchant Name:" (two words) with the value on the next line; bare "merchant" matched only the first word and then failed entirely (capture class excludes `:`, and `:` isn't a terminator either).

Direction resolves to "debit" via the pre-existing `GenericRegexLayer` default (no direction keyword matches "subsequent debit initiated" literally, since the debit-keyword list requires "debited" not "debit" — falls through to the existing `if amount_minor.is_some() { direction = debit }` default, unchanged code). Date ("24-04-2026") already matched the original numeric date-regex alternative, no change needed.

**Doc edits:** none — same reasoning as Cluster B (Doc 30 TASK-GMAIL-005/TASK-TXN-004 don't enumerate exhaustive keyword lists).

**Tests added:** `test_autopay_activation_classified_as_transaction` (`content_classifier_tests.rs`), `test_generic_merchant_heuristic_two_word_label_next_line` (`ladder.rs`), both built from the real body text.

**Verification:** included in the 64/69 full-corpus Python re-simulation above (tx idx 55 resolves to `transaction` — pass). `cargo test --lib` — 602 passed, same 4 pre-existing unrelated failures.

**Status:** Complete.

---

## Cluster C — EMI-booking, amount in password-protected attachment — 2 records

**Decision (Aditya, 2026-07-18):** out of scope. Accepted as a permanent, correct non-capture — no attachment-password-decryption exists in the pipeline, and building it is a real architectural addition, not a false-negative bug fix.

**Fix:** none. No code changed.

**Status:** Closed, not fixed, by explicit decision.

---

## Cluster H — declined USD transaction — 1 record

**Decision (Aditya, 2026-07-18):** don't capture. No status exists in the schema for "declined, not posted" (unverified this session); capturing it as an ordinary debit would misrepresent the ledger.

**Fix:** none. No code changed.

**Status:** Closed, not fixed, by explicit decision.

---

## Clusters E, F, G — `GenericRegexLayer` (Layer 3) hardening — 4 + 1 + 2 = 7 records

All three share one function (`GenericRegexLayer::extract`, `src-tauri/src/extraction/ladder.rs:509-620`), so implemented together as one diff. Root causes and fixes per Discovery Report, plus two corrections found only during test-driven verification (documented below, not silently absorbed).

**Cluster E (4 records, HDFC `RAZ*SWIGGY`) — fix:** broadened the merchant-capture character class from `[A-Za-z0-9\s]` to `[A-Za-z0-9\s*]` so card-network settlement descriptors (`RAZ*SWIGGY`) don't truncate at the `*`.

**Correction found during verification (not in original Discovery Report):** writing a test against the *real* HDFC body ("...debited **from** your HDFC Bank Credit Card ending 0364 **towards** RAZ*SWIGGY...") surfaced that the merchant regex's leftmost-match semantics were already picking "from" over "towards" — capturing "your HDFC Bank Credit" (the *source* instrument) instead of the actual merchant, regardless of the `*` fix. This was previously invisible because the same body also failed on a second, independent gap: the *date* regex found no match for "24 May, 2026" either (a third date-format variant, see Cluster F correction below), so `is_valid()`'s unconditional `event_time.is_some()` requirement (`ladder.rs:73`) discarded the whole extraction before the wrong-merchant value could ever surface. My original Discovery Report attributed Cluster E's failure entirely to the `*`-truncation; that was incomplete — both bugs were present simultaneously and independently blocking.

**Fix for the "from" ambiguity:** the `regex` crate has no lookaround, so a single alternation can't exclude "from" only when followed by "your". Restructured into two static regexes tried in sequence: `GENERIC_MERCHANT_RE_STRICT` (unambiguous merchant-labeling keywords: `towards`, `paid to`, `purchased at`, `txn at`, `info:`, `beneficiary`, `in favor of`, `merchant`) tried first against the whole body; `GENERIC_MERCHANT_RE` (the ambiguous `at`/`to`/`from`/`for`/`by`) tried only if the strict pass finds nothing. This fixes the HDFC case (strict pass finds "towards RAZ*SWIGGY" directly, "from" never considered) without weakening the fallback for bodies that only have an ambiguous keyword (Jupiter's "Payment from: NAME" still resolves via the fallback pass, unchanged).

**Cluster F (1 record, IDFC FIRST Bank `CRED TELECOM`) — fix:** added a `\d{2}\s+[a-zA-Z]{3},?\s+\d{2,4}` alternative to `GENERIC_DATE_RE` (space-separated "DD Mon[,] YYYY", comma optional) and matching `chrono` formats (`"%d %b %Y"`, `"%d %b, %Y"`, `"%d %b, %y"`) to `parse_date_generic`.

**Correction found during verification:** the comma-optional version of this same alternative is also what rescues Cluster E's HDFC records ("24 May, 2026" — day-month-comma-year), which I hadn't originally distinguished from IDFC's comma-less "23 MAY 2026" until the HDFC test failed on `event_time` even after the merchant fix. One regex alternative now covers both variants.

**Cluster G (2 records, Jupiter Federal Bank `Payment from:`) — fix:** made the colon between a merchant keyword and the counterparty value optional (`:?\s+` in place of `\s+`) so "Payment from:   NAME" (colon before whitespace) matches. Also required a *fourth* date-regex alternative, `[a-zA-Z]{3}\s+\d{2},\s*\d{4}` + `"%b %d, %Y"` format, for Jupiter's "Month DD, YYYY" ("May 30, 2026") — a date format distinct from both Cluster F's and the HDFC correction's day-first variants. This wasn't in the original Discovery Report (which attributed Cluster G's failure solely to the colon issue) — found the same way, by testing against the real body instead of assuming the colon fix alone was sufficient.

**Doc edits:** none. Doc 30 TASK-TXN-004 (line 714-720) describes Layer 3's merchant heuristic as "capitalized-token or 'at'/'to'/'towards'/'info:' heuristics" — illustrative phrasing ("...or ... heuristics"), not an exhaustive keyword enumeration; the full keyword set (including `from`/`for`/`by`/`paid to`/`merchant`/`beneficiary`/`in favor of`/`purchased at`/`txn at`) already existed in code before any of these fixes and was already broader than the doc's example list. The two-tier restructuring changes match *priority*, not which keywords are recognized — nothing in the doc text is contradicted. (Noted in passing: Doc 30 TASK-TXN-004 also cites `src-tauri/src/extraction/layer3_generic_heuristics.rs`, which doesn't exist — all 6 layers live in one file, `ladder.rs`. Same pattern as Discovery Report Finding F4, extended to the extraction module; not fixed, out of scope unless Aditya wants a Doc 30 path-correction pass.)

**Tests added** (`src-tauri/src/extraction/ladder.rs`, `mod tests`): `test_generic_merchant_heuristic_asterisk_descriptor`, `test_generic_date_space_separated_day_month_year`, `test_generic_merchant_heuristic_colon_label_and_month_first_date` — each built from the real false-negative body text (HDFC, IDFC FIRST Bank, Jupiter respectively), not synthetic placeholders, specifically so the "from"-ambiguity and third-date-format bugs above couldn't hide behind an idealized test body the way they did in my own first draft.

**Verification:**
- `cargo test --lib extraction::ladder` — 45 passed (42 pre-existing + 3 new), 0 failed.
- `cargo test --lib` (full suite) — 597 passed, 4 failed (same 4 pre-existing, unrelated failures as Cluster B — `test_seed_and_fetch`, `test_benchmark_corpus_processes`, `test_beta_onboarding_guide_limitations`, `test_documentation_completeness`).
- Did not invoke `email_segregate.rs` end-to-end for the same reason as Cluster B (requires live DB + Gmail-export wiring outside this session). Correctness verified via unit tests built directly from the real false-negative body text for all 3 sub-clusters, exercising the exact same `GenericRegexLayer::extract` function `email_segregate.rs` calls via `run_extraction_ladder`.

**Status:** Complete.

---

## Mandate Tracking Feature — new decision + implementation (idx 61/62, migrates Cluster D)

**Decision (Aditya, 2026-07-18):** idx 61 (e-Mandate registration) captured same as Cluster D; idx 62 (e-Mandate cancellation) — full feature built (new `recurring_payments` columns, `unresolved_mandate_cancellations` blocking table, third Mandate Queue) rather than a placeholder, per Aditya's explicit choice to scope this as new work for the phase. Full design brainstormed and planned first (`docs/superpowers/specs/2026-07-18-mandate-tracking-design.md`, `docs/superpowers/plans/2026-07-18-mandate-tracking.md`), then executed task-by-task (10 tasks, all committed individually).

**What shipped:**
- Migration `20260101000042_add_mandate_tracking.sql`: `recurring_payments.source`/`external_mandate_id`, new `unresolved_mandate_cancellations` table.
- `recurring_payments.rs`: `upsert_explicit`, `find_active_candidates_for_cancellation`, `mark_cancelled`.
- `unresolved_mandate_cancellations.rs`: `insert_unresolved`.
- `content_classifier.rs`: `ContentClass::MandateRegistration`/`MandateCancellation`, checked before any transaction-verb logic — supersedes Cluster D's `"successful autopay transaction"` verb-list addition (removed, routing happens earlier now).
- `mandate_extractor.rs` (new): merchant/cadence/max-limit/mandate-ID/instrument extraction. Merchant is the only mandatory field.
- `message_processor.rs`: `ProcessResult::MandateEvent`, Gate 2 dispatch branch.
- `queues.rs`: third Mandate Queue (`MandateJob`, `spawn_mandate_workers`, `process_mandate_job`) — registration upserts `recurring_payments`; cancellation matches by `external_mandate_id` else `(instrument, merchant)` among active rows, blocking to `unresolved_mandate_cancellations` on zero/multiple candidates rather than guessing. Both outcomes also send a synthesized `TransactionJob` onto the *existing, unmodified* Transaction Queue for the ₹0.00 row — not a second reconciliation write path.
- `polling.rs`/`historical_scan.rs`: wired `ProcessResult::MandateEvent` routing; added `mandate_events_found` alongside the existing `transactions_found`/`statements_found` scan-progress counters.

**Bugs found only via real-body tests, not assumed correct from the design:**
- `extract_mandate_fields`'s merchant regex initially allowed an optional colon (mirroring `ladder.rs`'s general-purpose regex) — but SBI Card's own boilerplate text says "...e-Mandate at **merchant** platform using your..." *before* the real "**Merchant**: ScribdInc" label, and the optional-colon version matched the boilerplate first (leftmost-match, same class of bug found earlier this session in Cluster E). Fixed by requiring the colon — every real label seen is colon-terminated, the boilerplate usage never is.

**Verified:**
- `cargo test --lib` — 616 passed, same 4 pre-existing unrelated failures (confirmed via this session's established stash-isolation method for the first of the 4; the other 3 have been consistently present and unrelated to any code this session touched, across every full-suite run since Cluster B).
- Full-corpus Python re-simulation (all 69 records, extended with the real mandate-detection logic) — **66/69 pass**, up from 64/69. Remaining 3 (idx 3, 9 — Cluster C; idx 25 — Cluster H) are exactly the deliberate non-captures from earlier decisions, nothing newly broken.

**Status:** Complete. Doc edits (Doc 12/18/30/48) remain — tracked separately.
