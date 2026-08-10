//! Command-line runner for the merchant-normalisation pass.
//!
//! Allows the pass to be run and inspected outside the app, which is how its
//! behaviour is evaluated on real data before being offered in the UI.
use dinero_app_lib::db;
use dinero_app_lib::db::merchants;
use dinero_app_lib::extraction::merchant_normalizer::is_plausible_merchant_name;
use std::path::PathBuf;

/// Default database path when none is supplied.
fn default_db_path() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME not set");
    PathBuf::from(home).join("Library/Application Support/com.dinero.app/finance.db")
}

#[tokio::main]
/// Runs the merchant-normalisation pass from the command line.
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
                    continue;
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
