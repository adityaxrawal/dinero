//! Cargo build script.
//!
//! Runs `tauri_build`, which generates the context the application embeds --
//! bundled frontend assets, the capability/permission manifest, and platform
//! resources. Without it `tauri::generate_context!` has nothing to expand.
/// Runs the Tauri build step and regenerates the bank template list.
fn main() {
    println!("cargo:rerun-if-changed=../.env");
    let env_path = std::path::Path::new("../.env");
    if env_path.exists() {
        if let Ok(contents) = std::fs::read_to_string(env_path) {
            for line in contents.lines() {
                let line = line.trim();
                if let Some(stripped) = line.strip_prefix("export ") {
                    let parts: Vec<&str> = stripped.splitn(2, '=').collect();
                    if parts.len() == 2 {
                        let key = parts[0].trim();
                        let value = parts[1].trim().trim_matches('"').trim_matches('\'');
                        println!("cargo:rustc-env={}={}", key, value);
                    }
                } else if !line.starts_with('#') && line.contains('=') {
                    let parts: Vec<&str> = line.splitn(2, '=').collect();
                    if parts.len() == 2 {
                        let key = parts[0].trim();
                        let value = parts[1].trim().trim_matches('"').trim_matches('\'');
                        println!("cargo:rustc-env={}={}", key, value);
                    }
                }
            }
        }
    }
    generate_bank_template_list();
    tauri_build::build()
}

/// Generates the compile-time list of banks that ship with a template.
///
/// Derived from the template files themselves, so adding a template does not also
/// require editing a hand-maintained list that could drift out of step.
fn generate_bank_template_list() {
    let dir = std::path::Path::new("assets/bank_templates");
    println!("cargo:rerun-if-changed=assets/bank_templates");

    let mut files: Vec<String> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("assets/bank_templates must be readable: {e}"))
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().to_string_lossy().into_owned();
            name.ends_with(".json").then_some(name)
        })
        .collect();
    files.sort();

    assert!(
        !files.is_empty(),
        "assets/bank_templates contains no *.json files -- Layer 2 would silently \
         have zero templates for every bank"
    );

    let mut out = String::from("const BANK_TEMPLATE_FILES: &[(&str, &str)] = &[\n");
    for name in &files {
        out.push_str(&format!(
            "    ({name:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \
             \"/assets/bank_templates/\", {name:?}))),\n"
        ));
    }
    out.push_str("];\n");

    let dest = std::path::Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo"))
        .join("bank_template_files.rs");
    std::fs::write(&dest, out).unwrap_or_else(|e| panic!("writing {dest:?} failed: {e}"));
}
