use sqlx::{sqlite::SqlitePoolOptions, FromRow, SqlitePool};
use std::fs;
use std::path::Path;
use tauri::{AppHandle, Manager};
use serde::{Deserialize, Serialize};

const LEGACY_APP_IDENTIFIER: &str = "com.pranavkundaikar.tmpemail-client-scaffold";

/// Copies data created before the app received its production identifier.
///
/// App data is keyed by the Tauri identifier, so a renamed app otherwise looks
/// like a clean installation. This is deliberately copy-only: the original
/// directory remains an intact fallback if anything goes wrong.
pub fn migrate_legacy_app_data(app: &AppHandle) -> Result<(), String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let parent = data_dir
        .parent()
        .ok_or_else(|| "Could not determine the application data directory".to_string())?;
    let legacy_dir = parent.join(LEGACY_APP_IDENTIFIER);

    if !legacy_dir.is_dir() {
        return Ok(());
    }

    fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;

    // Only fill missing files. This makes the migration safe for people who
    // already set up the renamed app, and lets a partially interrupted copy
    // finish on the next launch without overwriting newer data.
    copy_if_missing(&legacy_dir.join("credentials.json"), &data_dir.join("credentials.json"))?;

    let legacy_db = legacy_dir.join("mail.db");
    let target_db = data_dir.join("mail.db");
    if !target_db.exists() && legacy_db.exists() {
        copy_file_atomically(&legacy_db, &target_db)?;
        // SQLite may have recent writes in WAL mode. Copy its companion files
        // before opening the migrated database so SQLite can recover them.
        copy_if_missing(&legacy_dir.join("mail.db-wal"), &data_dir.join("mail.db-wal"))?;
        copy_if_missing(&legacy_dir.join("mail.db-shm"), &data_dir.join("mail.db-shm"))?;
    }

    Ok(())
}

fn copy_if_missing(source: &Path, destination: &Path) -> Result<(), String> {
    if source.exists() && !destination.exists() {
        copy_file_atomically(source, destination)?;
    }
    Ok(())
}

fn copy_file_atomically(source: &Path, destination: &Path) -> Result<(), String> {
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Could not determine migration file name".to_string())?;
    let temporary = destination.with_file_name(format!(".{file_name}.migrating"));

    if temporary.exists() {
        fs::remove_file(&temporary).map_err(|e| e.to_string())?;
    }
    fs::copy(source, &temporary).map_err(|e| e.to_string())?;
    fs::rename(&temporary, destination).map_err(|e| e.to_string())
}

pub async fn init_db(app: &AppHandle) -> Result<SqlitePool, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;

    fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;

    let db_path = data_dir.join("mail.db");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.to_string_lossy());

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|e| e.to_string())?;

    Ok(pool)
}

