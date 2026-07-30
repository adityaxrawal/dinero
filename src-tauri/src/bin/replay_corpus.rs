//! Dev-only measurement harness: replays a real fetched-mail corpus through
//! the actual extraction ladder and reports, per bank, which layer produced
//! each result.
//!
//! This exists because the only honest measure of a bank-template change is
//! how many *real* emails it newly extracts — a template that looks right and
//! never fires is worse than no template, since it costs maintenance without
//! moving accuracy. Run it before and after a template change; the number to
//! move is `bank_templates`' share.
//!
//! The corpus is gitignored local data, so this skips cleanly when absent
//! (same posture as `corpus_root()` in the phase-10 quality-gate tests).
//!
//! Usage:
//!   cargo run --bin replay_corpus                       # default corpus path
//!   cargo run --bin replay_corpus -- --corpus PATH
//!   cargo run --bin replay_corpus -- --bank "HDFC Bank" # one bank, verbose
//!   cargo run --bin replay_corpus -- --merchants        # rank extracted merchants

use dinero_app_lib::extraction::ladder::run_extraction_ladder;
use dinero_app_lib::extraction::lexicon::is_stopword_only_merchant;
use dinero_app_lib::extraction::merchant_confidence::{score_merchant, LOW_CONFIDENCE_THRESHOLD};
use dinero_app_lib::extraction::merchant_normalizer::{
    is_plausible_merchant_name, strip_noise_tokens,
};
use dinero_app_lib::ingestion::mime_sanitization::sanitize_html;
use dinero_app_lib::ingestion::verified_senders::{SenderVerificationResult, SenderValidator};
use std::collections::BTreeMap;

const DEFAULT_CORPUS: &str = "../real-test-data/JSON-FETCHED-MAILS/all_emails.json";

#[derive(serde::Deserialize)]
struct Corpus {
    emails: Vec<Email>,
}

#[derive(serde::Deserialize)]
struct Email {
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    snippet: Option<String>,
    #[serde(default)]
    #[serde(rename = "bodyText")]
    body_text: Option<String>,
    #[serde(default)]
    #[serde(rename = "internalDate")]
    internal_date: Option<serde_json::Value>,
}

#[derive(Default)]
struct Stats {
    by_layer: BTreeMap<String, usize>,
    unextracted: usize,
}

fn sender_address(from: &str) -> String {
    from.rsplit('<')
        .next()
        .unwrap_or(from)
        .trim_end_matches('>')
        .trim()
        .to_lowercase()
}

