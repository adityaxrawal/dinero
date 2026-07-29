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

/// Emits the `BANK_TEMPLATE_FILES` array consumed by
/// `extraction::ladder::bank_templates` -- one `(filename, include_str!(..))`
/// pair per `assets/bank_templates/*.json`. Generated rather than
/// hand-maintained because there is one template file per verified-sender
/// registry bank (~139), where a forgotten `include_str!` line would be a
/// silent, whole-bank Layer 2 coverage gap rather than a compile error.
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
    // Sorted so the generated file is stable across filesystems -- an
    // arbitrary readdir order would rewrite it (and trigger a rebuild) on
    // machines that enumerate differently.
    files.sort();

    assert!(
        !files.is_empty(),
        "assets/bank_templates contains no *.json files -- Layer 2 would silently \
         have zero templates for every bank"
    );

    let mut out = String::from("const BANK_TEMPLATE_FILES: &[(&str, &str)] = &[\n");
    for name in &files {
        // CARGO_MANIFEST_DIR-relative so the generated file works from OUT_DIR.
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
