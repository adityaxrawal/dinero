# Gmail Export Pipeline Hardening + Python CLI — Design

> **For agentic workers:** REQUIRED SUB-SKILL: use superpowers:subagent-driven-development or superpowers:executing-plans to implement, task-by-task.

**Goal:** Fix two real bugs in the uncommitted `email_segregate.rs` WIP (broken resumability, no error resilience), preserve human review annotations across reruns, and wrap the existing pipeline in a single tracked Python CLI entry point (`process` / `review` / `report`) instead of the current two-tools-invoked-separately setup. No classification logic is reimplemented anywhere — the Rust binary already calls the real `SenderValidator` / `ContentClassifier` / `run_extraction_ladder` gates and stays the only place that happens.

## Current State (verified by reading, not assumed)

- `src-tauri/src/bin/email_segregate.rs` (uncommitted WIP, +109/-27 over the last commit `3fdda69`) already reuses the real gate1/gate2/gate3 pipeline and writes one `{label}_combined.json` array per bucket to `gmail_export/segregated_emails/{transaction,statement,non_transaction}/`. A full run already happened: 42,034 emails classified (39,131 non_transaction / 2,633 transaction / 270 statement).
- That output predates today's pipeline fixes (`audit.md`: Cluster B/D/E/F/G content-classifier and extraction-ladder fixes, plus the new Mandate Tracking feature) — reclassifying now will move a meaningful number of records, especially out of `non_transaction`.
- `already_processed_ids()` scans for per-id `<id>.json` stems that no longer exist (output is now 3 combined files) — it always returns ~0 real matches, so every run reclassifies everything and unconditionally overwrites all three combined.json files with no merge step. Today there happen to be zero `_reviews.json` sidecars on disk, so nothing has been destroyed yet, but the next run would silently discard any review data saved between now and then.
- `main()` uses `.expect()`/`panic!` on line read and JSON parse — one malformed record kills the entire batch.
- No `--force` flag exists to distinguish "pick up where I left off" from "reclassify everyone under the fixed pipeline."
- `gmail_export/reviewer_app.py` (972 lines, untracked — `gmail_export/` is fully gitignored) is a working HTTP review UI already shaped for the combined.json format: loads `{folder}_combined.json`, keeps reviews in a separate `{folder}_combined_reviews.json` sidecar keyed by array index (never rewrites the multi-GB combined.json itself), has Save/Prev/Skip, keyboard shortcuts, and a full "expected_*" ground-truth schema (transaction + statement + EMI fields) per record. It currently hardcodes `FOLDERS = ["non_transaction"]` — built for false-negative hunting only.
- `scripts/review_gmail_export.py` (tracked, 3 commits) assumes the old one-file-per-email layout and cannot read the current combined.json format at all — confirmed obsolete, approved for deletion.
- No unified CLI exists; the Rust binary and `reviewer_app.py` are invoked separately by hand.

## Design

### 1. `email_segregate.rs` fixes

- **Resumable-by-id against combined.json.** Before processing, load each existing `{label}_combined.json` (if present) into an `id -> serde_json::Value` map (id read from `record["raw"]["id"]`). Build the skip-set from the union of all three maps' keys. Not `--force`: skip any email id already in that set. `--force`: skip nothing (reclassify all).
- **Preserve annotations across `--force` reruns.** When an id being reclassified already exists in one of the loaded maps, carry its existing `manual_review` and `pipeline_review` fields forward into the new output record verbatim; only `pipeline` (and `raw`, unchanged anyway) gets recomputed. This is the one behavior that must not regress — it's the entire point of keeping review work valid across pipeline fixes.
- **Move-on-reclassify.** If a previously-`non_transaction` id reclassifies to `transaction` under `--force`, it must end up written into `transaction_combined.json` only — not duplicated in both files. Since output is grouped and rewritten per label at the end of the run (existing pattern), this falls out naturally as long as the merge step above operates on a single unified `id -> record` map before regrouping by the *new* `predicted_label`, not by whatever file the id used to live in.
- **Per-record error resilience.** Replace the `.expect()`s in the `pending` filter_map and the `EmailRecord` deserialize with a branch that, on failure, writes one line to `gmail_export/segregation_errors.jsonl` (`{"line_number", "email_id": <id-if-parseable-else-null>, "error": <message>, "occurred_at"}`) and continues to the next line rather than panicking. Track an `errors` counter alongside the existing `counts` map.
- **`--force` flag.** New boolean flag, defaults false, documented above.
- **Machine-readable final summary.** After the existing human-readable `eprintln!` summary, print one line of JSON to stdout: `{"processed", "labels", "errors", "elapsed_secs", "sidecar_version"}` — this is what the Python CLI's `process` command parses to build its own summary; nothing else about the binary's I/O contract changes.

