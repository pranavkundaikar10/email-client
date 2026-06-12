use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize)]
struct CredentialsStore {
    accounts: Vec<StoredAccount>,
}

#[derive(Serialize, Deserialize)]
struct StoredAccount {
    email: String,
    password: String,
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
    // Restrict to owner read/write only
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn load_password(app: &AppHandle, email: &str) -> Result<String, String> {
    let store = load_store(app)?;
    store
        .accounts
        .into_iter()
        .find(|a| a.email == email)
        .map(|a| a.password)
        .ok_or_else(|| format!("No credentials found for {}", email))
}

#[tauri::command]
pub async fn add_account(
    app: tauri::AppHandle,
    email: String,
    password: String,
) -> Result<String, String> {
    // Validate by attempting a real IMAP connection before saving
    crate::commands::sync::test_imap_connection(&email, &password).await?;

    let mut store = load_store(&app)?;
    store.accounts.retain(|a| a.email != email);
    store.accounts.push(StoredAccount {
        email: email.clone(),
        password,
    });
    save_store(&app, &store)?;
    Ok(email)
}

#[tauri::command]
pub async fn remove_account(
    app: tauri::AppHandle,
    email: String,
) -> Result<(), String> {
    let mut store = load_store(&app)?;
    store.accounts.retain(|a| a.email != email);
    save_store(&app, &store)
}
