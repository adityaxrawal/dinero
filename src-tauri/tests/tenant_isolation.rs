//! TASK-AUTH-008: Enforce Tenant Isolation Pattern Across All IPC Handlers.
//!
//! Document 22 §13.1: Dinero is single-tenant per device. The mandatory
//! invariant isn't multi-tenant row isolation (there's exactly one
//! `local_profile` row, `CHECK(id = 1)`, TASK-DB-003) — it's that the
//! current profile is always resolved on the Rust side, never trusted from a
//! React-supplied argument. A forged IPC parameter must have nothing to attach
//! to, structurally.

/// `test_data_isolation_ipc_parameter_attack`: the mechanical half of the
/// convention Document 30 describes ("IPC argument structs must never carry
/// a `profile_id`/`user_id`-style field") — scans every `#[tauri::command]`
/// function's full signature (joining wrapped multi-line signatures) across
/// the entire command surface for such a parameter. A "forged IPC call
/// injecting a different profile context" has no parameter to inject *into*
/// if this scan passes: the attack surface this task defends against is
/// structurally absent, not merely rejected at runtime.
#[test]
fn test_data_isolation_ipc_parameter_attack() {
    let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut violations = Vec::new();

    fn scan(dir: &std::path::Path, violations: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan(&path, violations);
                continue;
            }
            if path.extension().map(|e| e == "rs") != Some(true) {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let lines: Vec<&str> = content.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                if line.trim() != "#[tauri::command]" {
                    continue;
                }
                let sig: String = lines[i + 1..(i + 15).min(lines.len())].join(" ");
                let sig_before_body = sig.split('{').next().unwrap_or(&sig).to_lowercase();
                if sig_before_body.contains("profile_id:")
                    || sig_before_body.contains("user_id:")
                    || sig_before_body.contains("session_id:")
                {
                    violations.push(format!(
                        "{}:{}: IPC command signature carries a profile_id/user_id/session_id-style parameter: {}",
                        path.display(),
                        i + 1,
                        sig_before_body.trim()
                    ));
                }
            }
        }
    }

    scan(&src_dir, &mut violations);
    assert!(
        violations.is_empty(),
        "no IPC command may accept an identity-scoping parameter from the caller \
         (identity is resolved on the Rust side only): {:#?}",
        violations
    );
}

#[test]
fn src_dir_used_by_the_scan_actually_exists() {
    let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    assert!(src_dir.join("commands").is_dir());
    assert!(src_dir.join("licensing").is_dir());
    assert!(src_dir.join("ingestion").is_dir());
}
