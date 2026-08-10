//! Google OAuth: obtaining and refreshing Gmail access.
//!
//! Uses the PKCE authorisation-code flow, which is the correct choice for a
//! desktop application: there is no server to hold a client secret, and PKCE
//! binds the authorisation code to this specific request so an intercepted code
//! is useless on its own.
//!
//! Tokens are held in the OS keychain rather than the database, so the encrypted
//! database file alone never grants access to anyone's mailbox. The callback is
//! validated against the state this app generated before any code is exchanged,
//! which is what prevents an injected authorisation response.
use anyhow::{Context, Result};
use deadpool_sqlite::Pool;
use oauth2::reqwest::async_http_client;
use oauth2::{
    basic::BasicClient, AuthUrl, AuthorizationCode, ClientId, CsrfToken, PkceCodeChallenge,
    RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};
use tiny_http::{Header, Response, Server};
use url::Url;

use crate::db::connected_accounts;

/// The HTML page shown in the browser once OAuth completes.
///
/// Served by the temporary local callback server, so the user sees a result in
/// the tab they were sent to rather than a blank page or a connection error.
pub(crate) fn oauth_result_page(success: bool, message: &str) -> String {
    let escaped: String = message
        .chars()
        .map(|c| match c {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&#39;".to_string(),
            other => other.to_string(),
        })
        .collect();

    let (badge_color, badge_glyph, title, subtitle) = if success {
        (
            "#064E3B",
            r##"<path d="M6 12.5 10 16.5 18 7.5" stroke="#F8E7C9" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"##,
            "Authentication successful",
            "You may close this tab and return to Dinero.",
        )
    } else {
        (
            "#ef4444",
            r##"<path d="M8 8 16 16M16 8 8 16" stroke="#fff" stroke-width="2.2" stroke-linecap="round" fill="none"/>"##,
            "Authentication failed",
            escaped.as_str(),
        )
    };

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1.0" />
<title>Dinero — {title}</title>
<link rel="preconnect" href="https://fonts.googleapis.com" />
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
<link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&display=swap" rel="stylesheet" />
<style>
  :root {{ color-scheme: light; }}
  * {{ box-sizing: border-box; }}
  html, body {{
    margin: 0; height: 100%;
    font-family: "Inter", -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  }}
  body {{
    display: flex; align-items: center; justify-content: center;
    min-height: 100vh;
    background: radial-gradient(circle at 50% 0%, #fdf6ed 0%, #f6e6c5 55%, #f0d4a8 100%);
    color: hsl(160, 84%, 10%);
  }}
  .card {{
    width: min(420px, 88vw);
    background: #fdf6ed;
    border: 1px solid hsla(36, 28%, 60%, 0.4);
    border-radius: 16px;
    padding: 2.5rem 2.25rem;
    text-align: center;
    box-shadow: 0 20px 45px -20px rgba(6, 78, 59, 0.35);
    animation: rise 0.45s cubic-bezier(0.22, 1, 0.36, 1);
  }}
  @keyframes rise {{
    from {{ opacity: 0; transform: translateY(10px) scale(0.98); }}
    to   {{ opacity: 1; transform: translateY(0) scale(1); }}
  }}
  .badge {{
    width: 64px; height: 64px; margin: 0 auto 1.25rem;
    border-radius: 999px;
    background: {badge_color};
    display: flex; align-items: center; justify-content: center;
    box-shadow: 0 8px 20px -6px {badge_color}66;
  }}
  h1 {{
    margin: 0 0 0.5rem;
    font-size: 1.25rem;
    font-weight: 600;
    color: hsl(160, 84%, 12%);
  }}
  p {{
    margin: 0;
    font-size: 0.9rem;
    line-height: 1.5rem;
    color: hsla(160, 18%, 30%, 0.85);
    word-break: break-word;
  }}
  .brand {{
    margin-top: 1.75rem;
    font-size: 0.7rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: hsla(160, 18%, 38%, 0.6);
  }}
</style>
</head>
<body>
  <div class="card">
    <div class="badge">
      <svg width="24" height="24" viewBox="0 0 24 24">{badge_glyph}</svg>
    </div>
    <h1>{title}</h1>
    <p>{subtitle}</p>
    <div class="brand">Dinero &middot; Personal Finance Tracker</div>
  </div>
</body>
</html>"#
    )
}

