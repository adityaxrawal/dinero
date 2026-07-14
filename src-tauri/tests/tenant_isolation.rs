//! TASK-AUTH-008: Enforce Tenant Isolation Pattern Across All IPC Handlers.
//!
//! Document 22 §13.1: Dinero is single-tenant per device. The mandatory
//! invariant isn't multi-tenant row isolation (there's exactly one
//! `local_profile` row, `CHECK(id = 1)`, TASK-DB-003) — it's that the
//! "current profile/session" is always resolved from Rust-side
//! `SessionState`, never trusted from a React-supplied argument. A forged
//! IPC parameter must have nothing to attach to, structurally.

use dinero_app_lib::auth::session::SessionState;
use dinero_app_lib::ipc::middleware::require_active_session;

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
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan(&path, violations);
                continue;
            }
            if path.extension().map(|e| e == "rs") != Some(true) {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else { continue };
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
         (must resolve exclusively from Rust-side SessionState): {:#?}",
        violations
    );
}

/// Exercises `require_active_session`'s actual behavior via the crate's
/// public API: a forged/absent session resolves to a hard error, never to
/// some default or caller-suggested identity.
#[test]
fn require_active_session_never_falls_back_to_a_default_identity() {
    let state = SessionState::default();
    let result = require_active_session(&state);
    assert!(result.is_err(), "an empty SessionState must never resolve to a usable session id");

    *state.0.lock().unwrap() = Some("real_session_id".to_string());
    assert_eq!(
        require_active_session(&state).unwrap(),
        "real_session_id",
        "once a real session is set, it must be the exact value returned — never altered or substituted"
    );
}

#[test]
fn src_dir_used_by_the_scan_actually_exists() {
    let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    assert!(src_dir.join("commands").is_dir());
    assert!(src_dir.join("licensing").is_dir());
    assert!(src_dir.join("ingestion").is_dir());
}
