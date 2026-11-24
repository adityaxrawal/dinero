//! Doc 12 §8.2a: the build must fail if any code path creates a row in
//! `transactions` outside the reconciliation engine (audit A-12).
//!
//! The rule, after A-9: rows are created only by
//! `db::transactions::insert_transaction`, which only
//! `reconciliation::canonical` calls; `create_canonical_transaction` is called
//! only from `reconciliation::engine` (ingest/complete) and
//! `reconciliation::cluster` (human cluster resolution). Updates to existing
//! rows (category, merchant cleanup, alert flags) are not creation and are
//! out of scope. Test modules are excluded: production code is everything
//! before a file's first `#[cfg(test)]`, plus no `*_tests.rs` / `tests.rs`.
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};

fn production_sources() -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n != "tests") {
                    walk(&path, out);
                }
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    walk(&src, &mut files);
    files
        .into_iter()
        .filter(|p| {
            let name = p.file_name().unwrap().to_string_lossy();
            !name.ends_with("_tests.rs") && name != "tests.rs"
        })
        .map(|p| {
            let text = fs::read_to_string(&p).unwrap();
            let prod = text.split("#[cfg(test)]").next().unwrap().to_string();
            let rel = p.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/");
            (rel, prod)
        })
        .collect()
}

fn files_matching(pattern: &str) -> Vec<String> {
    let re = Regex::new(pattern).unwrap();
    let mut hits: Vec<String> = production_sources()
        .into_iter()
        .filter(|(_, prod)| re.is_match(prod))
        .map(|(path, _)| path)
        .collect();
    hits.sort();
    hits
}

#[test]
fn only_db_transactions_inserts_transaction_rows() {
    assert_eq!(
        files_matching(r"(?i)(INSERT|REPLACE)(\s+OR\s+\w+)?\s+INTO\s+transactions\b[^_]"),
        vec!["db/transactions.rs".to_string()],
        "a new SQL insert into `transactions` bypasses ingest_observation (Doc 12 §8.2a)"
    );
}

#[test]
fn only_the_canonical_writer_calls_insert_transaction() {
    let callers: Vec<String> = files_matching(r"\binsert_transaction\s*\(")
        .into_iter()
        .filter(|f| f != "db/transactions.rs")
        .collect();
    assert_eq!(
        callers,
        vec!["reconciliation/canonical.rs".to_string()],
        "insert_transaction may only be called by reconciliation::canonical"
    );
}

#[test]
fn canonical_transactions_are_created_only_by_the_engine_and_cluster_resolution() {
    let callers: Vec<String> = files_matching(r"\bcreate_canonical_transaction\s*\(")
        .into_iter()
        .filter(|f| f != "reconciliation/canonical.rs")
        .collect();
    assert_eq!(
        callers,
        vec!["reconciliation/cluster.rs".to_string(), "reconciliation/engine.rs".to_string()],
        "create_canonical_transaction may only be called from the engine or cluster resolution"
    );
}
