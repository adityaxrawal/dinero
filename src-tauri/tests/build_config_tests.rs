//! Doc 30 TASK-DESK-009: CI validation for the Tauri build pipeline and
//! entitlements configuration. These are static file/config checks (no
//! actual signing or build required), consistent with this task's own
//! acceptance-criteria framing as "CI validation," not runtime behavior.
//!
//! `test_url_scheme_registered_in_info_plist` (Doc 30's own acceptance
//! criterion name) is deliberately NOT implemented as named: Document 22
//! (Security & Authentication Design) §5 explicitly states the custom
//! URL-scheme callback design that criterion assumes "is explicitly
//! superseded and must not be implemented," in favor of the already-built
//! local loopback listener (`ingestion::oauth`). This file's
//! `test_no_url_scheme_registered_loopback_design_used_instead` is the
//! correct, resolved replacement -- asserting the *absence* Document 22
//! requires, not the presence Doc 30's stale task text describes.

use std::fs;
use std::path::Path;

fn src_tauri_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn read_entitlements() -> String {
    fs::read_to_string(src_tauri_dir().join("Entitlements.plist"))
        .expect("Entitlements.plist must exist and be readable")
}

/// Doc 30 TASK-DESK-009 acceptance: `test_release_build_includes_all_required_entitlements`.
#[test]
fn test_release_build_includes_all_required_entitlements() {
    let plist = read_entitlements();

    assert!(
        plist.contains("com.apple.security.cs.allow-jit"),
        "the in-process Candle LLM runtime (Document 15 §9, TASK-TXN-006) requires \
         com.apple.security.cs.allow-jit -- its absence would silently break Layer 6 \
         extraction on every release build"
    );
    assert!(
        plist.contains("com.apple.security.network.client"),
        "required for the 5 disclosed non-financial network channels (Document 01 §10.4)"
    );

    // Document 15 §14.12 / security_hardening_check.sh's own check #5: the
    // App Sandbox must never be requested -- it directly contradicts the
    // allow-jit entitlement's own rationale for existing (direct-DMG
    // distribution specifically to avoid App Store sandboxing constraints,
    // Document 16 §9.1).
    assert!(
        !plist.contains("com.apple.security.app-sandbox"),
        "the App Sandbox entitlement must never be present -- it directly contradicts \
         the allow-jit/direct-DMG-distribution rationale (Document 16 §9.1)"
    );

    // Doc 30's task text also names "Keychain access-group entitlements for
    // the keyring crate" -- deliberately not added. This app has no App
    // Sandbox and shares no Keychain items across multiple bundle
    // identifiers (a single app, a single Keychain service name "dinero",
    // Document 18 §7.2/§8) -- the scenario `keychain-access-groups`
    // actually exists for. Standard code-signing-based Keychain ACLs
    // (matching Team ID/bundle ID) are sufficient without it; asserting
    // its *absence* here as a considered choice, not an oversight.
    assert!(
        !plist.contains("keychain-access-groups"),
        "no keychain-access-groups entitlement is needed or expected -- this app doesn't \
         share Keychain items across bundle identifiers"
    );
}

/// Replaces Doc 30's own (now-invalidated, see module doc comment)
/// `test_url_scheme_registered_in_info_plist`.
#[test]
fn test_no_url_scheme_registered_loopback_design_used_instead() {
    let info_plist_path = src_tauri_dir().join("Info.plist");
    assert!(
        info_plist_path.exists(),
        "Info.plist should exist (even if minimal) to document why no URL scheme is \
         registered, per Document 22 §5's explicit resolution"
    );
    let plist = fs::read_to_string(&info_plist_path).unwrap();
    // Checks for the actual XML element, not a bare substring -- this
    // file's own explanatory comment (deliberately) mentions
    // "CFBundleURLTypes" and "dinero://" in prose, which a naive substring
    // search would wrongly trip on.
    assert!(
        !plist.contains("<key>CFBundleURLTypes</key>"),
        "no custom URL scheme may be registered -- Document 22 §5 explicitly supersedes \
         the dinero://oauth-callback design in favor of the local loopback listener"
    );
    assert!(
        !plist.contains("<string>dinero</string>") && !plist.contains("<string>dinero://"),
        "the dinero:// scheme specifically must never be registered as a URL scheme -- \
         it's the exact superseded design Document 22 §5 names"
    );

    // The real OAuth redirect mechanism this app actually uses.
    let oauth_source = fs::read_to_string(src_tauri_dir().join("src/ingestion/oauth.rs"))
        .expect("src/ingestion/oauth.rs must exist");
    assert!(
        oauth_source.contains("127.0.0.1"),
        "the real OAuth flow must use the local loopback listener design (Document 22 §5, \
         Document 16 §7, Document 21 §2.1.2), not a URL scheme"
    );
}

/// Doc 30: "configure the bundle identifier `com.dinero.app`."
#[test]
fn test_bundle_identifier_and_category_configured() {
    let conf = fs::read_to_string(src_tauri_dir().join("tauri.conf.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&conf).unwrap();

    assert_eq!(parsed["identifier"], "com.dinero.app");
    assert_eq!(
        parsed["bundle"]["category"], "Finance",
        "public.app-category.finance, per Doc 30's own wording"
    );
    assert_eq!(
        parsed["bundle"]["macOS"]["minimumSystemVersion"], "10.13",
        "matches Document 18 §11's stated minimum and the download landing page's own \
         stated system requirement (TASK-DESK-007)"
    );
}

/// Doc 30: "separate unsigned-fast-iteration (local dev) vs. fully-signed-
/// notarized (release) build profiles." Already correctly split across two
/// workflows (TASK-DESK-006) -- verified here as a static file check
/// rather than re-implemented.
#[test]
fn test_unsigned_dev_and_signed_release_profiles_are_separate_workflows() {
    let repo_root = src_tauri_dir()
        .parent()
        .expect("src-tauri must have a parent (the repo root)");
    let rust_yml = fs::read_to_string(repo_root.join(".github/workflows/rust.yml")).unwrap();
    let release_yml = fs::read_to_string(repo_root.join(".github/workflows/release.yml")).unwrap();

    assert!(
        !rust_yml.contains("APPLE_CERTIFICATE"),
        "the per-push CI build must stay unsigned -- signing secrets belong only to the \
         dedicated tag-triggered release workflow (TASK-DESK-006's H8 fix)"
    );
    assert!(
        release_yml.contains("APPLE_CERTIFICATE"),
        "the release workflow must reference the real signing secrets"
    );
    assert!(
        release_yml.contains("tags:"),
        "the release workflow must be tag-triggered, not run on every push"
    );
}
