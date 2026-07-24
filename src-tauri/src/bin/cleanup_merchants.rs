//! One-off maintenance script: removes `merchants` rows that were
//! auto-created from boilerplate/garbage extraction before the
//! `is_plausible_merchant_name` gate existed (e.g. SBI Card's mis-anchored
//! "inform you that"), so a future email producing the same garbage falls
//! through to the now-guarded auto-create path instead of exact-matching
//! the already-poisoned alias. Never touches `source = 'user'` merchants
//! (anything a person confirmed by hand). Dry-run by default.
//!
//! Usage:
//!   cargo run --bin cleanup_merchants                    # dry-run, real db
//!   cargo run --bin cleanup_merchants -- --apply          # actually delete
//!   cargo run --bin cleanup_merchants -- --db PATH --apply

use dinero_app_lib::db;
use dinero_app_lib::db::merchants;
use dinero_app_lib::extraction::merchant_normalizer::is_plausible_merchant_name;
use std::path::PathBuf;

fn default_db_path() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME not set");
    PathBuf::from(home).join("Library/Application Support/com.dinero.app/finance.db")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let apply = args.iter().any(|a| a == "--apply");
    let db_path = args
        .iter()
        .position(|a| a == "--db")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .unwrap_or_else(default_db_path);

    println!(
        "Using database: {} ({})",
        db_path.display(),
        if apply { "APPLY" } else { "dry-run" }
    );
    let pool = db::init_db(db_path).await?;
    let conn = pool.get().await?;

    let removed = conn
        .interact(move |c| -> anyhow::Result<Vec<(String, String)>> {
            let mut removed = Vec::new();
            for m in merchants::select_all(c)? {
                if m.source != "system" {
                    continue; // never touch a user-confirmed merchant
                }
                if is_plausible_merchant_name(&m.normalized_name) {
                    continue;
                }
                removed.push((m.id.clone(), m.normalized_name.clone()));
                if apply {
                    for alias in merchants::select_aliases_by_merchant_id(c, &m.id)? {
                        merchants::delete_alias(c, &alias.id)?;
                    }
                    merchants::soft_delete(c, &m.id)?;
                }
            }
            Ok(removed)
        })
        .await
        .map_err(|e| anyhow::anyhow!("pool interact error: {:?}", e))??;

    if removed.is_empty() {
        println!("No implausible system merchants found.");
        return Ok(());
    }

    for (id, name) in &removed {
        println!(
            "{}  {id}  {name:?}",
            if apply { "REMOVED    " } else { "WOULD REMOVE" }
        );
    }
    println!(
        "\n{} implausible merchant(s) {}.",
        removed.len(),
        if apply {
            "removed"
        } else {
            "found -- dry-run, pass --apply to remove"
        }
    );

    Ok(())
}