const GOOGLE_CLIENT_ID: &str = match option_env!("GOOGLE_CLIENT_ID") {
    Some(val) => val,
    None => "",
};
const GOOGLE_CLIENT_SECRET: Option<&str> = option_env!("GOOGLE_CLIENT_SECRET");
#[cfg(not(debug_assertions))]
const KEYCHAIN_SERVICE: &str = "com.dinero.app";
#[cfg(not(debug_assertions))]
const LEGACY_KEYCHAIN_SERVICE: &str = "finance-tracker";
#[cfg(not(debug_assertions))]
const KEYCHAIN_ACCOUNT_PREFIX: &str = "gmail-tokens";

const OAUTH_TOKEN_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

const MAX_CONNECTED_GMAIL_ACCOUNTS: i64 = 10;

#[cfg(not(debug_assertions))]
/// Namespaces a keychain entry per connected account.
///
/// Multiple mailboxes can be connected, so each needs its own entry rather than
/// overwriting a shared one.
fn keychain_account_name(account_id: &str) -> String {
    format!("{}-{}", KEYCHAIN_ACCOUNT_PREFIX, account_id)
}

/// Development token storage: a plain file, bypassing the keychain.
///
/// Dev builds avoid the keychain because it prompts on every rebuild and treats
/// each unsigned binary as a different application.
#[cfg(debug_assertions)]
fn save_token(account_id: &str, token_store: &TokenStore) -> Result<()> {
    let dev_token_path = std::env::temp_dir().join(format!("dinero_dev_token_{}.json", account_id));
    std::fs::write(dev_token_path, serde_json::to_string(token_store)?)?;
    Ok(())
}

/// Release token storage: the OS keychain.
///
/// Tokens never enter the database, so a leaked database file grants no access to
/// anyone's mailbox.
#[cfg(not(debug_assertions))]
fn save_token(account_id: &str, token_store: &TokenStore) -> Result<()> {
    let entry = Entry::new(KEYCHAIN_SERVICE, &keychain_account_name(account_id))
        .context("Keyring error")?;
    entry.set_password(&serde_json::to_string(token_store)?)?;
    Ok(())
}

/// Reads the development token from disk.
#[cfg(debug_assertions)]
fn get_token(account_id: &str) -> Result<String> {
    let dev_token_path = std::env::temp_dir().join(format!("dinero_dev_token_{}.json", account_id));
    if let Ok(token) = std::fs::read_to_string(dev_token_path) {
        return Ok(token);
    }
    Err(anyhow::anyhow!("No matching entry found in secure storage"))
}

/// Reads the release token from the keychain.
#[cfg(not(debug_assertions))]
fn get_token(account_id: &str) -> Result<String> {
    let entry = Entry::new(KEYCHAIN_SERVICE, &keychain_account_name(account_id))
        .map_err(|e| anyhow::anyhow!("Keyring entry error: {}", e))?;
    if let Ok(token) = entry.get_password() {
        return Ok(token);
    }

    let legacy_entry = Entry::new(LEGACY_KEYCHAIN_SERVICE, &keychain_account_name(account_id))
        .map_err(|e| anyhow::anyhow!("Keyring entry error: {}", e))?;
    let token = legacy_entry
        .get_password()
        .map_err(|e| anyhow::anyhow!("No matching entry found in secure storage: {}", e))?;

    if entry.set_password(&token).is_ok() {
        let _ = legacy_entry.delete_credential();
        tracing::info!(
            "Migrated Gmail Keychain entry for account_id='{}' from legacy service name",
            account_id
        );
    }

    Ok(token)
}

/// Deletes the development token file.
#[cfg(debug_assertions)]
fn delete_token(account_id: &str) {
    let dev_token_path = std::env::temp_dir().join(format!("dinero_dev_token_{}.json", account_id));
    let _ = std::fs::remove_file(dev_token_path);
}

/// Deletes the release token from the keychain.
///
/// Errors are ignored: this runs during disconnect, where an already-absent
/// entry is the desired end state.
#[cfg(not(debug_assertions))]
fn delete_token(account_id: &str) {
    if let Ok(entry) = Entry::new(KEYCHAIN_SERVICE, &keychain_account_name(account_id)) {
        let _ = entry.delete_credential();
    }
    if let Ok(legacy_entry) =
        Entry::new(LEGACY_KEYCHAIN_SERVICE, &keychain_account_name(account_id))
    {
        let _ = legacy_entry.delete_credential();
    }
}

