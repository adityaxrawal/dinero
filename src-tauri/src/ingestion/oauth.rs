use anyhow::{Context, Result};
use deadpool_sqlite::Pool;
use keyring::Entry;
use oauth2::reqwest::async_http_client;
use oauth2::{
    basic::BasicClient, AuthUrl, AuthorizationCode, ClientId, CsrfToken,
    PkceCodeChallenge, RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};
use tiny_http::{Response, Server};
use url::Url;

use crate::db::connected_accounts;

const GOOGLE_CLIENT_ID: &str = match option_env!("GOOGLE_CLIENT_ID") {
    Some(val) => val,
    None => "",
};
// Doc 21 §2.1/22 §6.2, Doc 24 §3 (H6 fix): matches the `com.dinero.app`
// service name already used by db::crypto and statements::password —
// this was still the pre-rebrand "finance-tracker" name.
const KEYCHAIN_SERVICE: &str = "com.dinero.app";
/// Pre-rebrand service name — `get_token` migrates any entry still stored
/// under it to `KEYCHAIN_SERVICE` on first read, rather than orphaning a
/// user's existing Gmail connection when this constant changed.
const LEGACY_KEYCHAIN_SERVICE: &str = "finance-tracker";
const KEYCHAIN_ACCOUNT_PREFIX: &str = "gmail-tokens";

/// Doc 03 §8.2, Doc 40 §11: a license supports up to 10 connected Gmail
/// accounts (2nd-10th gated to `ACTIVE` — enforced in `start_oauth_flow_async`).
const MAX_CONNECTED_GMAIL_ACCOUNTS: i64 = 10;

/// Doc 24 §2/§8: secrets live only in Keychain — never plaintext files, env
/// vars, or config, under any build configuration. There is no debug-mode
/// fallback to a plaintext temp file.
///
/// Doc 22 §5.4/§6.2: each connected Gmail account gets its own isolated
/// Keychain entry, keyed by `account_id` — a single shared entry would have
/// each new account's token silently overwrite the previous account's,
/// making genuine multi-account support (Doc 03 §8.2) impossible in practice.
fn keychain_account_name(account_id: &str) -> String {
    format!("{}-{}", KEYCHAIN_ACCOUNT_PREFIX, account_id)
}

fn save_token(account_id: &str, token_store: &TokenStore) -> Result<()> {
    let entry = Entry::new(KEYCHAIN_SERVICE, &keychain_account_name(account_id))
        .context("Keyring error")?;
    entry.set_password(&serde_json::to_string(token_store)?)?;
    Ok(())
}

