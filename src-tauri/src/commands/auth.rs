use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use keyring::Entry;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Manager};
use url::Url;
use uuid::Uuid;

const GOOGLE_AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_USERINFO_URL: &str = "https://openidconnect.googleapis.com/v1/userinfo";
const KEYRING_SERVICE: &str = "com.pranavkundaikar.productiveemail.gmail-oauth";
const OAUTH_CALLBACK_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Clone)]
pub enum GmailAuth {
    AppPassword(String),
    OAuthAccessToken(String),
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AuthKind {
    #[default]
    AppPassword,
    GoogleOAuth,
}

#[derive(Serialize, Deserialize)]
struct CredentialsStore {
    accounts: Vec<StoredAccount>,
}

#[derive(Serialize, Deserialize)]
struct StoredAccount {
    /// Stable local identity used by the database. Older credential stores did
    /// not have this field, so it is populated during startup migration.
    #[serde(default)]
    id: String,
    email: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    auth_kind: AuthKind,
    #[serde(default)]
    profile_picture: Option<String>,
}

#[derive(Clone)]
struct CachedAccessToken {
    value: String,
    // OAuth expiry is wall-clock time. Unlike a process-relative timer, this
    // remains meaningful after a laptop sleeps for several hours.
    expires_at: SystemTime,
}

static ACCESS_TOKEN_CACHE: OnceLock<Mutex<HashMap<String, CachedAccessToken>>> = OnceLock::new();
// The initial sync, IDLE listener, and a body fetch can all start together.
// Serialize cache misses so they do not each prompt Keychain for the same
// refresh token before the first access token is cached.
static ACCESS_TOKEN_REFRESH_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

#[derive(Deserialize)]
struct GoogleTokenResponse {
    access_token: String,
    expires_in: u64,
    refresh_token: Option<String>,
}

#[derive(Deserialize)]
struct GoogleUserInfo {
    email: String,
    #[serde(default)]
    email_verified: bool,
    picture: Option<String>,
}

#[derive(Serialize)]
pub struct AccountProfile {
    email: String,
    profile_picture: Option<String>,
}

struct GoogleOAuthClient {
    id: String,
    secret: String,
}

fn google_oauth_client() -> Result<GoogleOAuthClient, String> {
    // The ignored .env file is only for local development. Release builds
    // receive these compile-time values from GitHub Actions secrets instead.
    let _ = dotenvy::dotenv();
    let value = |name: &str, compiled: Option<&str>| {
        compiled
            .map(str::to_owned)
            .or_else(|| env::var(name).ok())
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| format!("{name} is not configured"))
    };
    Ok(GoogleOAuthClient {
        id: value(
            "GOOGLE_OAUTH_CLIENT_ID",
            option_env!("GOOGLE_OAUTH_CLIENT_ID"),
        )?,
        secret: value(
            "GOOGLE_OAUTH_CLIENT_SECRET",
            option_env!("GOOGLE_OAUTH_CLIENT_SECRET"),
        )?,
    })
}

#[derive(Deserialize)]
struct GoogleErrorResponse {
    error: Option<String>,
    error_description: Option<String>,
}

async fn google_error(response: reqwest::Response, context: &str) -> String {
    let status = response.status();
    let details = response
        .json::<GoogleErrorResponse>()
        .await
        .ok()
        .and_then(|body| body.error_description.or(body.error));
    match details {
        Some(details) => format!("{context}: {details}"),
        None => format!("{context}: Google returned HTTP {status}"),
    }
}

fn creds_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("credentials.json"))
}