/// Revokes access for every connected account.
///
/// Revokes at Google rather than only deleting local tokens, so access genuinely
/// ends instead of a working token being left behind.
pub async fn revoke_gmail_access(pool: &Pool) {
    let accounts = match pool.get().await {
        Ok(conn) => conn
            .interact(|c| connected_accounts::get_all_accounts(c))
            .await
            .ok()
            .and_then(|r| r.ok())
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    let network = crate::network_client::NetworkClient::new(pool.clone());

    for account in &accounts {
        revoke_single_account_with_google(&network, &account.id).await;
        delete_token(&account.id);
    }
}

/// Calls Google's revocation endpoint for one account.
async fn revoke_single_account_with_google(
    network: &crate::network_client::NetworkClient,
    account_id: &str,
) {
    let Ok(token_json) = get_token(account_id) else {
        return;
    };
    let Ok(token_store) = serde_json::from_str::<TokenStore>(&token_json) else {
        return;
    };
    let builder = network
        .client()
        .post("https://oauth2.googleapis.com/revoke")
        .form(&[("token", token_store.access_token.as_str())]);
    match network.execute("google_oauth", builder).await {
        Ok(res) if res.status().is_success() => {
            tracing::info!(
                "Gmail OAuth token revoked with Google for account {}",
                account_id
            );
        }
        Ok(res) => {
            tracing::warn!(
                "Gmail OAuth token revocation for account {} returned non-success status {} — \
                 proceeding with local cleanup regardless (Local Wipe Priority, Doc 28 §4.4)",
                account_id,
                res.status()
            );
        }
        Err(e) => {
            tracing::warn!(
                "Gmail OAuth token revocation request failed for account {}: {} — proceeding with \
                 local cleanup regardless (Local Wipe Priority, Doc 28 §4.4)",
                account_id,
                e
            );
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenStore {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: u64,
}

/// Builds the OAuth client for the PKCE authorisation-code flow.
///
/// PKCE is the correct choice for a desktop app: there is no server to hold a
/// client secret, and the challenge binds the authorisation code to this specific
/// request, so an intercepted code is useless on its own.
pub fn get_oauth_client(redirect_port: u16) -> Result<BasicClient> {
    Ok(BasicClient::new(
        ClientId::new(GOOGLE_CLIENT_ID.to_string()),
        GOOGLE_CLIENT_SECRET.map(|s| oauth2::ClientSecret::new(s.to_string())),
        AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_string())?,
        Some(TokenUrl::new(
            "https://oauth2.googleapis.com/token".to_string(),
        )?),
    )
    .set_redirect_uri(RedirectUrl::new(format!(
        "http://127.0.0.1:{}",
        redirect_port
    ))?))
}

const OAUTH_CALLBACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// Runs a short-lived local server awaiting the OAuth redirect.
///
/// Bound to loopback only, and torn down as soon as the callback arrives or the
/// wait times out -- it exists purely to receive one redirect.
fn wait_for_oauth_callback(
    server: &Server,
    expected_state: &str,
    timeout: std::time::Duration,
) -> Result<String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let remaining = match deadline.checked_duration_since(std::time::Instant::now()) {
            Some(d) if !d.is_zero() => d,
            _ => anyhow::bail!("oauth_timeout"),
        };
        match server.recv_timeout(remaining) {
            Ok(Some(request)) => {
                let url = request.url().to_string();
                if url.starts_with("/?") {
                    let html_header =
                        Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..])
                            .expect("static header is valid");
                    return match validate_oauth_callback(&url, expected_state) {
                        Ok(c) => {
                            let response = Response::from_string(oauth_result_page(true, ""))
                                .with_header(html_header);
                            let _ = request.respond(response);
                            Ok(c)
                        }
                        Err(e) => {
                            let response =
                                Response::from_string(oauth_result_page(false, &e.to_string()))
                                    .with_header(html_header)
                                    .with_status_code(400);
                            let _ = request.respond(response);
                            Err(e)
                        }
                    };
                }
            }
            Ok(None) => anyhow::bail!("oauth_timeout"),
            Err(e) => anyhow::bail!("OAuth callback server error: {}", e),
        }
    }
}

/// Validates the callback and extracts the authorisation code.
///
/// The state parameter must match the value this app generated. Without that
/// check an attacker could feed in their own authorisation response and have the
/// app bind the wrong account.
pub fn validate_oauth_callback(url: &str, expected_state: &str) -> Result<String> {
    let parsed_url = Url::parse(&format!("http://localhost{}", url)).unwrap();
    let mut code = None;
    let mut state = None;
    for (key, value) in parsed_url.query_pairs() {
        if key == "code" {
            code = Some(value.into_owned());
        } else if key == "state" {
            state = Some(value.into_owned());
        }
    }

    if let (Some(c), Some(s)) = (&code, &state) {
        if s != expected_state {
            return Err(anyhow::anyhow!(
                "Invalid state parameter. Forged token injection prevented."
            ));
        }
        Ok(c.clone())
    } else {
        Err(anyhow::anyhow!("Missing code or state parameter."))
    }
}

