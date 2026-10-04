use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqlitePoolOptions, FromRow, SqlitePool};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tauri::{AppHandle, Manager};
use uuid::Uuid;

const LEGACY_APP_IDENTIFIER: &str = "com.pranavkundaikar.tmpemail-client-scaffold";

/// Copies data created before the app received its production identifier.
///
/// App data is keyed by the Tauri identifier, so a renamed app otherwise looks
/// like a clean installation. This is deliberately copy-only: the original
/// directory remains an intact fallback if anything goes wrong.
pub fn migrate_legacy_app_data(app: &AppHandle) -> Result<(), String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
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
    copy_if_missing(
        &legacy_dir.join("credentials.json"),
        &data_dir.join("credentials.json"),
    )?;

    let legacy_db = legacy_dir.join("mail.db");
    let target_db = data_dir.join("mail.db");
    if !target_db.exists() && legacy_db.exists() {
        copy_file_atomically(&legacy_db, &target_db)?;
        // SQLite may have recent writes in WAL mode. Copy its companion files
        // before opening the migrated database so SQLite can recover them.
        copy_if_missing(
            &legacy_dir.join("mail.db-wal"),
            &data_dir.join("mail.db-wal"),
        )?;
        copy_if_missing(
            &legacy_dir.join("mail.db-shm"),
            &data_dir.join("mail.db-shm"),
        )?;
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
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;

    fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;

    let db_path = data_dir.join("mail.db");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.to_string_lossy());

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
        .map_err(|e| e.to_string())?;

    // Keep the production migration set embedded in the desktop binary.
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|e| e.to_string())?;

    Ok(pool)
}

/// Converts legacy email-address account IDs to stable UUIDs. All affected
/// rows are migrated together before the application starts any sync workers.
/// A one-time backup is retained beside the database before changing existing
/// data so the original local state remains recoverable.
pub async fn normalize_account_ids(app: &AppHandle, pool: &SqlitePool) -> Result<(), String> {
    let accounts: Vec<(String, String)> = sqlx::query_as("SELECT id, email FROM accounts")
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
    let migrations: Vec<(String, String)> = accounts
        .into_iter()
        .filter(|(id, _)| Uuid::parse_str(id).is_err())
        .map(|(old_id, email)| {
            let new_id = crate::commands::auth::account_id_for_email(app, &email)
                .unwrap_or_else(|_| Uuid::new_v4().to_string());
            (old_id, new_id)
        })
        .collect();
    if migrations.is_empty() {
        return Ok(());
    }

    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let backup = data_dir.join("mail.before-account-id-migration.db");
    let quote_sql_path = |path: &Path| path.to_string_lossy().replace('\'', "''");
    let mut connection = pool.acquire().await.map_err(|e| e.to_string())?;
    if !backup.exists() {
        sqlx::query(&format!("VACUUM INTO '{}'", quote_sql_path(&backup)))
            .execute(&mut *connection)
            .await
            .map_err(|e| format!("Could not back up mail data before account migration: {e}"))?;
    }

    // Updating a primary key and its children requires temporarily disabling
    // SQLite's immediate FK checks; all references are updated in one
    // transaction and checks are restored before returning.
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *connection)
        .await
        .map_err(|e| e.to_string())?;
    let migration_result = async {
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *connection)
            .await
            .map_err(|e| e.to_string())?;
        for (old_id, new_id) in &migrations {
            for table in [
                "threads",
                "messages",
                "mail_operations",
                "mail_flag_operations",
            ] {
                sqlx::query(&format!(
                    "UPDATE {table} SET account_id = ? WHERE account_id = ?"
                ))
                .bind(new_id)
                .bind(old_id)
                .execute(&mut *connection)
                .await
                .map_err(|e| e.to_string())?;
            }
            sqlx::query("UPDATE accounts SET id = ? WHERE id = ?")
                .bind(new_id)
                .bind(old_id)
                .execute(&mut *connection)
                .await
                .map_err(|e| e.to_string())?;
        }
        sqlx::query("COMMIT")
            .execute(&mut *connection)
            .await
            .map_err(|e| e.to_string())?;
        Ok::<(), String>(())
    }
    .await;
    if migration_result.is_err() {
        let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
    }
    let restore_result = sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut *connection)
        .await;
    migration_result?;
    restore_result.map_err(|e| e.to_string())?;
    Ok(())
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

    sqlx::query(&format!(
        "ATTACH DATABASE '{}' AS legacy",
        quote_sql_path(&legacy_db)
    ))
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
            // Splits are copied from the legacy app during the one-time data
            // recovery. App settings only fill missing values: a setting that
            // has already been changed in the renamed app must never revert
            // merely because migration cleanup is retried after a restart.
            "INSERT OR REPLACE INTO splits (id, name, position, rules) SELECT id, name, position, rules FROM legacy.splits",
            "INSERT OR IGNORE INTO app_settings (key, value) SELECT key, value FROM legacy.app_settings",
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
    merge_result?;
    let detach_result = sqlx::query("DETACH DATABASE legacy")
        .execute(&mut *connection)
        .await;

    // The import itself has committed at this point. A best-effort DETACH can
    // occasionally fail while SQLite still holds an internal statement; the
    // connection will be dropped on return, so do not repeat the whole import
    // (and overwrite user choices) on every later launch because of it.
    if let Err(error) = detach_result {
        eprintln!("Could not detach legacy database after migration: {error}");
    }

    fs::write(&marker, "merged\n").map_err(|e| e.to_string())?;

    // Rebuild the external-content search index after importing threads. The
    // one-time data import is already complete even if this derived index
    // rebuild needs to be retried separately.
    sqlx::query("INSERT INTO threads_fts(threads_fts) VALUES('rebuild')")
        .execute(&mut *connection)
        .await
        .map_err(|e| e.to_string())?;
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
    pub analysis_importance: Option<i64>,
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
    pub has_attachments: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, FromRow)]