### 2. Python CLI — `scripts/gmail_pipeline.py`

Single tracked entry point (mirrors the flat-script convention already used by `scripts/generate_benchmark_corpus.py` etc.) — `gmail_export/` can't hold tracked source (confirmed by the original plan's own note: the directory-level `.gitignore` entry blocks negation).

- **`gmail_pipeline.py process [--force] [--progress-every N] [--concurrency N] [--emails-jsonl PATH]`**
  Always runs `cargo build --release --bin email_segregate` under `src-tauri/` first (cargo itself is incremental — a no-op if nothing changed, so this is the only staleness check needed), then execs the binary with pass-through flags, streams its stderr progress lines live, captures its final stdout JSON summary line, then prints a combined final report: processed / per-label counts / errors / elapsed, plus current on-disk totals per label (from combined.json lengths) so a partial/incremental run's cumulative state is visible, not just this run's delta.
- **`gmail_pipeline.py review`**
  `os.execv`s into the existing `gmail_export/reviewer_app.py` (no reimplementation, no new UI). Errors clearly if that file is missing.
- **`gmail_pipeline.py report`**
  Reads the 3 `{label}_combined.json` files and their `{label}_combined_reviews.json` sidecars (if present). Prints: total emails, per-label counts, review completion (reviewed/pending per bucket), a confusion-style breakdown — for each reviewed record, `predicted_label` (the bucket it's filed under) vs. `manual_review.type` (what the reviewer said it actually is) — labeling matches as correct and mismatches as false_positive (predicted this bucket, reviewer said otherwise) or false_negative (predicted `non_transaction`, reviewer said `transaction`/`statement`), grouped by `pipeline.rejection_reason`/`gate2_result` the same way `audit.md`'s manual clustering already did by hand. Also reports the error count from `segregation_errors.jsonl` if present.

### 3. Cleanup

- Delete `scripts/review_gmail_export.py` (confirmed obsolete — cannot read current output format, superseded by `report` above).
- `reviewer_app.py`: change `FOLDERS = ["non_transaction"]` to `["transaction", "statement", "non_transaction"]` so false positives in the transaction/statement buckets are reviewable too, not just false negatives. No other change to that file — it's untracked/gitignored data tooling, out of scope to restructure further.

### 4. Documentation

New `docs/gmail-export-pipeline.md` (tracked, since `docs/` isn't gitignored): architecture (why Rust does classification and Python only orchestrates), the `process`/`review`/`report` execution flow, the annotation-preservation guarantee across `--force` reruns, and how the `report` command's false_positive/false_negative groupings are meant to be consumed for future pipeline fixes — using today's `audit.md` Cluster B–H cycle as the worked example of the loop this tooling is meant to support.

## Error/data formats

- `gmail_export/segregation_errors.jsonl` — one JSON object per line, per malformed record, never overwritten (append-only, matches `audit.md`'s own append-only convention).
- `{label}_combined.json` — unchanged shape: array of `{"raw", "pipeline", "pipeline_review", "manual_review"}`.
- `{label}_combined_reviews.json` — unchanged, owned entirely by `reviewer_app.py` (index-keyed sidecar); `report` only reads it.

## Testing plan

- Rust: unit tests for the new merge/skip logic (id already present + `--force` → old `manual_review` survives, `pipeline` changes; id already present + no `--force` → record untouched, not even re-read) and for the error-continuation path (one malformed line among several valid ones → valid ones still processed, one line appended to `segregation_errors.jsonl`, exit code still 0). Run via `cargo test --bin email_segregate`.
- Python: exercise `process`/`report` against a small synthetic `gmail_export/` fixture under `/tmp` (few records across labels, one with a saved review, one malformed line) rather than the real 1.5GB export — proves the merge/report logic without touching real data or Keychain.
- Manual smoke test (by the user, per the earlier decision to leave the real production rerun to them): `process --force` against the real export once code is verified, followed by `report`.

## Out of scope

- Running the real full batch against production data (explicit user decision — tooling only this session).
- Any change to `reviewer_app.py` beyond the one-line `FOLDERS` change.
- Any change to the actual classification logic (content_classifier.rs, ladder.rs, verified_senders.rs, mandate_extractor.rs) — those were already fixed today per `audit.md`; this task only makes the harness around them correct and durable.