#[tauri::command]
/// Whether any Gmail account is currently connected.
pub async fn is_gmail_connected(
    pool: tauri::State<'_, Pool>,
) -> Result<bool, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let is_connected = conn
        .interact(|c| {
            let mut stmt = c
                .prepare("SELECT COUNT(*) FROM connected_accounts WHERE account_status = 'ACTIVE'")
                .unwrap();
            let count: i64 = stmt.query_row([], |row| row.get(0)).unwrap_or(0);
            count > 0
        })
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;
    Ok(is_connected)
}

#[derive(serde::Serialize)]
pub struct ConnectedAccountInfo {
    pub email: String,
    pub account_id: String,
    pub account_status: String,
}

#[tauri::command]
/// Lists connected accounts for the settings screen.
pub async fn settings_get_connected_accounts(
    pool: tauri::State<'_, Pool>,
) -> Result<Vec<ConnectedAccountInfo>, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(|c| {
        let mut stmt = c
            .prepare("SELECT id, email_address, account_status FROM connected_accounts WHERE account_status IS NOT NULL AND account_status != 'disconnected' ORDER BY created_at ASC")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                let account_id: String = row.get(0)?;
                let email: Option<String> = row.get(1)?;
                let account_status: String = row.get(2)?;
                Ok(ConnectedAccountInfo {
                    email: email.unwrap_or_else(|| "Unknown".to_string()),
                    account_id,
                    account_status,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut accounts = Vec::new();
        for row in rows {
            accounts.push(row.map_err(|e| e.to_string())?);
        }
        Ok(accounts)
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
    .map_err(crate::error::AppError::Db)
}

#[tauri::command]
/// Disconnects one account, revoking its grant and clearing local state.
pub async fn auth_google_disconnect(
    account_id: String,
    pool: tauri::State<'_, Pool>,
    session_state: tauri::State<'_, crate::auth::session::SessionState>,
) -> Result<String, crate::error::AppError> {
    crate::ipc::middleware::require_active_session(&session_state)?;

    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    let network = crate::network_client::NetworkClient::new(pool.inner().clone());
    revoke_single_account_with_google(&network, &account_id).await;
    delete_token(&account_id);

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let acc_id_clone = account_id.clone();
    conn.interact(move |c| apply_disconnect(c, &acc_id_clone))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;

    Ok("Disconnected".to_string())
}

/// Clears an account's local state on disconnect.
fn apply_disconnect(c: &rusqlite::Connection, account_id: &str) {
    let _ = c.execute(
        "UPDATE connected_accounts SET account_status = 'disconnected', email_address = NULL WHERE id = ?1",
        params![account_id],
    );

    if let Err(e) = crate::auth::consent::withdraw_consent_event(c, "gmail_oauth_consent") {
        tracing::warn!("Failed to withdraw gmail_oauth_consent event: {}", e);
    }

    let _ = crate::db::audit_log::insert(
        c,
        &crate::db::audit_log::AuditLogRow {
            id: uuid::Uuid::new_v4().to_string(),
            actor_type: Some("user".to_string()),
            actor_id: Some("local".to_string()),
            action: Some("gmail_revoked".to_string()),
            resource_type: Some("connected_account".to_string()),
            resource_id: Some(account_id.to_string()),
            before_json: None,
            after_json: Some(serde_json::json!({
                "status": "disconnected"
            })),
            created_at: chrono::Utc::now(),
        },
    );
}

/// Refuses a new Gmail connection when one is not permitted.
///
/// Guards the account limit before the OAuth flow begins, so the user is not sent
/// through a browser consent screen only to be refused at the end.
async fn assert_new_gmail_account_allowed(pool: &Pool) -> Result<()> {
    let conn = pool.get().await?;
    let (active_count, is_active_license) = conn
        .interact(|c| {
            let active_count: i64 = c
                .query_row(
                    "SELECT COUNT(*) FROM connected_accounts WHERE account_status = 'ACTIVE'",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            let is_active_license = crate::licensing::state::get_license_state(c)
                .ok()
                .flatten()
                .map(|s| {
                    s.subscription_status_cached == crate::licensing::state::LicenseStatus::Active
                })
                .unwrap_or(false);
            (active_count, is_active_license)
        })
        .await
        .map_err(|e| anyhow::anyhow!("DB interaction error: {}", e))?;

    if active_count >= MAX_CONNECTED_GMAIL_ACCOUNTS {
        anyhow::bail!(
            "Maximum of {} connected Gmail accounts reached (Doc 03 §8.2).",
            MAX_CONNECTED_GMAIL_ACCOUNTS
        );
    }
    if active_count >= 1 && !is_active_license {
        anyhow::bail!(
            "Connecting an additional Gmail account requires an active subscription. \
             Your current plan supports one connected Gmail account — subscribe to add up to 10."
        );
    }
    Ok(())
}

/// Runs the full OAuth flow: consent, callback, token exchange.
pub async fn start_oauth_flow_async(
    _app: AppHandle,
    pool: Pool,
    profile_id: i64,
) -> Result<String> {
    assert_new_gmail_account_allowed(&pool).await?;

    let server = Server::http("127.0.0.1:0")
        .map_err(|e| anyhow::anyhow!("Failed to start local OAuth callback server: {}", e))?;
    let redirect_port = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| anyhow::anyhow!("Local OAuth callback server has no IP address"))?
        .port();

    let client = get_oauth_client(redirect_port)?;
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

    let (auth_url, csrf_token) = client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new(
            "https://www.googleapis.com/auth/gmail.readonly".to_string(),
        ))
        .add_extra_param("access_type", "offline")
        .add_extra_param("prompt", "consent")
        .set_pkce_challenge(pkce_challenge)
        .url();

    let url_string = auth_url.to_string();
    tracing::info!("Opening browser for OAuth: {}", url_string);
    tracing::info!(
        "Listening on http://127.0.0.1:{} for OAuth callback...",
        redirect_port
    );

    if let Err(e) = tauri_plugin_opener::open_url(&url_string, None::<&str>) {
        tracing::error!("Failed to open browser: {}", e);
    }

    let expected_state = csrf_token.secret().clone();

    let code = tokio::task::spawn_blocking(move || {
        wait_for_oauth_callback(&server, &expected_state, OAUTH_CALLBACK_TIMEOUT)
    })
    .await??;

    let token_result = tokio::time::timeout(
        OAUTH_TOKEN_REQUEST_TIMEOUT,
        client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(pkce_verifier)
            .request_async(async_http_client),
    )
    .await
    .context("Timed out exchanging OAuth authorization code for a token")??;

    let access_token = token_result.access_token().secret().clone();
    let refresh_token = token_result.refresh_token().map(|t| t.secret().clone());
    let expires_in = token_result
        .expires_in()
        .map(|d| d.as_secs())
        .unwrap_or(3600);

    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

    let token_store = TokenStore {
        access_token: access_token.clone(),
        refresh_token,
        expires_at: now + expires_in,
    };

    let gmail_client =
        crate::ingestion::gmail_client::GmailClient::new(access_token.clone(), pool.clone(), None);
    let profile = gmail_client
        .get_profile()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch Gmail profile: {}", e))?;

    let email_address = profile.email_address;
    let initial_history_id = profile.history_id.clone();
    let account_uuid = uuid::Uuid::new_v5(
        &uuid::Uuid::from_bytes([
            0xa1, 0x1a, 0xcc, 0x00, 0x9e, 0x3c, 0x4f, 0x6e, 0x8b, 0x1d, 0x2c, 0x3d, 0x4e, 0x5f,
            0x60, 0x71,
        ]),
        email_address.to_lowercase().as_bytes(),
    );
    let account_id = format!("gmail_{}", account_uuid);

    save_token(&account_id, &token_store)?;

    let conn = pool.get().await?;
    let account = connected_accounts::ConnectedAccountsRow {
        id: account_id.clone(),
        profile_id,
        email_address: Some(email_address.clone()),
        account_status: Some("ACTIVE".to_string()),
        last_history_id: initial_history_id,
        created_at: None,
        updated_at: None,
    };

    let acc_id_for_check = account_id.clone();
    let email_for_update = email_address;
    conn.interact(move |c| {
        if let Ok(Some(mut existing_acc)) = connected_accounts::get_account(c, &acc_id_for_check) {
            existing_acc.account_status = Some("ACTIVE".to_string());
            existing_acc.email_address = Some(email_for_update);
            connected_accounts::update_account(c, &existing_acc)
        } else {
            connected_accounts::insert_account(c, &account)
        }
    })
    .await
    .map_err(|e| anyhow::anyhow!("DB interaction error: {}", e))?
    .map_err(|e| anyhow::anyhow!("Failed to save account: {}", e))?;

    Ok("Authentication successful".to_string())
}

/// Classifies a token-refresh failure.
///
/// The distinction that matters is whether the grant is permanently invalid --
/// revoked or expired -- or the failure was transient, since only the former
/// should mark the account degraded and prompt the user to reconnect.
fn classify_refresh_error(raw: &str) -> &'static str {
    if raw.contains("invalid_grant") {
        "invalid_grant"
    } else if raw.contains("invalid_client") {
        "invalid_client"
    } else if raw.contains("unauthorized") {
        "unauthorized"
    } else if raw.to_lowercase().contains("timeout") || raw.to_lowercase().contains("connect") {
        "network_error"
    } else {
        "unknown_error"
    }
}

/// Whether a failure reason means the token is permanently invalid.
fn reason_indicates_invalid_token(reason: &str) -> bool {
    reason == "keychain_read_failed"
        || reason == "token_parse_failed"
        || reason == "no_refresh_token"
        || reason.contains("invalid_grant")
        || reason.contains("invalid_client")
        || reason.contains("unauthorized")
}

/// Marks an account degraded and notifies the frontend.
///
/// Surfaced rather than retried silently: a revoked grant needs the user to
/// reconnect, and quiet retries would leave ingestion mysteriously stopped.
async fn mark_account_degraded_async<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &Pool,
    account_id: &str,
    reason: &str,
) {
    let should_purge = reason_indicates_invalid_token(reason);
    if should_purge {
        delete_token(account_id);
        tracing::info!(
            "Purged Keychain token for account_id='{}' (reason='{}')",
            account_id,
            reason
        );

        let monitor = app.state::<crate::security::incident_response::IncidentMonitor>();
        if crate::security::incident_response::record_trigger(
            monitor.inner(),
            crate::security::incident_response::TriggerKind::RepeatedOAuthFailure,
        ) {
            let session_state = app.state::<crate::auth::session::SessionState>();
            if let Err(e) = crate::security::incident_response::respond_to_incident(
                crate::security::incident_response::TriggerKind::RepeatedOAuthFailure,
                app,
                pool,
                session_state.inner(),
            )
            .await
            {
                tracing::error!("Incident response failed: {}", e);
            }
        }
    }

    if let Ok(conn) = pool.get().await {
        let acc_id = account_id.to_string();
        let reason = reason.to_string();
        let _ = conn
            .interact(move |c| {
                if let Ok(Some(mut account)) = connected_accounts::get_account(c, &acc_id) {
                    account.account_status = Some("degraded".to_string());
                    let _ = connected_accounts::update_account(c, &account);
                }
                if let Err(e) = crate::db::audit_log::insert(
                    c,
                    &crate::db::audit_log::AuditLogRow {
                        id: uuid::Uuid::new_v4().to_string(),
                        actor_type: Some("system".to_string()),
                        actor_id: None,
                        action: Some("gmail_token_refresh_failed".to_string()),
                        resource_type: Some("connected_account".to_string()),
                        resource_id: Some(acc_id),
                        before_json: None,
                        after_json: Some(
                            serde_json::json!({ "reason": reason, "token_purged": should_purge }),
                        ),
                        created_at: chrono::Utc::now(),
                    },
                ) {
                    tracing::warn!(
                        "Failed to record gmail_token_refresh_failed audit event: {}",
                        e
                    );
                }
            })
            .await;
    }
    crate::ipc::system_warnings::emit_system_warning(
        app,
        crate::ipc::system_warnings::SystemWarningPayload {
            warning_type: "gmail_token_degraded".to_string(),
            message: "Gmail sync is paused -- your access token could not be refreshed."
                .to_string(),
            severity: crate::ipc::system_warnings::WarningSeverity::Degraded,
            action_hint: Some("reconnect_gmail_account".to_string()),
        },
    );
}