pub struct AttachmentRow {
    pub id: String,
    pub message_id: String,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: i64,
}

async fn enrich_correspondents(pool: &SqlitePool, rows: &mut [ThreadRow]) {
    if rows.is_empty() {
        return;
    }
    let placeholders = std::iter::repeat("?")
        .take(rows.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        r#"
        SELECT m.thread_id, m.from_name, m.from_email
        FROM messages m JOIN threads t ON t.id = m.thread_id
        JOIN accounts a ON a.id = t.account_id
        WHERE m.thread_id IN ({placeholders})
          AND LOWER(m.from_email) != LOWER(a.email)
          AND m.sent_at = (
              SELECT MAX(m2.sent_at) FROM messages m2
              WHERE m2.thread_id = m.thread_id
                AND LOWER(m2.from_email) != LOWER(a.email)
          )
    "#
    );
    let mut query = sqlx::query_as::<_, (String, String, String)>(&sql);
    for row in rows.iter() {
        query = query.bind(&row.id);
    }
    let matches = match query.fetch_all(pool).await {
        Ok(matches) => matches,
        Err(error) => {
            eprintln!("Could not enrich thread correspondents: {error}");
            return;
        }
    };
    let correspondents: HashMap<_, _> = matches
        .into_iter()
        .map(|(id, name, email)| (id, (name, email)))
        .collect();
    for row in rows {
        if let Some((name, email)) = correspondents.get(&row.id) {
            row.from_name = name.clone();
            row.from_email = email.clone();
        }
    }
}

