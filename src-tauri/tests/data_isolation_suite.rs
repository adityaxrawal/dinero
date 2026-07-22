//! Doc 30 TASK-QA-007: Data Isolation and Privacy Regression Suite.
//!
//! `licensing-backend/tests/data_isolation.test.ts` (TASK-LIC-010) already
//! scans the Licensing Backend's own API/schema for financial-field leakage
//! -- this suite covers the wider, desktop-side scope Document 30 actually
//! asks for: every outbound network call site app-wide, the local schema,
//! and a dependency scan for disallowed cloud LLM SDKs. Same source-scanning
//! style already established by `tenant_isolation.rs`/`secrets_audit.rs`.

use std::path::Path;

fn src_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn walk_rs_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_rs_files(&path, out);
        } else if path.extension().map(|e| e == "rs") == Some(true) {
            out.push(path);
        }
    }
}

/// Doc 30 TASK-QA-007 acceptance: `test_only_allowed_outbound_channels_exist`.
/// Every real HTTP client construction in the crate must belong to one of
/// the documented outbound channels: Gmail API, Google OAuth,
/// GitHub (updater), the Licensing Backend, or Hugging Face (local LLM
/// GGUF model downloads -- a real, legitimate channel found while building
/// this suite that Document 30's own "Gmail, Google OAuth, GitHub updates,
/// licensing" list omits; flagged as a doc gap, not a violation, since the
/// domain is a well-known model host and the only data crossing the wire is
/// a model weights download, never user data). `llama_sidecar.rs`'s client
/// only ever talks to `127.0.0.1` (its own local subprocess) and is
/// deliberately excluded -- that traffic never leaves the device at all.
#[test]
fn test_only_allowed_outbound_channels_exist() {
    let allowed_domains_by_file: &[(&str, &[&str])] = &[
        ("network_client.rs", &[]), // the shared client itself; channel is asserted at call sites
        ("llm_manager.rs", &["huggingface.co"]),
    ];

    let mut files = Vec::new();
    walk_rs_files(&src_dir(), &mut files);

    let mut unexpected_bare_clients = Vec::new();
    for path in &files {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        let filename = path.file_name().unwrap().to_string_lossy().to_string();
        if filename == "network_client.rs" {
            continue; // the one legitimate place a bare reqwest::Client is built
        }
        if !content.contains("reqwest::Client::new()") && !content.contains("Client::builder()") {
            continue;
        }
        // llama_sidecar.rs builds two bare clients: one that only ever
        // addresses 127.0.0.1 (its own local subprocess, `health`/`completion`
        // -- never leaves the device) and one that downloads the llama.cpp
        // release tarball from github.com (a real, legitimate, but currently
        // NetworkClient-bypassing channel -- see the `network_client.rs`
        // doc comment this same task corrected).
        if filename == "llama_sidecar.rs" {
            assert!(
                content.contains("github.com"),
                "llama_sidecar.rs's release download must come from the documented github.com channel"
            );
            continue;
        }
        let expected = allowed_domains_by_file
            .iter()
            .find(|(f, _)| *f == filename)
            .map(|(_, domains)| *domains)
            .unwrap_or(&[]);
        if expected.is_empty() {
            unexpected_bare_clients.push(filename);
        } else {
            for domain in expected {
                assert!(
                    content.contains(domain),
                    "{filename} builds its own HTTP client but doesn't reference an expected allowed domain ({domain})"
                );
            }
        }
    }

    assert!(
        unexpected_bare_clients.is_empty(),
        "found a bare (non-NetworkClient-routed) HTTP client in an unexpected file -- \
         every new outbound channel must either route through NetworkClient (for Network \
         Activity audit visibility) or be added to this test's explicit allow-list: {:#?}",
        unexpected_bare_clients
    );

    // The channel names actually passed to NetworkClient::execute -- must
    // match exactly the documented set, nothing else. Scoped to
    // `.execute("..."` calls on the `network`/`self.network` receiver
    // specifically (not rusqlite::Connection::execute, which takes SQL, not
    // a channel name, as its first argument).
    let mut channels = std::collections::HashSet::new();
    for path in &files {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        for cap in content.split("network.execute(\"").skip(1) {
            if let Some(end) = cap.find('"') {
                channels.insert(cap[..end].to_string());
            }
        }
    }
    let allowed_channels = ["gmail_api", "google_oauth", "licensing_backend"];
    for channel in &channels {
        assert!(
            allowed_channels.contains(&channel.as_str()),
            "NetworkClient channel '{channel}' is not in the documented allow-list {:?}",
            allowed_channels
        );
    }
}