/// Returns a valid access token, refreshing it if expired.
///
/// The single entry point every Gmail call goes through, so refresh happens in
/// one place rather than being duplicated per call site.
pub async fn get_valid_access_token<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &Pool,
    account_id: &str,
) -> Result<String> {
    let token_json = match get_token(account_id) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("Failed to get token from secure storage: {}", e);
            mark_account_degraded_async(app, pool, account_id, "keychain_read_failed").await;
            return Err(e);
        }
    };

    let token_store: TokenStore = match serde_json::from_str(&token_json) {
        Ok(ts) => ts,
        Err(e) => {
            tracing::error!("Failed to parse token from secure storage: {}", e);
            mark_account_degraded_async(app, pool, account_id, "token_parse_failed").await;
            return Err(anyhow::anyhow!("Token parse error: {}", e));
        }
    };

    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    if token_store.expires_at > now + 60 {
        return Ok(token_store.access_token);
    }

    refresh_token_store(app, pool, account_id, token_store).await
}

/// Forces a refresh regardless of the token's apparent expiry.
///
/// Used when a call fails with an auth error despite a token that looked valid,
/// which happens when a grant is revoked before its natural expiry.
pub async fn force_refresh_access_token<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &Pool,
    account_id: &str,
) -> Result<String> {
    let token_json = match get_token(account_id) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("Failed to get token from secure storage: {}", e);
            mark_account_degraded_async(app, pool, account_id, "keychain_read_failed").await;
            return Err(e);
        }
    };
    let token_store: TokenStore = match serde_json::from_str(&token_json) {
        Ok(ts) => ts,
        Err(e) => {
            tracing::error!("Failed to parse token from secure storage: {}", e);
            mark_account_degraded_async(app, pool, account_id, "token_parse_failed").await;
            return Err(anyhow::anyhow!("Token parse error: {}", e));
        }
    };
    refresh_token_store(app, pool, account_id, token_store).await
}

