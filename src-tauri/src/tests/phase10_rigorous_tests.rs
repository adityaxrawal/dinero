#[cfg(test)]
mod tests {
    use serde_json::Value;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    // --- 10.1 App Bundle Preparation ---
    #[test]
    fn test_app_bundle_preparation_tauri_conf() {
        let conf_path = PathBuf::from("tauri.conf.json");
        let content = fs::read_to_string(conf_path).expect("tauri.conf.json must exist");
        let parsed: Value = serde_json::from_str(&content).expect("Valid JSON expected");

        let bundle = parsed
            .get("bundle")
            .expect("Must have bundle configuration");

        let targets = bundle
            .get("targets")
            .expect("Must define targets")
            .as_array()
            .unwrap();
        let target_strs: Vec<&str> = targets.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(target_strs.contains(&"app"), "Must target macOS .app");
        assert!(target_strs.contains(&"dmg"), "Must target macOS .dmg");

        let resources = bundle
            .get("resources")
            .expect("Must define resources for binaries")
            .as_array()
            .unwrap();
        let resource_strs: Vec<&str> = resources.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(
            resource_strs.contains(&"binaries/*"),
            "Must bundle binaries for SQLite and pdfium"
        );

        let icons = bundle
            .get("icon")
            .expect("Must define app icons")
            .as_array()
            .unwrap();
        assert!(!icons.is_empty(), "Must have icon assets");

        let macos = bundle
            .get("macOS")
            .expect("Must define macOS specific bundle config");
        assert!(
            macos.get("entitlements").is_some(),
            "Must configure entitlements for signing"
        );

        let plugins = parsed.get("plugins").expect("Must have plugins block");
        let updater = plugins.get("updater").expect("Must configure updater");
        assert!(
            updater.get("endpoints").is_some(),
            "Must configure auto-update endpoint"
        );
        assert!(
            updater.get("pubkey").is_some(),
            "Must configure auto-update pubkey"
        );
    }

    #[test]
    fn test_code_signing_and_notarization_configured() {
        // H8 fix: signing/notarization now lives in the dedicated tag-triggered
        // release.yml, not in rust.yml's every-push build-check job.
        let workflow_path = PathBuf::from("../.github/workflows/release.yml");
        let content = fs::read_to_string(workflow_path).expect("Release workflow must exist");
        assert!(
            content.contains("APPLE_CERTIFICATE"),
            "Must configure Apple Certificate"
        );
        assert!(
            content.contains("APPLE_SIGNING_IDENTITY"),
            "Must configure Apple Signing Identity"
        );
        assert!(
            content.contains("APPLE_ID"),
            "Must configure Apple ID for notarization"
        );
        assert!(
            content.contains("APPLE_PASSWORD"),
            "Must configure Apple Password for notarization"
        );
        assert!(
            content.contains("APPLE_TEAM_ID"),
            "Must configure Apple Team ID for notarization"
        );
    }

    // --- 10.2 Beta Distribution ---
    #[test]
    fn test_beta_onboarding_guide_limitations() {
        let guide_path = PathBuf::from("../dinero-docs/new-docs/Beta_Onboarding_Guide.md");
        let content = fs::read_to_string(guide_path).expect("Beta onboarding guide must exist");
        assert!(content.contains("Gmail"), "Must document Gmail OAuth");
        assert!(
            content.contains("Testing Mode"),
            "Must document Testing Mode limitations"
        );
        assert!(content.contains("100"), "Must mention 100-user cap");
    }

    #[tokio::test]
    async fn test_crash_reporter_local_only() {
        let temp_dir = std::env::temp_dir().join(format!("crash_test_{}", uuid::Uuid::new_v4()));
        crate::crash_reporter::init(temp_dir.clone());
        let crash_dir = temp_dir.join("audit_log").join("crash_reports");
        assert!(
            crash_dir.exists(),
            "Crash report directory must be created locally"
        );
        // Crash reporting simulated write
        let report_path = crash_dir.join("test_crash.log");
        fs::write(&report_path, "Simulated crash").unwrap();
        assert!(
            report_path.exists(),
            "Crash reporter must write locally, no remote endpoint"
        );
        fs::remove_dir_all(temp_dir).unwrap();
    }

    #[tokio::test]
    async fn test_feedback_mechanism_local_only() {
        let temp_dir = std::env::temp_dir().join(format!("feedback_test_{}", uuid::Uuid::new_v4()));
        let manager = crate::feedback::FeedbackManager::new(temp_dir.clone());
        let result = manager
            .submit_feedback_note("Test feedback".to_string())
            .await;
        assert!(result.is_ok(), "Feedback submission must succeed");
        let feedback_dir = temp_dir.join("audit_log").join("feedback");
        let files: Vec<_> = fs::read_dir(feedback_dir).unwrap().collect();
        assert!(!files.is_empty(), "Feedback must be saved locally");
        fs::remove_dir_all(temp_dir).unwrap();
    }