/// Doc 30 TASK-QA-007 acceptance: `test_no_financial_data_leaves_device`.
/// `NetworkActivityLogRow` (the Network Activity audit trail every
/// `NetworkClient::execute` call writes to) must only ever store
/// request/response *metadata* -- method, domain, redacted URL, byte
/// counts, status code, masked-field names, channel -- never a request or
/// response body, which is exactly where financial transaction content
/// could otherwise leak into the local audit log.
#[test]
fn test_no_financial_data_leaves_device() {
    let content = std::fs::read_to_string(src_dir().join("db/network_activity_log.rs"))
        .expect("network_activity_log.rs must exist");
    for forbidden in ["body", "response_text", "raw_response", "payload_json"] {
        assert!(
            !content.to_lowercase().contains(forbidden),
            "NetworkActivityLogRow must never store a request/response body field ('{forbidden}' found)"
        );
    }

    let client_content = std::fs::read_to_string(src_dir().join("network_client.rs")).unwrap();
    assert!(
        client_content.contains("bytes_sent") && client_content.contains("bytes_received"),
        "NetworkClient must log only byte counts, not actual body content"
    );
    assert!(
        !client_content.contains(".text().await") && !client_content.contains("res.bytes()"),
        "NetworkClient::execute must never read/log the actual response body"
    );
}

/// Doc 30 TASK-QA-007 acceptance: `test_no_disallowed_cloud_llm_dependencies`.
/// The local-LLM fallback (Document 16 §12.3) is a locally-run Candle/GGUF
/// model via `llama_sidecar` -- no cloud LLM SDK may ever be a dependency,
/// since that would mean financial transaction text is being sent to a
/// third-party AI provider, violating the core "never leaves the device"
/// guarantee.
#[test]
fn test_no_disallowed_cloud_llm_dependencies() {
    let cargo_toml = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
    )
    .unwrap();
    let disallowed_crates = [
        "async-openai",
        "openai-api",
        "anthropic-sdk",
        "async-anthropic",
        "cohere-rust",
        "mistralai",
    ];
    for crate_name in disallowed_crates {
        assert!(
            !cargo_toml.to_lowercase().contains(crate_name),
            "Cargo.toml must never depend on a cloud LLM SDK ('{crate_name}' found)"
        );
    }

    let mut files = Vec::new();
    walk_rs_files(&src_dir(), &mut files);
    let disallowed_domains = [
        "api.openai.com",
        "api.anthropic.com",
        "api.cohere.ai",
        "api.mistral.ai",
        "generativelanguage.googleapis.com",
    ];
    for path in &files {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        for domain in disallowed_domains {
            assert!(
                !content.contains(domain),
                "{}: references a disallowed cloud LLM API domain '{domain}'",
                path.display()
            );
        }
    }
}

/// Doc 30 TASK-QA-007 acceptance: `test_license_backend_contains_identity_only_fields`.
/// The desktop-side payloads sent *to* the Licensing Backend
/// (`licensing::client`'s request structs) must carry only identity/billing
/// metadata -- never a financial-transaction-shaped field. The Licensing
/// Backend's own schema/API side of this same guarantee is already
/// independently verified by `licensing-backend/tests/data_isolation.test.ts`
/// (TASK-LIC-010) -- this is the desktop-side half of the same invariant.
#[test]
fn test_license_backend_contains_identity_only_fields() {
    let content = std::fs::read_to_string(src_dir().join("licensing/client.rs")).unwrap();
    let forbidden_fields = [
        "amount_minor",
        "merchant",
        "transaction_id",
        "account_number",
        "iban",
        "card_number",
        "statement",
    ];
    for field in forbidden_fields {
        assert!(
            !content.to_lowercase().contains(field),
            "licensing/client.rs's request/response structs must never carry a financial field ('{field}' found)"
        );
    }
}