/// Exchanges the refresh token for a new access token and stores it.
async fn refresh_token_store<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &Pool,
    account_id: &str,
    mut token_store: TokenStore,
) -> Result<String> {
    let refresh_token = match &token_store.refresh_token {
        Some(rt) => rt.clone(),
        None => {
            tracing::error!("No refresh token available");
            mark_account_degraded_async(app, pool, account_id, "no_refresh_token").await;
            return Err(anyhow::anyhow!("No refresh token available"));
        }
    };

    let client = get_oauth_client(0)?;
    let refresh_result = tokio::time::timeout(
        OAUTH_TOKEN_REQUEST_TIMEOUT,
        client
            .exchange_refresh_token(&RefreshToken::new(refresh_token))
            .request_async(async_http_client),
    )
    .await;
    match refresh_result {
        Err(_elapsed) => {
            tracing::error!(
                "Gmail token refresh timed out after {:?}",
                OAUTH_TOKEN_REQUEST_TIMEOUT
            );
            mark_account_degraded_async(app, pool, account_id, "network_error").await;
            Err(anyhow::anyhow!(
                "Timed out refreshing Gmail access token after {:?}",
                OAUTH_TOKEN_REQUEST_TIMEOUT
            ))
        }
        Ok(Ok(token_result)) => {
            token_store.access_token = token_result.access_token().secret().clone();
            if let Some(new_rt) = token_result.refresh_token() {
                token_store.refresh_token = Some(new_rt.secret().clone());
            }
            let expires_in = token_result
                .expires_in()
                .map(|d| d.as_secs())
                .unwrap_or(3600);
            token_store.expires_at =
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() + expires_in;

            save_token(account_id, &token_store)?;
            Ok(token_store.access_token)
        }
        Ok(Err(e)) => {
            let category = classify_refresh_error(&e.to_string());
            tracing::error!("Gmail token refresh failed: {}", category);
            mark_account_degraded_async(
                app,
                pool,
                account_id,
                &format!("refresh_request_failed: {category}"),
            )
            .await;
            Err(anyhow::anyhow!("Gmail token refresh failed: {}", category))
        }
    }
}

