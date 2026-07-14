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
    tauri_build::build()
}
