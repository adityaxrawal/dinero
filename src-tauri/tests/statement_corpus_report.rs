//! Accuracy check over the owner's real statements (current-issues #4).
//!
//! Opt-in: reads `real-test-data/STATEMENT-PDFS`, which only exists on the
//! owner's machine. Run with
//! `cargo test --test statement_corpus_report -- --ignored --nocapture`.
//! Prints, per file: whether it opened, pages, rows read, unreadable pages and
//! lines that look like transactions but weren't read.
//!
//! Password-protected statements are read only if their passwords are given
//! locally, never stored in the repo: `DINERO_CORPUS_PASSWORDS="pw1;pw2"`
//! (each is tried on each protected file).

use dinero_app_lib::statements::parser::parse_in_memory_with_password;
use dinero_app_lib::statements::password::is_pdf_unencrypted;
use dinero_app_lib::statements::row_extractor::{extract_rows, missed_lines};

#[tokio::test]
#[ignore = "reads the owner's real statements; run on request"]
async fn report_statement_corpus() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../real-test-data");
    let mut files: Vec<_> = ["STATEMENT-PDFS", "MAILS/statements"]
        .iter()
        .filter_map(|d| std::fs::read_dir(root.join(d)).ok())
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pdf")))
        .collect();
    files.sort();
    for (i, path) in files.iter().enumerate() {
        let bytes = std::fs::read(path).unwrap();
        let mut password: Option<String> = None;
        if !is_pdf_unencrypted(&bytes).await {
            let candidates = std::env::var("DINERO_CORPUS_PASSWORDS").unwrap_or_default();
            for pw in candidates.split(';').filter(|p| !p.is_empty()) {
                if let Ok(r) = parse_in_memory_with_password(&bytes, Some(pw)).await {
                    if !r.pages.is_empty() {
                        password = Some(pw.to_string());
                        break;
                    }
                }
            }
            if password.is_none() {
                println!("CORPUS #{i}: password-protected (set DINERO_CORPUS_PASSWORDS to read it)");
                continue;
            }
        }
        match parse_in_memory_with_password(&bytes, password.as_deref()).await {
            Err(e) => println!("CORPUS #{i}: did not open ({})", e.to_string().lines().next().unwrap_or("")),
            Ok(parsed) => {
                let rows = extract_rows(&parsed.pages).unwrap_or_default();
                let missed = missed_lines(&parsed.pages);
                let chars: usize = parsed.pages.iter().map(|p| p.text.len()).sum();
                println!(
                    "CORPUS #{i}: pages={} text_chars={} rows={} unreadable_pages={:?} missed_lines={}",
                    parsed.pages.len(),
                    chars,
                    rows.len(),
                    parsed.missing_pages,
                    missed.len()
                );
                for m in missed.iter().take(8) {
                    println!("CORPUS #{i}   missed p{}: {}", m.page, m.text);
                }
            }
        }
    }
}
