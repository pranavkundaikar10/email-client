use sqlx::{sqlite::SqlitePoolOptions, FromRow, SqlitePool};
use std::fs;
use tauri::{AppHandle, Manager};
use serde::{Deserialize, Serialize};

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