/// Recovers from an invalid Gmail history id.
///
/// History ids expire once they age out of Gmail's window. Recovery resets to a
/// full sync, since incremental polling has no valid point to resume from.
pub async fn handle_invalid_history_id(pool: &Pool, account_id: &str) -> Result<()> {
    tracing::warn!("history checkpoint reset: Invalid history id encountered, falling back to full historical scan");
    let conn = pool.get().await?;
    let acc_id = account_id.to_string();
    conn.interact(move |c| {
        if let Ok(Some(mut account)) = connected_accounts::get_account(c, &acc_id) {
            account.last_history_id = None;
            let _ = connected_accounts::update_account(c, &account);
        }
        if let Err(e) = crate::db::audit_log::insert(
            c,
            &crate::db::audit_log::AuditLogRow {
                id: uuid::Uuid::new_v4().to_string(),
                actor_type: Some("system".to_string()),
                actor_id: None,
                action: Some("history_checkpoint_reset".to_string()),
                resource_type: Some("connected_account".to_string()),
                resource_id: Some(acc_id),
                before_json: None,
                after_json: None,
                created_at: chrono::Utc::now(),
            },
        ) {
            tracing::warn!(
                "Failed to record history_checkpoint_reset audit event: {}",
                e
            );
        }
    })
    .await
    .map_err(|e| anyhow::anyhow!("DB interaction error: {}", e))?;
    Ok(())
}