/// Mirrors `MessageProcessor`'s body selection: prefer the text part, fall
/// back to flattening HTML, then to Gmail's snippet.
fn body_of(e: &Email) -> String {
    let raw = e.body_text.clone().unwrap_or_default();
    let looks_html = raw.trim_start().starts_with('<') || raw.contains("<html");
    let body = if looks_html {
        sanitize_html(&raw)
    } else {
        raw
    };
    if body.trim().is_empty() {
        e.snippet.clone().unwrap_or_default()
    } else {
        body
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |k: &str| {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let path = arg("--corpus").unwrap_or_else(|| DEFAULT_CORPUS.to_string());
    let only_bank = arg("--bank");
    let merchant_mode = args.iter().any(|a| a == "--merchants");
    let mut merchants: BTreeMap<String, (usize, String)> = BTreeMap::new();

    let Ok(raw) = std::fs::read_to_string(&path) else {
        eprintln!("SKIP: no corpus at {path} (gitignored local data). Nothing to measure.");
        return;
    };
    let corpus: Corpus = serde_json::from_str(&raw).expect("corpus must be valid JSON");

    let mgr = deadpool_sqlite::Manager::from_config(
        &deadpool_sqlite::Config {
            path: ":memory:".into(),
            pool: Some(deadpool_sqlite::PoolConfig::new(1)),
        },
        deadpool_sqlite::Runtime::Tokio1,
    );
    let pool = deadpool_sqlite::Pool::builder(mgr).build().unwrap();
    let validator = SenderValidator::new();

    let mut per_bank: BTreeMap<String, Stats> = BTreeMap::new();
    let mut considered = 0usize;

    for e in &corpus.emails {
        let Some(from) = e.from.as_deref() else {
            continue;
        };
        // Gate 1, exactly as the real pipeline runs it.
        let bank = match validator.verify_sender(&sender_address(from), None) {
            SenderVerificationResult::VerifiedTransactionCandidate(b)
            | SenderVerificationResult::VerifiedStatementCandidate(b) => b,
            _ => continue,
        };
        if only_bank.as_ref().is_some_and(|b| *b != bank) {
            continue;
        }

        let body = body_of(e);
        // Gate 2 proper needs the full classifier; this harness only skips
        // bodies with no currency-marked amount at all, which no extraction
        // layer could succeed on anyway.
        if !body.contains('₹')
            && !body.to_lowercase().contains("rs.")
            && !body.to_lowercase().contains("rs ")
            && !body.to_uppercase().contains("INR")
        {
            continue;
        }
        considered += 1;

        let internal_date = e.internal_date.as_ref().and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse::<i64>().ok())
                .or_else(|| v.as_i64())
                .map(|ms| ms / 1000)
        });

        let mut timed_out = false;
        // `llm_eligible: false` -- Layers 1-5 only, matching the scan path.
        let result = run_extraction_ladder(
            &pool,
            &bank,
            &body,
            None,
            false,
            internal_date,
            &mut timed_out,
            None,
        )
        .await
        .ok()
        .flatten();

        let stats = per_bank.entry(bank.clone()).or_default();
        match result {
            Some(obs) => {
                if let Some(raw) = obs.merchant_raw.as_deref() {
                    // Key on the post-cleanup form, which is what actually
                    // becomes a `merchants` row.
                    let cleaned = strip_noise_tokens(raw);
                    if !cleaned.is_empty() {
                        let e = merchants
                            .entry(cleaned)
                            .or_insert((0, format!("{bank} | {}", obs.extraction_method)));
                        e.0 += 1;
                    }
                }
                *stats.by_layer.entry(obs.extraction_method).or_default() += 1;
            }
            None => stats.unextracted += 1,
        }

        if only_bank.is_some() && considered <= 5 {
            println!("--- {:?}\n{}\n", e.subject.as_deref().unwrap_or(""), {
                let t = body.split_whitespace().collect::<Vec<_>>().join(" ");
                t.chars().take(240).collect::<String>()
            });
        }
    }

    if merchant_mode {
        let mut rows: Vec<(&String, &(usize, String))> = merchants.iter().collect();
        rows.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(b.0)));
        println!(
            "\n{:<46} {:>6}  {:<7} {:<7} {:>5} {}",
            "MERCHANT (post strip_noise_tokens)", "COUNT", "STOPONLY", "PLAUSIBL", "CONF", "SOURCE"
        );
        println!("{}", "-".repeat(108));
        for (name, (count, src)) in &rows {
            // Both existing gates, as the pipeline would apply them.
            let stop_only = is_stopword_only_merchant(name);
            let plausible = is_plausible_merchant_name(name);
            // `established = false`: on a fresh database every one of these
            // is auto-created from a single email, which is exactly the state
            // the cleanup pass is meant to unwind. A user's real DB will rate
            // the recurring merchants higher.
            let layer = src.rsplit('|').next().unwrap_or("").trim();
            let conf = score_merchant(Some(layer), name, false);
            println!(
                "{name:<46} {count:>6}  {:<7} {:<7} {conf:>5.2} {src}",
                if stop_only { "REJECT" } else { "pass" },
                if plausible { "pass" } else { "REJECT" },
            );
        }

        let reaching: Vec<_> = rows
            .iter()
            .filter(|(n, _)| !is_stopword_only_merchant(n) && is_plausible_merchant_name(n))
            .collect();
        let queued: Vec<_> = reaching
            .iter()
            .filter(|(n, (_, src))| {
                let layer = src.rsplit('|').next().unwrap_or("").trim();
                score_merchant(Some(layer), n, false) < LOW_CONFIDENCE_THRESHOLD
            })
            .collect();
        let queued_occurrences: usize = queued.iter().map(|(_, (c, _))| *c).sum();

        println!(
            "\ndistinct merchants: {}  |  reaching a merchants row today: {}",
            rows.len(),
            reaching.len()
        );
        println!(
            "issue #12 queue: {} distinct ({} occurrences) score below {:.2} and would go to the LLM",
            queued.len(),
            queued_occurrences,
            LOW_CONFIDENCE_THRESHOLD
        );
        return;
    }

    let mut totals: BTreeMap<String, usize> = BTreeMap::new();
    let mut total_unextracted = 0usize;
    println!("\n{:<44} {:>7} {:>7} {:>7} {:>7}", "BANK", "L2", "L3/4", "L5", "none");
    println!("{}", "-".repeat(76));
    for (bank, s) in &per_bank {
        let l2 = *s.by_layer.get("bank_templates").unwrap_or(&0);
        let other: usize = s
            .by_layer
            .iter()
            .filter(|(k, _)| k.as_str() != "bank_templates" && !k.starts_with("layer5"))
            .map(|(_, v)| *v)
            .sum();
        let l5: usize = s
            .by_layer
            .iter()
            .filter(|(k, _)| k.starts_with("layer5"))
            .map(|(_, v)| *v)
            .sum();
        if l2 + other + l5 + s.unextracted == 0 {
            continue;
        }
        println!("{bank:<44} {l2:>7} {other:>7} {l5:>7} {:>7}", s.unextracted);
        for (k, v) in &s.by_layer {
            *totals.entry(k.clone()).or_default() += v;
        }
        total_unextracted += s.unextracted;
    }

    let extracted: usize = totals.values().sum();
    println!("{}", "-".repeat(76));
    println!("\namount-bearing mails from verified bank senders: {considered}");
    println!("extracted: {extracted}   unextracted: {total_unextracted}");
    for (layer, n) in &totals {
        println!(
            "  {layer:<28} {n:>6}  ({:.1}% of extracted)",
            100.0 * *n as f64 / extracted.max(1) as f64
        );
    }
}
