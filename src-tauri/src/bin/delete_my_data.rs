//! Command-line tool that erases all local application data.
//!
//! Provides a way to delete everything that does not depend on the GUI starting
//! -- which matters precisely when the database is corrupt and the app will not
//! launch.
use dinero_app_lib::commands::data::perform_account_deletion;
use dinero_app_lib::db;
use std::io::{self, Write};
use std::path::PathBuf;

/// Default app data directory.
fn default_app_data_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("com.dinero.app");
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("com.dinero.app");
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join("com.dinero.app");
        }
    }
    PathBuf::from(".")
}

#[tokio::main]
/// Erases all local data without needing the GUI.
///
/// Exists precisely for the case where the database is corrupt and the app will
/// not start.
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let auto_confirm = args
        .iter()
        .any(|a| a == "--yes" || a == "--force" || a == "-y");

    let app_dir = args
        .iter()
        .position(|a| a == "--app-dir")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .or_else(|| {
            args.iter()
                .position(|a| a == "--db")
                .and_then(|i| args.get(i + 1))
                .and_then(|db_str| PathBuf::from(db_str).parent().map(|p| p.to_path_buf()))
        })
        .unwrap_or_else(default_app_data_dir);

    let db_path = app_dir.join("finance.db");

    println!("--------------------------------------------------");
    println!("DELETE MY DATA - Local Wipe Command");
    println!("Target App Directory: {}", app_dir.display());
    println!("Target Database Path:  {}", db_path.display());
    println!("--------------------------------------------------");
    println!(
        "WARNING: Permanently delete all your data from this device:\n\
         transactions, statements, instruments, connected Gmail accounts,\n\
         and encryption keys. This action cannot be undone."
    );
    println!("--------------------------------------------------");

    if !auto_confirm {
        print!("Type 'DELETE MY DATA' to confirm deletion: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if input.trim() != "DELETE MY DATA" {
            println!("Aborted. Confirmation phrase did not match 'DELETE MY DATA'.");
            std::process::exit(1);
        }
    } else {
        println!("Automated confirmation supplied (--yes / --force). Proceeding...");
    }

    println!("Starting data deletion...");

    let pool = if db_path.exists() {
        match db::init_db(db_path.clone()).await {
            Ok(p) => Some(p),
            Err(e) => {
                println!(
                    "Warning: Could not open SQLite database pool ({}). Proceeding with local wipe anyway.",
                    e
                );
                None
            }
        }
    } else {
        println!(
            "No finance.db found at target path. Proceeding with keychain and backup cleanup..."
        );
        None
    };

    perform_account_deletion(&app_dir, pool.as_ref())
        .await
        .map_err(|e| anyhow::anyhow!("Failed during account deletion: {:?}", e))?;

    println!("--------------------------------------------------");
    println!("DELETE MY DATA completed successfully.");
    println!("All local database files, backup files, and Keychain entries have been removed.");
    println!("--------------------------------------------------");

    Ok(())
}