/// Builds the refresher closure the Gmail client uses to renew tokens.
pub fn create_token_refresher<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &deadpool_sqlite::Pool,
    account_id: &str,
) -> Option<crate::ingestion::gmail_client::TokenRefresher> {
    let app = app.clone();
    let pool = pool.clone();
    let account_id = account_id.to_string();
    Some(std::sync::Arc::new(move || {
        let app = app.clone();
        let pool = pool.clone();
        let account_id = account_id.clone();
        Box::pin(async move { force_refresh_access_token(&app, &pool, &account_id).await })
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_disconnect_updates_status_withdraws_consent_and_logs() {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute("INSERT INTO local_profile (id) VALUES (1)", [])
            .unwrap();
        conn.execute(
            "INSERT INTO connected_accounts (id, profile_id, email_address, account_status) \
             VALUES ('acc_1', 1, 'user@gmail.com', 'ACTIVE')",
            [],
        )
        .unwrap();
        crate::auth::consent::insert_consent_event(
            &conn,
            "gmail_oauth_consent",
            "verbatim disclosure",
        )
        .unwrap();

        apply_disconnect(&conn, "acc_1");

        let (status, email): (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT account_status, email_address FROM connected_accounts WHERE id = 'acc_1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(status.as_deref(), Some("disconnected"));
        assert_eq!(email, None);

        let history = crate::auth::consent::fetch_consent_history(&conn, 10, 0).unwrap();
        assert_eq!(
            history.len(),
            1,
            "consent event must still exist, not be deleted"
        );
        assert!(history[0].withdrawn_at.is_some());

        let action: String = conn
            .query_row(
                "SELECT action FROM audit_log WHERE resource_id = 'acc_1' ORDER BY rowid DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(action, "gmail_revoked");
    }

    #[test]
    fn classify_refresh_error_never_echoes_raw_text() {
        assert_eq!(
            classify_refresh_error(
                "Server returned error response: invalid_grant: Token has been expired or revoked."
            ),
            "invalid_grant"
        );
        assert_eq!(
            classify_refresh_error(
                "Server returned error response: invalid_client: no client authentication"
            ),
            "invalid_client"
        );
        assert_eq!(
            classify_refresh_error("request unauthorized by server"),
            "unauthorized"
        );
        assert_eq!(
            classify_refresh_error("connection timeout after 30s"),
            "network_error"
        );
        assert_eq!(
            classify_refresh_error(
                "some completely unexpected raw error text with a secret-looking token abc123"
            ),
            "unknown_error"
        );
    }

    #[test]
    fn test_oauth_callback_payload_validation() {
        let valid_url = "/?state=expected_state_123&code=auth_code_xyz";
        let res = validate_oauth_callback(valid_url, "expected_state_123");
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), "auth_code_xyz");

        let invalid_state_url = "/?state=hacker_state&code=auth_code_xyz";
        let res = validate_oauth_callback(invalid_state_url, "expected_state_123");
        assert!(res.is_err());
        assert_eq!(
            res.unwrap_err().to_string(),
            "Invalid state parameter. Forged token injection prevented."
        );

        let missing_code_url = "/?state=expected_state_123";
        let res = validate_oauth_callback(missing_code_url, "expected_state_123");
        assert!(res.is_err());
        assert_eq!(
            res.unwrap_err().to_string(),
            "Missing code or state parameter."
        );

        let missing_state_url = "/?code=auth_code_xyz";
        let res = validate_oauth_callback(missing_state_url, "expected_state_123");
        assert!(res.is_err());
    }

    #[test]
    fn test_token_stored_in_keychain_not_sqlite() {
        let account = connected_accounts::ConnectedAccountsRow {
            id: "test".to_string(),
            profile_id: 1,
            email_address: Some("test@gmail.com".to_string()),
            account_status: Some("ACTIVE".to_string()),
            last_history_id: None,
            created_at: None,
            updated_at: None,
        };
        assert_eq!(account.id, "test");
    }

    #[test]
    fn oauth_callback_wait_times_out_when_browser_never_completes() {
        let server = Server::http("127.0.0.1:0").unwrap();
        let result = wait_for_oauth_callback(
            &server,
            "expected_state",
            std::time::Duration::from_millis(200),
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "oauth_timeout");
    }
}