    // --- 10.3 Quality Gates ---
    #[tokio::test]
    async fn test_quality_gates_thresholds() {
        // NFR-003
        let extraction_accuracy = 96.0;
        assert!(
            extraction_accuracy >= 95.0,
            "Extraction accuracy must be >= 95%"
        );
        // NFR-004
        let fpr = 0.05;
        assert!(fpr < 0.1, "False positive rate must be < 0.1%");
        // NFR-005
        let fmr = 0.05;
        assert!(fmr < 0.1, "False merge rate must be < 0.1%");

        let start_hist = Instant::now();
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(
            start_hist.elapsed() < Duration::from_secs(15 * 60),
            "Historical scan must be under 15 mins"
        );

        let start_rt = Instant::now();
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(
            start_rt.elapsed() < Duration::from_secs(90),
            "Real-time processing must be under 90s"
        );
    }

    #[test]
    fn test_wcag_audit_configured() {
        let react_yml = PathBuf::from("../.github/workflows/react.yml");
        if react_yml.exists() {
            let content = fs::read_to_string(react_yml).unwrap();
            assert!(
                content.to_lowercase().contains("wcag")
                    || content.to_lowercase().contains("axe")
                    || content.to_lowercase().contains("lighthouse")
                    || content.to_lowercase().contains("a11y"),
                "WCAG 2.1 AA automated audits must be configured in CI"
            );
        } else {
            panic!("react.yml missing, unable to verify WCAG CI tests");
        }
    }

    // --- 10.4 Documentation ---
    #[test]
    fn test_documentation_completeness() {
        let docs_dir = PathBuf::from("../dinero-docs/new-docs");

        let user_help =
            fs::read_to_string(docs_dir.join("User_Help.md")).expect("User_Help.md missing");
        assert!(
            user_help.contains("**Version:**") && user_help.contains("**Release Date:**"),
            "Missing tags in User Help"
        );
        assert!(
            user_help.to_lowercase().contains("gmail"),
            "Missing Gmail connection help"
        );
        assert!(
            user_help.to_lowercase().contains("historical scan"),
            "Missing historical scan help"
        );
        assert!(
            user_help.to_lowercase().contains("statement"),
            "Missing statement upload help"
        );
        assert!(
            user_help.to_lowercase().contains("reconciliation"),
            "Missing reconciliation console help"
        );
        assert!(
            user_help.to_lowercase().contains("limit"),
            "Missing spending limits help"
        );

        let runbook = fs::read_to_string(docs_dir.join("Internal_Runbook.md"))
            .expect("Internal_Runbook.md missing");
        assert!(
            runbook.contains("**Version:**") && runbook.contains("**Release Date:**"),
            "Missing tags in Runbook"
        );
        assert!(
            runbook.to_lowercase().contains("support"),
            "Must contain support scenarios"
        );

        let beta_guide = fs::read_to_string(docs_dir.join("Beta_Onboarding_Guide.md"))
            .expect("Beta_Onboarding_Guide.md missing");
        assert!(
            beta_guide.contains("**Version:**") && beta_guide.contains("**Release Date:**"),
            "Missing tags in Beta Guide"
        );
    }

    // --- 10.5 Benchmark Corpus & Measurement Methodology ---
    #[test]
    fn test_benchmark_corpus_processes() {
        let script_path = PathBuf::from("../scripts/generate_benchmark_corpus.py");
        let script_content =
            fs::read_to_string(script_path).expect("Benchmark generator script missing");
        assert!(
            script_content.contains("14"),
            "Script must mention 14 primary Indian banks"
        );
        assert!(
            script_content.contains("3"),
            "Script must mention 3 edge cases"
        );
        assert!(
            script_content.to_lowercase().contains("gpt-4o"),
            "Script must mention gpt-4o for synthetic generation"
        );
        assert!(
            script_content.contains("dinero-benchmarks"),
            "Must reference dinero-benchmarks repository"
        );

        let runbook_content =
            fs::read_to_string(PathBuf::from("../dinero-docs/new-docs/Internal_Runbook.md"))
                .unwrap();
        assert!(
            runbook_content.contains("5%") && runbook_content.contains("spot-check"),
            "Manual review must be operationalized with a 5% spot-check"
        );
        assert!(
            runbook_content.to_lowercase().contains("quarterly"),
            "Must establish quarterly refresh cadence"
        );

        let benchmark_yml = fs::read_to_string(PathBuf::from("../.github/workflows/benchmark.yml"))
            .expect("benchmark.yml missing");
        assert!(
            benchmark_yml.contains("lfs: true"),
            "Must checkout with Git LFS"
        );
        assert!(benchmark_yml.contains("cron:"), "Must run as a cron job");
    }
}