fn get_token(account_id: &str) -> Result<String> {
    let entry = Entry::new(KEYCHAIN_SERVICE, &keychain_account_name(account_id))
        .map_err(|e| anyhow::anyhow!("Keyring entry error: {}", e))?;
    if let Ok(token) = entry.get_password() {
        return Ok(token);
    }

    // H6 fix: fall back to the pre-rebrand service name and migrate the
    // entry forward so it isn't silently orphaned by the rename above.
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

fn delete_token(account_id: &str) {
    if let Ok(entry) = Entry::new(KEYCHAIN_SERVICE, &keychain_account_name(account_id)) {
        let _ = entry.delete_credential();
    }
    // Best-effort cleanup of a not-yet-migrated legacy entry (H6 fix).
    if let Ok(legacy_entry) = Entry::new(LEGACY_KEYCHAIN_SERVICE, &keychain_account_name(account_id)) {
        let _ = legacy_entry.delete_credential();
    }
}

/// Doc 28 §4.4 step 2 ("Reset App Data" full wipe): revokes every connected
/// Gmail account's OAuth token server-side via Google's revoke endpoint — not
/// just deleting the local Keychain copies (`auth_google_disconnect`'s per-account
/// behavior) — then clears each Keychain entry regardless of whether the
/// network call succeeded, per the doc's "Local Wipe Priority" rule (§4.4:
/// local wipe proceeds even if a remote call fails or the device is offline).
/// Iterates every row in `connected_accounts`, not just one, since Doc 03
/// §8.2 allows up to 10 simultaneously-connected accounts.
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

    // Doc 01 §10.4 (BG-02): routed through NetworkClient so this revocation
    // call is captured in the local Network Activity audit trail.
    let network = crate::network_client::NetworkClient::new(pool.clone());

    for account in &accounts {
        if let Ok(token_json) = get_token(&account.id) {
            if let Ok(token_store) = serde_json::from_str::<TokenStore>(&token_json) {
                let builder = network
                    .client()
                    .post("https://oauth2.googleapis.com/revoke")
                    .form(&[("token", token_store.access_token.as_str())]);
                let revoke_result = network.execute(builder).await;
                match revoke_result {
                    Ok(res) if res.status().is_success() => {
                        tracing::info!("Gmail OAuth token revoked with Google for account {}", account.id);
                    }
                    Ok(res) => {
                        tracing::warn!(
                            "Gmail OAuth token revocation for account {} returned non-success status {} — \
                             proceeding with local wipe regardless (Local Wipe Priority, Doc 28 §4.4)",
                            account.id, res.status()
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Gmail OAuth token revocation request failed for account {}: {} — proceeding with \
                             local wipe regardless (Local Wipe Priority, Doc 28 §4.4)",
                            account.id, e
                        );
                    }
                }
            }
        }
        delete_token(&account.id);
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenStore {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: u64,
}

/// H9 fix: `redirect_port` is the OS-assigned ephemeral port the local
/// callback server is actually listening on (RFC 8252 §7.3 — loopback OAuth
/// redirect URIs may use any port), not a hardcoded value that could already
/// be in use by another process. Refresh-token exchanges (`get_valid_access_token`)
/// also go through this function, but `redirect_uri` is never sent as part of
/// a `refresh_token` grant (RFC 6749 §6) — the port passed there is inert.
pub fn get_oauth_client(redirect_port: u16) -> Result<BasicClient> {
    // Doc 22 §5.1, Doc 26 §8.2 (I10 fix): PKCE is the whole point of a public/
    // native-app OAuth client — no client secret should be embedded or sent.
    Ok(BasicClient::new(
        ClientId::new(GOOGLE_CLIENT_ID.to_string()),
        None,
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

/// Doc 30 TASK-AUTH-001: the loopback listener times out after 5 minutes if
/// the user closes the browser without completing consent.
const OAUTH_CALLBACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// Blocks (synchronously — call from `spawn_blocking`) waiting for the OAuth
/// redirect on `server`, bounded by `timeout`. `recv_timeout` bounds the wait
/// itself, so the calling thread terminates on its own once the deadline
/// passes rather than being left blocked indefinitely in `incoming_requests()`
/// with no way to unblock it from the async side. Returns `Err` with the
/// message `"oauth_timeout"` on timeout — matching `AppError::Auth`'s
/// expected variant once the caller maps it at the IPC boundary.
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
                    return match validate_oauth_callback(&url, expected_state) {
                        Ok(c) => {
                            let response = Response::from_string(
                                "Authentication successful! You may close this tab.",
                            );
                            let _ = request.respond(response);
                            Ok(c)
                        }
                        Err(e) => {
                            let response = Response::from_string(e.to_string());
                            let _ = request.respond(response.with_status_code(400));
                            Err(e)
                        }
                    };
                }
                // Not the callback request (e.g. a favicon probe) — keep
                // waiting for the real one within the remaining budget.
            }
            Ok(None) => anyhow::bail!("oauth_timeout"),
            Err(e) => anyhow::bail!("OAuth callback server error: {}", e),
        }
    }
}

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
pub async fn is_gmail_connected(pool: tauri::State<'_, Pool>) -> Result<bool, String> {
    let conn = pool.get().await.map_err(|e| e.to_string())?;
    let is_connected = conn
        .interact(|c| {
            let mut stmt = c
                .prepare("SELECT COUNT(*) FROM connected_accounts WHERE account_status = 'ACTIVE'")
                .unwrap();
            let count: i64 = stmt.query_row([], |row| row.get(0)).unwrap_or(0);
            count > 0
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(is_connected)
}

#[derive(serde::Serialize)]
pub struct ConnectedAccountInfo {
    pub email: String,
    pub account_id: String,
}

/// Doc 03 §8.2: a license supports up to 10 *simultaneously* connected Gmail
/// accounts — the frontend needs the full list, not just one, to render a
/// multi-account management UI.
#[tauri::command]
pub async fn list_connected_accounts(
    pool: tauri::State<'_, Pool>,
) -> Result<Vec<ConnectedAccountInfo>, String> {
    let conn = pool.get().await.map_err(|e| e.to_string())?;
    conn.interact(|c| {
        let mut stmt = c
            .prepare("SELECT id, email_address FROM connected_accounts WHERE account_status = 'ACTIVE' ORDER BY created_at ASC")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                let account_id: String = row.get(0)?;
                let email: Option<String> = row.get(1)?;
                Ok(ConnectedAccountInfo {
                    email: email.unwrap_or_else(|| "Unknown".to_string()),
                    account_id,
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
    .map_err(|e| e.to_string())?
}

/// G20/H10/J8 fix: renamed from `disconnect_gmail` to match Doc 19 §5.3's
/// documented `auth_google_disconnect` naming.
#[tauri::command]
pub async fn auth_google_disconnect(
    account_id: String,
    pool: tauri::State<'_, Pool>,
) -> Result<String, String> {
    crate::licensing::gate::assert_write_allowed(pool.inner())
        .await
        .map_err(|e| e.to_string())?;

    // 1. Delete this account's token from keychain (per-account entry, Doc 22 §5.4/§6.2).
    delete_token(&account_id);

    // 2. Mark this specific account as inactive — not a blanket update, since
    // Doc 03 §8.2 allows multiple simultaneously-connected accounts and
    // disconnecting one must not affect the others.
    let conn = pool.get().await.map_err(|e| e.to_string())?;
    let acc_id_clone = account_id.clone();
    conn.interact(move |c| {
        let _ = c.execute(
            "UPDATE connected_accounts SET account_status = 'INACTIVE', email_address = NULL WHERE id = ?1",
            params![acc_id_clone],
        );

        let _ = crate::db::audit_log::insert(
            c,
            &crate::db::audit_log::AuditLogRow {
                id: uuid::Uuid::new_v4().to_string(),
                actor_type: Some("user".to_string()),
                actor_id: Some("local".to_string()),
                action: Some("disconnect_gmail".to_string()),
                resource_type: Some("connected_account".to_string()),
                resource_id: Some(acc_id_clone.clone()),
                before_json: None,
                after_json: Some(serde_json::json!({
                    "status": "disconnected"
                })),
                created_at: chrono::Utc::now(),
            },
        );
    }).await.map_err(|e| e.to_string())?;

    Ok("Disconnected".to_string())
}

/// Doc 03 §8.2, Doc 40 §11: enforced entirely locally — the connected-account
/// count and the cached, signature-verified license state are both already
/// known on-device, so this never needs a Licensing Backend round-trip.
/// A fresh install with no `license_state` row yet (first-ever account) is
/// always allowed, matching "at least one connected Gmail account at every
/// license state" — this only restricts going *beyond* the first account.
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
                .map(|s| s.subscription_status_cached == crate::licensing::state::LicenseStatus::Active)
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

pub async fn start_oauth_flow_async(
    _app: AppHandle,
    pool: Pool,
    profile_id: i64,
) -> Result<String> {
    assert_new_gmail_account_allowed(&pool).await?;

    // H9 fix: bind the loopback callback server to an OS-assigned ephemeral
    // port (port 0) rather than a hardcoded 3456 — a fixed port could already
    // be in use by another process (or a stale previous run), failing the
    // whole connect flow. RFC 8252 §7.3 permits any port for a loopback
    // redirect URI, so the OAuth client is built against whatever port the
    // server actually got.
    let server = Server::http("127.0.0.1:0")
        .map_err(|e| anyhow::anyhow!("Failed to start local OAuth callback server: {}", e))?;
    let redirect_port = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| anyhow::anyhow!("Local OAuth callback server has no IP address"))?
        .port();

    let client = get_oauth_client(redirect_port)?;
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

    // Doc 01 §8.1 C-05, Doc 22 §5.2: "Gmail scope must remain gmail.readonly
    // only... any broader scope requires re-evaluation." No openid/email/
    // profile scope is ever requested — the account's email address is
    // obtained post-token via Gmail API's own users.getProfile endpoint
    // (see GmailClient::get_profile), which gmail.readonly alone grants.
    let (auth_url, csrf_token) = client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new(
            "https://www.googleapis.com/auth/gmail.readonly".to_string(),
        ))
        .set_pkce_challenge(pkce_challenge)
        .url();

    let url_string = auth_url.to_string();
    tracing::info!("Opening browser for OAuth: {}", url_string);
    tracing::info!("Listening on http://127.0.0.1:{} for OAuth callback...", redirect_port);

    if let Err(e) = tauri_plugin_opener::open_url(&url_string, None::<&str>) {
        tracing::error!("Failed to open browser: {}", e);
    }

    let expected_state = csrf_token.secret().clone();

    // Doc 30 TASK-AUTH-001: if the user closes the browser without completing
    // consent, the loopback listener must not wait forever.
    let code = tokio::task::spawn_blocking(move || {
        wait_for_oauth_callback(&server, &expected_state, OAUTH_CALLBACK_TIMEOUT)
    })
    .await??;

    // Exchange code for token
    let token_result = client
        .exchange_code(AuthorizationCode::new(code))
        .set_pkce_verifier(pkce_verifier)
        .request_async(async_http_client)
        .await?;

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

    // Fetch the account's email via Gmail API's own users.getProfile — not
    // Google's separate userinfo endpoint, which would require scopes this
    // app must never request (Doc 01 §8.1 C-05, see the scope comment
    // above). GmailClient already routes through NetworkClient (Doc 01
    // §10.4, BG-02), so this call is captured in the Network Activity log.
    let gmail_client = crate::ingestion::gmail_client::GmailClient::new(
        access_token.clone(),
        pool.clone(),
    );
    let profile = gmail_client
        .get_profile()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch Gmail profile: {}", e))?;

    let email_address = profile.email_address;
    // No stable Google account ID is available under gmail.readonly alone
    // (users.getProfile doesn't return one) — a Gmail address uniquely and
    // stably identifies a Gmail account, so a deterministic hash of the
    // (lowercased) email becomes the account_id instead. Hashed rather than
    // used verbatim so the identifier that ends up in checkpoints, audit
    // logs, and job keys never carries the raw email address.
    let account_uuid = uuid::Uuid::new_v5(
        &uuid::Uuid::from_bytes([
            0xa1, 0x1a, 0xcc, 0x00, 0x9e, 0x3c, 0x4f, 0x6e, 0x8b, 0x1d, 0x2c, 0x3d, 0x4e, 0x5f,
            0x60, 0x71,
        ]),
        email_address.to_lowercase().as_bytes(),
    );
    let account_id = format!("gmail_{}", account_uuid);

    save_token(&account_id, &token_store)?;

    // Update connected_accounts
    let conn = pool.get().await?;
    let account = connected_accounts::ConnectedAccountsRow {
        id: account_id.clone(),
        profile_id,
        email_address: Some(email_address.clone()),
        account_status: Some("ACTIVE".to_string()),
        last_history_id: None,
        created_at: None,
        updated_at: None,
    };

    let acc_id_for_check = account_id.clone();
    let email_for_update = email_address;
    conn.interact(move |c| {
        let result = if let Ok(Some(mut existing_acc)) = connected_accounts::get_account(c, &acc_id_for_check) {
            // Always update both status AND email on reconnect
            existing_acc.account_status = Some("ACTIVE".to_string());
            existing_acc.email_address = Some(email_for_update);
            connected_accounts::update_account(c, &existing_acc)
        } else {
            connected_accounts::insert_account(c, &account)
        };

        // Doc 25 §4.2/§4.4: every consent event is recorded locally with a UTC
        // timestamp — this is the moment the user has just authorized Gmail
        // access. Best-effort: a logging hiccup should not fail the OAuth flow
        // the user is actively waiting on.
        if result.is_ok() {
            if let Err(e) = crate::db::audit_log::record_consent_event(
                c,
                "gmail_access",
                "User consents to Gmail access",
            ) {
                tracing::warn!("Failed to record Gmail access consent event: {}", e);
            }
        }

        result
    })
    .await
    .map_err(|e| anyhow::anyhow!("DB interaction error: {}", e))?
    .map_err(|e| anyhow::anyhow!("Failed to save account: {}", e))?;

    Ok("Authentication successful".to_string())
}

/// I8 fix: whether a degradation `reason` indicates the stored credential
/// itself is unusable/invalid (so the Keychain entry should be purged to
/// force a clean re-auth) versus a plausibly-transient failure (a network
/// blip shouldn't nuke an otherwise-good refresh token). `keychain_read_failed`,
/// `token_parse_failed`, and `no_refresh_token` are always unusable; a refresh
/// request failure is only treated as invalid when the OAuth server itself
/// rejected the grant (`invalid_grant`/`invalid_client`/`unauthorized`) rather
/// than e.g. a network timeout.
fn reason_indicates_invalid_token(reason: &str) -> bool {
    reason == "keychain_read_failed"
        || reason == "token_parse_failed"
        || reason == "no_refresh_token"
        || reason.contains("invalid_grant")
        || reason.contains("invalid_client")
        || reason.contains("unauthorized")
}

async fn mark_account_degraded_async<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &Pool,
    account_id: &str,
    reason: &str,
) {
    let should_purge = reason_indicates_invalid_token(reason);
    if should_purge {
        // I8 fix: revoked/invalid tokens were previously marked degraded but
        // left sitting in Keychain indefinitely.
        delete_token(account_id);
        tracing::info!(
            "Purged Keychain token for account_id='{}' (reason='{}')",
            account_id,
            reason
        );
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
                // J6 fix: token-refresh-failure lifecycle event (Doc 25 §6.1).
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
                        after_json: Some(serde_json::json!({ "reason": reason, "token_purged": should_purge })),
                        created_at: chrono::Utc::now(),
                    },
                ) {
                    tracing::warn!("Failed to record gmail_token_refresh_failed audit event: {}", e);
                }
            })
            .await;
    }
    let _ = app.emit("system_warning", "gmail_token_degraded");
}

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

    let mut token_store: TokenStore = match serde_json::from_str(&token_json) {
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

    let refresh_token = match &token_store.refresh_token {
        Some(rt) => rt.clone(),
        None => {
            tracing::error!("No refresh token available");
            mark_account_degraded_async(app, pool, account_id, "no_refresh_token").await;
            return Err(anyhow::anyhow!("No refresh token available"));
        }
    };

    // The refresh_token grant never sends redirect_uri (RFC 6749 §6), so the
    // port here is inert — this call opens no local server and no browser.
    let client = get_oauth_client(0)?;
    match client
        .exchange_refresh_token(&RefreshToken::new(refresh_token))
        .request_async(async_http_client)
        .await
    {
        Ok(token_result) => {
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
        Err(e) => {
            tracing::error!("Failed to refresh token: {}", e);
            mark_account_degraded_async(app, pool, account_id, &format!("refresh_request_failed: {e}")).await;
            Err(anyhow::anyhow!("Refresh token failed: {}", e))
        }
    }
}

pub async fn handle_invalid_history_id(pool: &Pool, account_id: &str) -> Result<()> {
    tracing::warn!("history checkpoint reset: Invalid history id encountered, falling back to full historical scan");
    let conn = pool.get().await?;
    let acc_id = account_id.to_string();
    conn.interact(move |c| {
        if let Ok(Some(mut account)) = connected_accounts::get_account(c, &acc_id) {
            account.last_history_id = None;
            let _ = connected_accounts::update_account(c, &account);
        }
    })
    .await
    .map_err(|e| anyhow::anyhow!("DB interaction error: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oauth_callback_payload_validation() {
        // Valid payload
        let valid_url = "/?state=expected_state_123&code=auth_code_xyz";
        let res = validate_oauth_callback(valid_url, "expected_state_123");
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), "auth_code_xyz");

        // Invalid state
        let invalid_state_url = "/?state=hacker_state&code=auth_code_xyz";
        let res = validate_oauth_callback(invalid_state_url, "expected_state_123");
        assert!(res.is_err());
        assert_eq!(
            res.unwrap_err().to_string(),
            "Invalid state parameter. Forged token injection prevented."
        );

        // Missing code
        let missing_code_url = "/?state=expected_state_123";
        let res = validate_oauth_callback(missing_code_url, "expected_state_123");
        assert!(res.is_err());
        assert_eq!(
            res.unwrap_err().to_string(),
            "Missing code or state parameter."
        );

        // Missing state
        let missing_state_url = "/?code=auth_code_xyz";
        let res = validate_oauth_callback(missing_state_url, "expected_state_123");
        assert!(res.is_err());
    }

    #[test]
    fn test_token_stored_in_keychain_not_sqlite() {
        // We verify that ConnectedAccountsRow doesn't have token fields
        let account = connected_accounts::ConnectedAccountsRow {
            id: "test".to_string(),
            profile_id: 1,
            email_address: Some("test@gmail.com".to_string()),
            account_status: Some("ACTIVE".to_string()),
            last_history_id: None,
            created_at: None,
            updated_at: None,
        };
        // This is a static compilation check that `account` has no `access_token` field
        assert_eq!(account.id, "test");
    }

    /// Doc 30 TASK-AUTH-001: "If the user closes the browser without
    /// completing consent, the loopback listener times out ... with
    /// `AppError::Auth("oauth_timeout")`." Uses a short injected timeout
    /// (rather than the real 5 minutes) so the test itself stays fast; no
    /// request is ever sent to the server, simulating an abandoned browser.
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