/// Completes the identifier migration for installations where the renamed app
/// already created a small fresh database before the legacy copy could run.
/// The current database is backed up first, then legacy rows are added without
/// replacing newer mail state. The old database is never modified.
pub async fn merge_legacy_app_data(app: &AppHandle, pool: &SqlitePool) -> Result<(), String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let marker = data_dir.join("legacy-data-merged-v1");
    if marker.exists() {
        return Ok(());
    }
    let parent = data_dir
        .parent()
        .ok_or_else(|| "Could not determine the application data directory".to_string())?;
    let legacy_db = parent.join(LEGACY_APP_IDENTIFIER).join("mail.db");
    let current_db = data_dir.join("mail.db");
    if !legacy_db.exists() || !current_db.exists() {
        return Ok(());
    }

    let quote_sql_path = |path: &Path| path.to_string_lossy().replace('\'', "''");
    let backup = data_dir.join("mail.before-legacy-merge.db");
    let mut connection = pool.acquire().await.map_err(|e| e.to_string())?;

    if !backup.exists() {
        sqlx::query(&format!("VACUUM INTO '{}'", quote_sql_path(&backup)))
            .execute(&mut *connection)
            .await
            .map_err(|e| format!("Could not back up current mail data before migration: {e}"))?;
    }

    sqlx::query(&format!("ATTACH DATABASE '{}' AS legacy", quote_sql_path(&legacy_db)))
        .execute(&mut *connection)
        .await
        .map_err(|e| format!("Could not open legacy mail data: {e}"))?;

    let merge_result = async {
        sqlx::query("BEGIN IMMEDIATE").execute(&mut *connection).await.map_err(|e| e.to_string())?;
        for statement in [
            "INSERT OR IGNORE INTO accounts (id, email, provider, synced_at) SELECT id, email, provider, synced_at FROM legacy.accounts",
            "INSERT OR IGNORE INTO threads (id, account_id, subject, snippet, unread, starred, archived, snoozed_until, last_message_at, label_ids, category, folder) SELECT id, account_id, subject, snippet, unread, starred, archived, snoozed_until, last_message_at, label_ids, category, folder FROM legacy.threads",
            "INSERT OR IGNORE INTO messages (id, thread_id, account_id, from_email, from_name, to_emails, cc_emails, subject, body_html, body_text, sent_at, unread, body_fetched, imap_uid, is_newsletter, has_attachments) SELECT id, thread_id, account_id, from_email, from_name, to_emails, cc_emails, subject, body_html, body_text, sent_at, unread, body_fetched, imap_uid, is_newsletter, has_attachments FROM legacy.messages",
            "INSERT OR IGNORE INTO email_analysis (thread_id, is_actionable, importance, category, summary, action_items, deadline, model, analyzed_at, is_job_related, job_category, recommended_action) SELECT thread_id, is_actionable, importance, category, summary, action_items, deadline, model, analyzed_at, is_job_related, job_category, recommended_action FROM legacy.email_analysis",
            "INSERT OR IGNORE INTO email_reviews (thread_id, decision, reviewed_at) SELECT thread_id, decision, reviewed_at FROM legacy.email_reviews",
            "INSERT OR IGNORE INTO follow_ups (thread_id, due_at, status, created_at, updated_at) SELECT thread_id, due_at, status, created_at, updated_at FROM legacy.follow_ups",
            // Splits and preferences are user-authored local configuration, so
            // the legacy version should win over the fresh defaults.
            "INSERT OR REPLACE INTO splits (id, name, position, rules) SELECT id, name, position, rules FROM legacy.splits",
            "INSERT OR REPLACE INTO app_settings (key, value) SELECT key, value FROM legacy.app_settings",
        ] {
            sqlx::query(statement).execute(&mut *connection).await.map_err(|e| e.to_string())?;
        }
        sqlx::query("COMMIT").execute(&mut *connection).await.map_err(|e| e.to_string())?;
        Ok::<(), String>(())
    }
    .await;

    if merge_result.is_err() {
        let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
    }
    let detach_result = sqlx::query("DETACH DATABASE legacy").execute(&mut *connection).await;
    merge_result?;
    detach_result.map_err(|e| e.to_string())?;

    // Rebuild the external-content search index after importing threads.
    sqlx::query("INSERT INTO threads_fts(threads_fts) VALUES('rebuild')")
        .execute(&mut *connection)
        .await
        .map_err(|e| e.to_string())?;
    fs::write(&marker, "merged\n").map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize, Clone, FromRow)]
pub struct ThreadRow {
    pub id: String,
    pub account_id: String,
    pub subject: String,
    pub snippet: String,
    pub unread: bool,
    pub starred: bool,
    pub archived: bool,
    pub last_message_at: String,
    pub label_ids: String,
    pub from_name: String,
    pub from_email: String,
    pub to_emails: String,
    pub category: String,
    pub folder: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, FromRow)]
pub struct MessageRow {
    pub id: String,
    pub thread_id: String,
    pub from_email: String,
    pub from_name: String,
    pub to_emails: String,
    pub cc_emails: String,
    pub subject: String,
    pub body_html: Option<String>,
    pub body_text: Option<String>,
    pub sent_at: String,
    pub unread: bool,
    pub body_fetched: bool,
}

