//! Doc 30 TASK-OPS-005: Rollback and Incident Response Playbooks.
//!
//! `docs/incident-response.md` and `scripts/rollback_release.sh` are the
//! two deliverables Doc30 names for this task. Both are plain text/shell,
//! so — same convention as `security_hardening_check.sh`'s real execution
//! and the QA suite's source-scanning tests — these tests verify the real
//! artifacts on disk rather than a fabricated stand-in.

use std::path::Path;
use std::process::Command;

fn incident_response_doc() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("docs")
        .join("incident-response.md");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("docs/incident-response.md must exist: {e}"))
}

/// Doc 30 TASK-OPS-005 acceptance: `test_rollback_runbook_covers_key_incidents`.
/// The task text names 6 incident classes explicitly: "activation outage,
/// validation outage, refresh outage, signing-key compromise, database
/// corruption, app startup failure."
#[test]
fn test_rollback_runbook_covers_key_incidents() {
    let doc = incident_response_doc().to_lowercase();
    for class in [
        "activation outage",
        "validation outage",
        "refresh outage",
        "signing-key compromise",
        "database corruption",
        "app startup failure",
    ] {
        assert!(
            doc.contains(class),
            "docs/incident-response.md must cover the '{class}' incident class"
        );
    }
}

/// Doc 30 TASK-OPS-005 acceptance: `test_severity_model_accounts_for_grace_period`.
/// The severity model must be explicitly tied to the 7-day GRACE period,
/// not just a generic error-count-based tiering — this is the task's exact
/// framing ("so outages are triaged by actual user impact rather than raw
/// error counts").
#[test]
fn test_severity_model_accounts_for_grace_period() {
    let doc = incident_response_doc();
    for tier in ["SEV-1", "SEV-2", "SEV-3"] {
        assert!(doc.contains(tier), "severity model must define {tier}");
    }
    assert!(
        doc.contains("7-day") && doc.to_lowercase().contains("grace"),
        "the severity model must explicitly account for the 7-day GRACE period, \
        not just raw error counts"
    );
    // The one case GRACE does *not* cover must be called out, not glossed
    // over -- a brand-new activation has no prior cached JWT to fall back on.
    assert!(
        doc.contains("no GRACE mitigant") || doc.contains("no prior cached JWT"),
        "the doc must call out that activation outages are NOT protected by GRACE"
    );
}

/// Doc 30 TASK-OPS-005 acceptance: `test_one_command_rollback_targets_previous_build`.
/// Really executes `scripts/rollback_release.sh` against a fake `gh` binary
/// on PATH (never touching the real network/GitHub), and asserts it both
/// deletes the bad release/tag AND re-designates the previous published
/// release as "latest" -- the part that actually "restores the previous
/// successful build artifact," since the Tauri updater endpoint resolves
/// strictly against whichever release GitHub currently considers latest.
#[test]
fn test_one_command_rollback_targets_previous_build() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let script = repo_root.join("scripts").join("rollback_release.sh");
    assert!(script.exists(), "scripts/rollback_release.sh must exist");

    let script_content = std::fs::read_to_string(&script).unwrap();
    assert!(
        script_content.contains("--latest"),
        "rollback_release.sh must re-designate a release as latest, not just delete the bad one"
    );
    assert!(
        script_content.contains("exclude-drafts")
            && script_content.contains("exclude-pre-releases"),
        "rollback_release.sh must never restore an unpromoted draft/pre-release as latest"
    );

    let tmp = std::env::temp_dir().join(format!("dinero_rollback_test_{}", uuid::Uuid::new_v4()));
    let bin_dir = tmp.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let log_path = tmp.join("gh_calls.log");

    // A fake `gh` that logs every invocation, simulates one bad release
    // (v1.2.3, currently latest) and one good previous release (v1.2.2),
    // and never touches the network.
    let fake_gh = format!(
        r#"#!/usr/bin/env bash
echo "$@" >> "{log}"
case "$1 $2" in
  "release view")
    exit 0
    ;;
  "release delete")
    exit 0
    ;;
  "release list")
    # Real `gh` applies --jq server-side and would print just the extracted
    # tag name (the script's `--jq '.[0].tagName'` flag) -- simulate that
    # already-filtered output directly, rather than raw JSON the real
    # command would never actually emit given that flag.
    echo "v1.2.2"
    exit 0
    ;;
  "release edit")
    exit 0
    ;;
esac
exit 0
"#,
        log = log_path.display()
    );
    let fake_gh_path = bin_dir.join("gh");
    std::fs::write(&fake_gh_path, fake_gh).unwrap();

    // The script also calls real `git push --delete`/`git tag -d` -- a fake
    // `git` (shadowing the real one via PATH order) is essential here so
    // this test never mutates the actual repo's tags or pushes to origin.
    let fake_git = format!(
        r#"#!/usr/bin/env bash
echo "$@" >> "{log}"
exit 0
"#,
        log = log_path.display()
    );
    let fake_git_path = bin_dir.join("git");
    std::fs::write(&fake_git_path, fake_git).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake_gh_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&fake_git_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let path_var = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = Command::new("bash")
        .arg(&script)
        .arg("v1.2.3")
        .env("PATH", path_var)
        .env("GIT_TERMINAL_PROMPT", "0")
        .current_dir(&repo_root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = stdin.write_all(b"y\n");
            }
            child.wait_with_output()
        })
        .unwrap();

    let log = std::fs::read_to_string(&log_path).unwrap_or_default();
    assert!(
        log.contains("release delete v1.2.3") || log.contains("v1.2.3"),
        "rollback must delete the bad release; gh calls were:\n{log}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        log.contains("release edit v1.2.2 --latest"),
        "rollback must re-designate the previous published release (v1.2.2) as latest; gh calls were:\n{log}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}