fn load_store(app: &AppHandle) -> Result<CredentialsStore, String> {
    let path = creds_path(app)?;
    if !path.exists() {
        return Ok(CredentialsStore { accounts: vec![] });
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

fn save_store(app: &AppHandle, store: &CredentialsStore) -> Result<(), String> {
    let path = creds_path(app)?;
    let json = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    fs::write(&path, &json).map_err(|e| e.to_string())?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    Ok(())
}

fn refresh_token_entry(email: &str) -> Result<Entry, String> {
    Entry::new(KEYRING_SERVICE, email).map_err(|e| e.to_string())
}

fn read_refresh_token(email: &str) -> Result<String, String> {
    refresh_token_entry(email)?
        .get_password()
        .map_err(|_| "Your Google sign-in has expired. Please sign in again.".to_string())
}

fn save_refresh_token(email: &str, refresh_token: &str) -> Result<(), String> {
    refresh_token_entry(email)?
        .set_password(refresh_token)
        .map_err(|e| format!("Could not save the Google sign-in securely: {e}"))
}

fn delete_refresh_token(email: &str) -> Result<(), String> {
    match refresh_token_entry(email)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

pub async fn get_gmail_auth(app: &AppHandle, email: &str) -> Result<GmailAuth, String> {
    let account = load_store(app)?
        .accounts
        .into_iter()
        .find(|account| account.email == email || account.id == email)
        .ok_or_else(|| format!("No credentials found for {email}"))?;

    match account.auth_kind {
        AuthKind::AppPassword => Ok(GmailAuth::AppPassword(account.password)),
        AuthKind::GoogleOAuth => Ok(GmailAuth::OAuthAccessToken(access_token_for(&account.email).await?)),
    }
}

/// Adds stable IDs to credential stores created before account contexts were
/// introduced. This is intentionally idempotent and does not touch secrets.
pub fn ensure_account_ids(app: &AppHandle) -> Result<(), String> {
    let mut store = load_store(app)?;
    let mut changed = false;
    for account in &mut store.accounts {
        if Uuid::parse_str(&account.id).is_err() {
            account.id = Uuid::new_v4().to_string();
            changed = true;
        }
    }
    if changed {
        save_store(app, &store)?;
    }
    Ok(())
}

pub fn account_id_for_email(app: &AppHandle, email: &str) -> Result<String, String> {
    load_store(app)?
        .accounts
        .into_iter()
        .find(|account| account.email == email)
        .map(|account| account.id)
        .ok_or_else(|| format!("No account found for {email}"))
}

pub fn account_email_for_id(app: &AppHandle, account_id: &str) -> Result<String, String> {
    load_store(app)?
        .accounts
        .into_iter()
        .find(|account| account.id == account_id || account.email == account_id)
        .map(|account| account.email)
        .ok_or_else(|| format!("No account found for {account_id}"))
}

async fn access_token_for(email: &str) -> Result<String, String> {
    let google_client = google_oauth_client()?;
    let cache = ACCESS_TOKEN_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(cached) = cache
        .lock()
        .map_err(|_| "OAuth token cache is unavailable".to_string())?
        .get(email)
    {
        if cached.expires_at > SystemTime::now() + Duration::from_secs(60) {
            return Ok(cached.value.clone());
        }
    }

    let _refresh_guard = ACCESS_TOKEN_REFRESH_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    // A concurrent caller may have refreshed the token while this caller was
    // waiting for the lock, so always check the cache again.
    if let Some(cached) = cache
        .lock()
        .map_err(|_| "OAuth token cache is unavailable".to_string())?
        .get(email)
    {
        if cached.expires_at > SystemTime::now() + Duration::from_secs(60) {
            return Ok(cached.value.clone());
        }
    }

    let refresh_token = tokio::task::spawn_blocking({
        let email = email.to_string();
        move || read_refresh_token(&email)
    })
    .await
    .map_err(|e| e.to_string())??;

    let response = Client::new()
        .post(GOOGLE_TOKEN_URL)
        .form(&[
            ("client_id", google_client.id.as_str()),
            ("client_secret", google_client.secret.as_str()),
            ("refresh_token", refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|e| format!("Could not refresh Google sign-in: {e}"))?
        .error_for_status()
        .map_err(|_| "Your Google sign-in has expired. Please sign in again.".to_string())?
        .json::<GoogleTokenResponse>()
        .await
        .map_err(|e| format!("Could not read Google sign-in response: {e}"))?;

    cache
        .lock()
        .map_err(|_| "OAuth token cache is unavailable".to_string())?
        .insert(
            email.to_string(),
            CachedAccessToken {
                value: response.access_token.clone(),
                expires_at: SystemTime::now() + Duration::from_secs(response.expires_in),
            },
        );
    Ok(response.access_token)
}

/// Discard only the short-lived in-memory token. The refresh token remains in
/// the operating-system credential store, so the next authentication obtains
/// a replacement without asking the user to sign in again.
pub fn invalidate_cached_access_token(email: &str) {
    if let Ok(mut cache) = ACCESS_TOKEN_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        cache.remove(email);
    }
}

#[tauri::command]
pub async fn connect_google_account(app: tauri::AppHandle) -> Result<String, String> {
    let google_client = google_oauth_client()?;
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("Could not start the secure sign-in callback: {e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}");
    let state = Uuid::new_v4().to_string();
    let verifier = format!("{}{}", Uuid::new_v4(), Uuid::new_v4());
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));

    let mut authorize_url = Url::parse(GOOGLE_AUTHORIZE_URL).map_err(|e| e.to_string())?;
    authorize_url.query_pairs_mut().extend_pairs([
        ("client_id", google_client.id.as_str()),
        ("redirect_uri", redirect_uri.as_str()),
        ("response_type", "code"),
        ("scope", "openid email profile https://mail.google.com/"),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("access_type", "offline"),
        ("prompt", "consent"),
        ("state", state.as_str()),
    ]);
    tauri_plugin_opener::open_url(authorize_url.as_str(), None::<&str>)
        .map_err(|e| format!("Could not open your browser: {e}"))?;

    let callback = tokio::task::spawn_blocking(move || wait_for_oauth_callback(listener))
        .await
        .map_err(|e| e.to_string())??;
    if callback.state != state {
        return Err("Google sign-in could not be verified. Please try again.".to_string());
    }
    let code = callback.code.ok_or_else(|| {
        callback
            .error
            .unwrap_or_else(|| "Google sign-in was cancelled.".to_string())
    })?;

    let token_response = Client::new()
        .post(GOOGLE_TOKEN_URL)
        .form(&[
            ("client_id", google_client.id.as_str()),
            ("client_secret", google_client.secret.as_str()),
            ("code", code.as_str()),
            ("code_verifier", verifier.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await
        .map_err(|e| format!("Could not complete Google sign-in: {e}"))?;
    if !token_response.status().is_success() {
        return Err(google_error(token_response, "Google rejected the sign-in").await);
    }
    let token = token_response
        .json::<GoogleTokenResponse>()
        .await
        .map_err(|e| format!("Could not read Google sign-in response: {e}"))?;

    let user = Client::new()
        .get(GOOGLE_USERINFO_URL)
        .bearer_auth(&token.access_token)
        .send()
        .await
        .map_err(|e| format!("Could not identify the Google account: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Could not identify the Google account: {e}"))?
        .json::<GoogleUserInfo>()
        .await
        .map_err(|e| format!("Could not read the Google account: {e}"))?;
    if !user.email_verified {
        return Err("Google did not confirm this email address.".to_string());
    }
    crate::commands::sync::test_imap_connection(
        &user.email,
        &GmailAuth::OAuthAccessToken(token.access_token.clone()),
    )
    .await?;
    let refresh_token = token
        .refresh_token
        .ok_or_else(|| "Google did not return a reusable sign-in. Please try again.".to_string())?;

    tokio::task::spawn_blocking({
        let email = user.email.clone();
        let refresh_token = refresh_token.clone();
        move || save_refresh_token(&email, &refresh_token)
    })
    .await
    .map_err(|e| e.to_string())??;

    let mut store = load_store(&app)?;
    let existing_id = store
        .accounts
        .iter()
        .find(|account| account.email == user.email)
        .map(|account| account.id.clone())
        .filter(|id| Uuid::parse_str(id).is_ok())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    store.accounts.retain(|account| account.email != user.email);
    store.accounts.push(StoredAccount {
        id: existing_id,
        email: user.email.clone(),
        password: String::new(),
        auth_kind: AuthKind::GoogleOAuth,
        profile_picture: user.picture,
    });
    if let Err(error) = save_store(&app, &store) {
        let email = user.email.clone();
        let _ = tokio::task::spawn_blocking(move || delete_refresh_token(&email)).await;
        return Err(error);
    }
    if let Ok(mut cache) = ACCESS_TOKEN_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        cache.insert(
            user.email.clone(),
            CachedAccessToken {
                value: token.access_token,
                expires_at: SystemTime::now() + Duration::from_secs(token.expires_in),
            },
        );
    }
    Ok(user.email)
}

struct OAuthCallback {
    code: Option<String>,
    state: String,
    error: Option<String>,
}

fn wait_for_oauth_callback(listener: TcpListener) -> Result<OAuthCallback, String> {
    let deadline = Instant::now() + OAUTH_CALLBACK_TIMEOUT;
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let mut request = [0_u8; 8192];
                let bytes = stream.read(&mut request).map_err(|e| e.to_string())?;
                let request = String::from_utf8_lossy(&request[..bytes]);
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                let callback_url =
                    Url::parse(&format!("http://127.0.0.1{path}")).map_err(|e| e.to_string())?;
                let params: HashMap<_, _> = callback_url.query_pairs().into_owned().collect();
                let success = params.contains_key("code");
                let body = if success {
                    "You can return to Productive Email."
                } else {
                    "Google sign-in was not completed. You can return to Productive Email."
                };
                let html = format!("<html><body style=\"font-family: -apple-system, sans-serif; padding: 2rem;\">{body}</body></html>");
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}", html.len());
                stream
                    .write_all(response.as_bytes())
                    .map_err(|e| e.to_string())?;
                return Ok(OAuthCallback {
                    code: params.get("code").cloned(),
                    state: params.get("state").cloned().unwrap_or_default(),
                    error: params.get("error").cloned(),
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err("Google sign-in timed out. Please try again.".to_string());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

#[tauri::command]
pub fn get_accounts(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    Ok(load_store(&app)?
        .accounts
        .into_iter()
        .map(|account| account.email)
        .collect())
}

#[tauri::command]
pub fn get_account_profile(app: tauri::AppHandle, email: String) -> Result<AccountProfile, String> {
    let account = load_store(&app)?
        .accounts
        .into_iter()
        .find(|account| account.email == email)
        .ok_or_else(|| format!("No account found for {email}"))?;
    Ok(AccountProfile {
        email: account.email,
        profile_picture: account.profile_picture,
    })
}

#[tauri::command]
pub async fn add_account(
    app: tauri::AppHandle,
    email: String,
    password: String,
) -> Result<String, String> {
    crate::commands::sync::test_imap_connection(&email, &GmailAuth::AppPassword(password.clone()))
        .await?;
    let mut store = load_store(&app)?;
    let existing_id = store
        .accounts
        .iter()
        .find(|account| account.email == email)
        .map(|account| account.id.clone())
        .filter(|id| Uuid::parse_str(id).is_ok())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    store.accounts.retain(|account| account.email != email);
    store.accounts.push(StoredAccount {
        id: existing_id,
        email: email.clone(),
        password,
        auth_kind: AuthKind::AppPassword,
        profile_picture: None,
    });
    save_store(&app, &store)?;
    Ok(email)
}

#[tauri::command]
pub async fn remove_account(app: tauri::AppHandle, email: String) -> Result<(), String> {
    let mut store = load_store(&app)?;
    if let Some(account) = store.accounts.iter().find(|account| account.email == email) {
        if matches!(account.auth_kind, AuthKind::GoogleOAuth) {
            let email_for_keyring = email.clone();
            tokio::task::spawn_blocking(move || delete_refresh_token(&email_for_keyring))
                .await
                .map_err(|e| e.to_string())??;
        }
    }
    store.accounts.retain(|account| account.email != email);
    save_store(&app, &store)
}