#[tauri::command]
pub async fn get_threads(
    pool: tauri::State<'_, SqlitePool>,
    account_id: String,
    view: Option<String>,
    category: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<ThreadRow>, String> {
    list_threads_for_account(
        pool.inner(),
        &account_id,
        view.as_deref(),
        category.as_deref(),
        limit,
        offset,
    )
    .await
}

/// The account-scoped thread-list query shared by the Tauri command and
/// isolated tests. Review and follow-up queues are intentionally unified;
/// ordinary list views must never cross this account boundary.
async fn list_threads_for_account(
    pool: &SqlitePool,
    account_id: &str,
    view: Option<&str>,
    category: Option<&str>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<ThreadRow>, String> {
    let limit = limit.unwrap_or(50);
    let offset = offset.unwrap_or(0);
    if offset > 0 {
        eprintln!("Thread list: loading local page at offset {offset} (limit {limit})");
    }

    let where_clause = match view {
        Some("sent") => "t.folder = 'sent'".to_string(),
        Some("drafts") => "t.folder = 'drafts'".to_string(),
        // Gmail's Starred label is independent of Inbox: archived (and sent)
        // threads remain starred until the user explicitly removes the star.
        Some("starred") => "t.starred = 1".to_string(),
        Some("archive") => "t.folder = 'inbox' AND t.archived = 1".to_string(),
        _ => {
            let cat = match category {
                Some(c) if !c.is_empty() => {
                    format!(" AND t.category = '{}'", c.replace('\'', "''"))
                }
                _ => String::new(),
            };
            format!("t.folder = 'inbox' AND t.archived = 0{}", cat)
        }
    };
    // An archive removes a thread from Inbox but must not hide it from Gmail's
    // independent Starred label during the Undo/delivery window. A pending
    // trash operation, however, is hidden everywhere just like a deleted
    // message in Gmail.
    let pending_operation_clause = match view {
        Some("starred") => {
            r#"
          AND NOT EXISTS (
              SELECT 1 FROM mail_operations o
              WHERE o.thread_id = t.id
                AND o.operation = 'trash'
                AND o.status IN ('pending', 'in_progress')
          )
        "#
        }
        _ => {
            r#"
          AND NOT EXISTS (
              SELECT 1 FROM mail_operations o
              WHERE o.thread_id = t.id
                AND o.status IN ('pending', 'in_progress')
          )
        "#
        }
    };

    let mut rows = sqlx::query_as::<_, ThreadRow>(&format!(
        r#"
        SELECT
            t.id, t.account_id, t.subject, t.snippet,
            t.unread, t.starred, t.archived,
            t.last_message_at, t.label_ids,
            COALESCE(m.from_name, '') AS from_name,
            COALESCE(m.from_email, '') AS from_email,
            COALESCE(m.to_emails, '[]') AS to_emails,
            t.category,
            t.folder,
            a.importance AS analysis_importance
        FROM threads t
        LEFT JOIN email_analysis a ON a.thread_id = t.id
        LEFT JOIN messages m ON m.thread_id = t.id
            AND m.sent_at = (SELECT MAX(sent_at) FROM messages WHERE thread_id = t.id)
        WHERE t.account_id = ? AND {} {}
        ORDER BY t.last_message_at DESC
        LIMIT ? OFFSET ?
        "#,
        where_clause, pending_operation_clause,
    ))
    .bind(account_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    if offset > 0 {
        eprintln!("Thread list: local page returned {} threads", rows.len());
    }
    enrich_correspondents(pool, &mut rows).await;
    Ok(rows)
}

#[tauri::command]
pub async fn get_messages(
    pool: tauri::State<'_, SqlitePool>,
    account_id: String,
    thread_id: String,
) -> Result<Vec<MessageRow>, String> {
    get_messages_for_account(pool.inner(), &account_id, &thread_id).await
}

pub(crate) async fn get_messages_for_account(
    pool: &SqlitePool,
    account_id: &str,
    thread_id: &str,
) -> Result<Vec<MessageRow>, String> {
    sqlx::query_as::<_, MessageRow>(
        r#"
        SELECT m.id, m.thread_id, m.from_email, m.from_name,
               COALESCE(m.to_emails, '[]') AS to_emails,
               COALESCE(m.cc_emails, '[]') AS cc_emails,
               m.subject,
               m.body_html, m.body_text, m.sent_at, m.unread, m.body_fetched, m.has_attachments
        FROM messages m
        JOIN threads t ON t.id = m.thread_id
        WHERE m.thread_id = ? AND t.account_id = ?
        ORDER BY sent_at ASC
        "#,
    )
    .bind(thread_id)
    .bind(account_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_message_attachments(
    pool: tauri::State<'_, SqlitePool>,
    message_id: String,
) -> Result<Vec<AttachmentRow>, String> {
    sqlx::query_as::<_, AttachmentRow>(
        "SELECT id, message_id, filename, content_type, size_bytes FROM message_attachments WHERE message_id = ? ORDER BY filename",
    )
    .bind(message_id)
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
    account_id: String,
    query: String,
) -> Result<Vec<ThreadRow>, String> {
    let mut rows = sqlx::query_as::<_, ThreadRow>(
        r#"
        SELECT
            t.id, t.account_id, t.subject, t.snippet,
            t.unread, t.starred, t.archived,
            t.last_message_at, t.label_ids,
            COALESCE(m.from_name, '') AS from_name,
            COALESCE(m.from_email, '') AS from_email,
            COALESCE(m.to_emails, '[]') AS to_emails,
            t.category,
            t.folder,
            a.importance AS analysis_importance
        FROM threads_fts f
        JOIN threads t ON t.rowid = f.rowid
        LEFT JOIN email_analysis a ON a.thread_id = t.id
        LEFT JOIN messages m ON m.thread_id = t.id
            AND m.sent_at = (SELECT MAX(sent_at) FROM messages WHERE thread_id = t.id)
        WHERE threads_fts MATCH ? AND t.account_id = ?
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
    .bind(account_id)
    .fetch_all(pool.inner())
    .await
    .map_err(|e| e.to_string())?;
    enrich_correspondents(pool.inner(), &mut rows).await;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::list_threads_for_account;
    use crate::commands::test_support::TestDatabase;

    #[tokio::test]
    async fn account_thread_lists_never_include_another_accounts_threads() {
        let database = TestDatabase::new().await;
        database.seed_account("account-a", "a@example.com").await;
        database.seed_account("account-b", "b@example.com").await;
        database
            .seed_thread_for_account("thread-a", "account-a")
            .await;
        database
            .seed_thread_for_account("thread-b", "account-b")
            .await;

        let account_a_threads =
            list_threads_for_account(&database.pool, "account-a", None, None, Some(50), Some(0))
                .await
                .expect("list account A threads");
        let account_b_threads =
            list_threads_for_account(&database.pool, "account-b", None, None, Some(50), Some(0))
                .await
                .expect("list account B threads");

        assert_eq!(
            account_a_threads
                .iter()
                .map(|thread| thread.id.as_str())
                .collect::<Vec<_>>(),
            ["thread-a"]
        );
        assert_eq!(
            account_b_threads
                .iter()
                .map(|thread| thread.id.as_str())
                .collect::<Vec<_>>(),
            ["thread-b"]
        );
        database.close().await;
    }
}