#[tauri::command]
pub async fn get_threads(
    pool: tauri::State<'_, SqlitePool>,
    view: Option<String>,
    category: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<ThreadRow>, String> {
    let limit = limit.unwrap_or(50);
    let offset = offset.unwrap_or(0);

    let where_clause = match view.as_deref() {
        Some("sent")    => "t.folder = 'sent'".to_string(),
        Some("drafts")  => "t.folder = 'drafts'".to_string(),
        Some("starred") => "t.folder = 'inbox' AND t.starred = 1 AND t.archived = 0".to_string(),
        Some("archive") => "t.folder = 'inbox' AND t.archived = 1".to_string(),
        _               => {
            let cat = match &category {
                Some(c) if !c.is_empty() => format!(" AND t.category = '{}'", c.replace('\'', "''")),
                _ => String::new(),
            };
            format!("t.folder = 'inbox' AND t.archived = 0{}", cat)
        }
    };

    sqlx::query_as::<_, ThreadRow>(&format!(
        r#"
        SELECT
            t.id, t.account_id, t.subject, t.snippet,
            t.unread, t.starred, t.archived,
            t.last_message_at, t.label_ids,
            COALESCE(m.from_name, '') AS from_name,
            COALESCE(m.from_email, '') AS from_email,
            COALESCE(m.to_emails, '[]') AS to_emails,
            t.category,
            t.folder
        FROM threads t
        LEFT JOIN messages m ON m.thread_id = t.id
            AND m.sent_at = (SELECT MAX(sent_at) FROM messages WHERE thread_id = t.id)
        WHERE {}
          AND NOT EXISTS (
              SELECT 1 FROM mail_operations o
              WHERE o.thread_id = t.id
                AND o.status IN ('pending', 'in_progress')
          )
        ORDER BY t.last_message_at DESC
        LIMIT ? OFFSET ?
        "#,
        where_clause
    ))
    .bind(limit)
    .bind(offset)
    .fetch_all(pool.inner())
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_messages(
    pool: tauri::State<'_, SqlitePool>,
    thread_id: String,
) -> Result<Vec<MessageRow>, String> {
    sqlx::query_as::<_, MessageRow>(
        r#"
        SELECT id, thread_id, from_email, from_name,
               COALESCE(to_emails, '[]') AS to_emails,
               COALESCE(cc_emails, '[]') AS cc_emails,
               subject,
               body_html, body_text, sent_at, unread, body_fetched
        FROM messages
        WHERE thread_id = ?
        ORDER BY sent_at ASC
        "#,
    )
    .bind(thread_id)
    .fetch_all(pool.inner())
    .await
    .map_err(|e| e.to_string())
}


#[tauri::command]
pub async fn get_unread_counts(
    pool: tauri::State<'_, SqlitePool>,
) -> Result<std::collections::HashMap<String, i64>, String> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT category, COUNT(*) FROM threads
         WHERE unread = 1 AND archived = 0 AND folder = 'inbox'
           AND NOT EXISTS (
               SELECT 1 FROM mail_operations o
               WHERE o.thread_id = threads.id
                 AND o.status IN ('pending', 'in_progress')
           )
         GROUP BY category",
    )
    .fetch_all(pool.inner())
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows.into_iter().collect())
}

#[tauri::command]
pub async fn search_threads(
    pool: tauri::State<'_, SqlitePool>,
    query: String,
) -> Result<Vec<ThreadRow>, String> {
    sqlx::query_as::<_, ThreadRow>(
        r#"
        SELECT
            t.id, t.account_id, t.subject, t.snippet,
            t.unread, t.starred, t.archived,
            t.last_message_at, t.label_ids,
            COALESCE(m.from_name, '') AS from_name,
            COALESCE(m.from_email, '') AS from_email,
            COALESCE(m.to_emails, '[]') AS to_emails,
            t.category,
            t.folder
        FROM threads_fts f
        JOIN threads t ON t.rowid = f.rowid
        LEFT JOIN messages m ON m.thread_id = t.id
            AND m.sent_at = (SELECT MAX(sent_at) FROM messages WHERE thread_id = t.id)
        WHERE threads_fts MATCH ?
          AND NOT EXISTS (
              SELECT 1 FROM mail_operations o
              WHERE o.thread_id = t.id
                AND o.status IN ('pending', 'in_progress')
          )
        ORDER BY rank
        LIMIT 50
        "#,
    )
    .bind(query)
    .fetch_all(pool.inner())
    .await
    .map_err(|e| e.to_string())
}
