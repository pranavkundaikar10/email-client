use imap::Session;
use mailparse::{parse_mail, MailHeaderMap};
use native_tls::TlsConnector;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::Arc;
use chrono::Utc;
use chrono::NaiveDateTime;
use tokio::sync::Mutex;
use uuid::Uuid;
use serde::Serialize;
use tauri::Emitter;

type ImapSession = Session<native_tls::TlsStream<TcpStream>>;

const GMAIL_IMAP_HOST: &str = "imap.gmail.com";
const GMAIL_IMAP_PORT: u16 = 993;
const SYNC_DAYS: i64 = 365;
const SYNC_LIMIT: usize = 2000;
const MAX_OPERATION_ATTEMPTS: i64 = 3;
// Gmail changes are intentionally held briefly so the client can offer a
// reliable Undo action before any remote state is changed.
const OPERATION_UNDO_WINDOW_SECONDS: i64 = 8;

/// Ensures Gmail-changing operations run one at a time, even when several
/// buttons or keyboard shortcuts are pressed in quick succession.
#[derive(Clone, Default)]
pub struct MailOperationWorker {
    lock: Arc<Mutex<()>>,
}

struct MessageMeta {
    imap_uid: String,
    message_id: String,
    thread_id: String,
    subject: String,
    snippet: String,
    from_name: String,
    from_email: String,
    to_emails: String, // JSON array of raw address strings
    // Gmail's server-assigned IMAP INTERNALDATE. The legacy `sent_at` SQLite
    // column is intentionally populated with this recipient-facing time so
    // every existing view orders exactly as Gmail does.
    received_at: String,
    unread: bool,
    starred: bool,
    is_newsletter: bool,
}

fn is_newsletter(headers: &mailparse::ParsedMail) -> bool {
    if headers.headers.get_first_value("List-Unsubscribe").is_some()
        || headers.headers.get_first_value("List-Id").is_some()
    {
        return true;
    }
    if let Some(prec) = headers.headers.get_first_value("Precedence") {
        let p = prec.to_lowercase();
        if p.contains("bulk") || p.contains("list") || p.contains("junk") {
            return true;
        }
    }
    false
}

fn connect(email: &str, password: &str) -> Result<ImapSession, String> {
    let tls = TlsConnector::new().map_err(|e| e.to_string())?;
    let client =
        imap::connect((GMAIL_IMAP_HOST, GMAIL_IMAP_PORT), GMAIL_IMAP_HOST, &tls)
            .map_err(|e| e.to_string())?;
    client
        .login(email, password)
        .map_err(|(e, _)| e.to_string())
}

pub async fn test_imap_connection(email: &str, password: &str) -> Result<(), String> {
    let email = email.to_string();
    let password = password.to_string();
    tokio::task::spawn_blocking(move || {
        let mut session = connect(&email, &password)?;
        session.logout().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

// Fetch headers only — no bodies. Returns (metadata, full set of all UIDs matching the query).
fn fetch_headers_query(
    email: &str,
    password: &str,
    imap_query: &str,
    limit: usize,
) -> Result<(Vec<MessageMeta>, std::collections::HashSet<u32>), String> {
    let mut session = connect(email, password)?;
    session.select("INBOX").map_err(|e| e.to_string())?;

    let search_result = session
        .uid_search(imap_query)
        .map_err(|e| e.to_string())?;

    // Capture the full UID set before truncation — used for reconciliation.
    let all_inbox_uids: std::collections::HashSet<u32> = search_result.iter().cloned().collect();

    if search_result.is_empty() {
        session.logout().ok();
        return Ok((vec![], all_inbox_uids));
    }

    let mut uids: Vec<u32> = search_result.into_iter().collect();
    uids.sort_unstable_by(|a, b| b.cmp(a));
    uids.truncate(limit);

    let uid_set = uids
        .iter()
        .map(|u| u.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let fetch_items = "(FLAGS INTERNALDATE BODY.PEEK[HEADER.FIELDS (FROM TO CC SUBJECT DATE MESSAGE-ID REFERENCES IN-REPLY-TO LIST-UNSUBSCRIBE LIST-ID PRECEDENCE)])";
    // uid_fetch uses persistent UIDs — stable across sessions
    let messages = session
        .uid_fetch(&uid_set, fetch_items)
        .map_err(|e| e.to_string())?;

    let result = parse_fetched_messages(&messages);
    session.logout().ok();
    Ok((result, all_inbox_uids))
}

fn parse_fetched_messages(messages: &imap::types::ZeroCopy<Vec<imap::types::Fetch>>) -> Vec<MessageMeta> {
    let mut result = Vec::with_capacity(messages.len());
    for msg in messages.iter() {
        let imap_uid = msg.uid.unwrap_or(msg.message).to_string();
        let flags = msg.flags();
        let unread = !flags.iter().any(|f| matches!(f, imap::types::Flag::Seen));
        let starred = flags.iter().any(|f| matches!(f, imap::types::Flag::Flagged));

        let header_bytes = msg.header().unwrap_or(&[]);
        let parsed = parse_mail(header_bytes).ok();

        let subject = parsed.as_ref()
            .and_then(|p| p.headers.get_first_value("Subject"))
            .unwrap_or_default();

        let from_raw = parsed.as_ref()
            .and_then(|p| p.headers.get_first_value("From"))
            .unwrap_or_default();
        let (from_name, from_email) = parse_from(&from_raw);

        let to_raw = parsed.as_ref()
            .and_then(|p| p.headers.get_first_value("To"))
            .unwrap_or_default();
        let to_parts: Vec<String> = to_raw.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        let to_emails = serde_json::to_string(&to_parts).unwrap_or_else(|_| "[]".to_string());

        let date_raw = parsed.as_ref()
            .and_then(|p| p.headers.get_first_value("Date"))
            .unwrap_or_default();
        // INTERNALDATE is assigned by Gmail when it accepts the message and
        // is the timestamp Gmail uses for mailbox ordering. Only malformed or
        // non-conforming servers need the RFC Date header fallback.
        let received_at = msg
            .internal_date()
            .map(|date| date.with_timezone(&Utc).to_rfc3339())
            .unwrap_or_else(|| parse_date(&date_raw));

        let message_id = parsed.as_ref()
            .and_then(|p| p.headers.get_first_value("Message-ID"))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| imap_uid.clone());

        let thread_id = parsed.as_ref()
            .and_then(|p| {
                p.headers.get_first_value("In-Reply-To")
                    .or_else(|| p.headers.get_first_value("References"))
            })
            .and_then(|s| s.split_whitespace().next().map(|s| s.trim().to_string()))
            .unwrap_or_else(|| message_id.clone());

        let snippet: String = subject.chars().take(120).collect();
        let newsletter = parsed.as_ref().map(|p| is_newsletter(p)).unwrap_or(false);

        result.push(MessageMeta {
            imap_uid, message_id, thread_id, subject, snippet,
            from_name, from_email, to_emails, received_at, unread, starred,
            is_newsletter: newsletter,
        });
    }
    result
}

fn fetch_headers(
    email: &str,
    password: &str,
) -> Result<(Vec<MessageMeta>, std::collections::HashSet<u32>, String), String> {
    let cutoff = Utc::now() - chrono::Duration::days(SYNC_DAYS);
    let imap_since = cutoff.format("%d-%b-%Y").to_string();
    let db_since = cutoff.format("%Y-%m-%dT%H:%M:%S").to_string();
    let (metas, inbox_uids) =
        fetch_headers_query(email, password, &format!("SINCE {}", imap_since), SYNC_LIMIT)?;
    Ok((metas, inbox_uids, db_since))
}

// Fetches from a non-INBOX folder located via RFC 6154 special-use attribute.
fn fetch_folder_headers(email: &str, password: &str, special_attr: &str, fallback: &str) -> Result<Vec<MessageMeta>, String> {
    let mut session = connect(email, password)?;
    let mailbox = find_special_mailbox(&mut session, special_attr)
        .unwrap_or_else(|| fallback.to_string());
    session.select(&mailbox).map_err(|e| e.to_string())?;

    let since = (Utc::now() - chrono::Duration::days(SYNC_DAYS))
        .format("%d-%b-%Y")
        .to_string();
    let search_result = session.uid_search(&format!("SINCE {}", since))
        .map_err(|e| e.to_string())?;

    if search_result.is_empty() {
        session.logout().ok();
        return Ok(vec![]);
    }

    let mut uids: Vec<u32> = search_result.into_iter().collect();
    uids.sort_unstable_by(|a, b| b.cmp(a));
    uids.truncate(SYNC_LIMIT);
    let uid_set = uids.iter().map(|u| u.to_string()).collect::<Vec<_>>().join(",");

    let fetch_items = "(FLAGS INTERNALDATE BODY.PEEK[HEADER.FIELDS (FROM TO CC SUBJECT DATE MESSAGE-ID REFERENCES IN-REPLY-TO LIST-UNSUBSCRIBE LIST-ID PRECEDENCE)])";
    let messages = session.uid_fetch(&uid_set, fetch_items).map_err(|e| e.to_string())?;
    let result = parse_fetched_messages(&messages);
    session.logout().ok();
    Ok(result)
}

fn fetch_older_headers(email: &str, password: &str, before_imap_date: &str) -> Result<Vec<MessageMeta>, String> {
    let (metas, _) = fetch_headers_query(email, password, &format!("BEFORE {}", before_imap_date), 200)?;
    Ok(metas)
}

async fn write_metas_to_db(pool: &SqlitePool, email: &str, folder: &str, metas: Vec<MessageMeta>) -> Result<usize, String> {
    let count = metas.len();
    if count == 0 { return Ok(0); }

    let split_rows = crate::commands::splits::load_splits(pool).await?;

    sqlx::query("INSERT OR IGNORE INTO accounts (id, email, provider) VALUES (?, ?, 'gmail')")
        .bind(email)
        .bind(email)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;

    for m in &metas {
        let category = crate::commands::splits::evaluate_splits(
            &split_rows, &m.from_email, &m.subject, m.is_newsletter,
        );

        sqlx::query(
            r#"
            INSERT INTO threads (id, account_id, subject, snippet, unread, starred, last_message_at, label_ids, category, folder)
            VALUES (?, ?, ?, ?, ?, ?, ?, '[]', ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                snippet         = excluded.snippet,
                last_message_at = MAX(last_message_at, excluded.last_message_at),
                category        = excluded.category,
                folder          = excluded.folder,
                unread          = MIN(threads.unread, excluded.unread)
            "#,
        )
        .bind(&m.thread_id).bind(email).bind(&m.subject).bind(&m.snippet)
        .bind(m.unread as i64).bind(m.starred as i64).bind(&m.received_at).bind(&category).bind(folder)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

        sqlx::query(
            r#"
            INSERT INTO messages
                (id, thread_id, account_id, from_email, from_name, to_emails, subject, sent_at, unread, body_fetched, imap_uid, is_newsletter)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                imap_uid = CASE WHEN imap_uid = '' THEN excluded.imap_uid ELSE imap_uid END,
                to_emails = CASE WHEN to_emails = '' OR to_emails = '[]' THEN excluded.to_emails ELSE to_emails END,
                -- Backfill old locally cached sender dates on the next header sync.
                sent_at = excluded.sent_at
            "#,
        )
        .bind(&m.message_id).bind(&m.thread_id).bind(email)
        .bind(&m.from_email).bind(&m.from_name).bind(&m.to_emails).bind(&m.subject).bind(&m.received_at)
        .bind(m.unread as i64).bind(&m.imap_uid).bind(m.is_newsletter as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

        sqlx::query(
            "INSERT OR REPLACE INTO threads_fts(rowid, subject, snippet, from_email)
             SELECT rowid, subject, snippet, ? FROM threads WHERE id = ?",
        )
        .bind(&m.from_email).bind(&m.thread_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }

    // Existing installs previously stored the sender's Date header. Rebuild
    // each touched thread from the now-normalized message timestamps so a
    // newer-but-delayed message cannot remain buried under stale metadata.
    sqlx::query(
        "UPDATE threads SET last_message_at = (SELECT MAX(sent_at) FROM messages WHERE thread_id = threads.id) WHERE account_id = ? AND folder = ?",
    )
    .bind(email)
    .bind(folder)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(count)
}

// After a sync, mark inbox threads removed from Gmail as archived locally.
// Threads whose IMAP UIDs are all absent from `current_inbox_uids` were archived/deleted on Gmail.
async fn reconcile_removed(
    pool: &SqlitePool,
    email: &str,
    since_date: &str,
    current_inbox_uids: std::collections::HashSet<u32>,
) -> Result<(), String> {
    // All active inbox threads in the sync window
    let thread_ids: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT t.id FROM threads t
         WHERE t.account_id = ? AND t.folder = 'inbox' AND t.archived = 0
           AND t.last_message_at >= ?",
    )
    .bind(email)
    .bind(since_date)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    for (thread_id,) in thread_ids {
        let msg_uids: Vec<(String,)> = sqlx::query_as(
            "SELECT imap_uid FROM messages WHERE thread_id = ? AND imap_uid != ''",
        )
        .bind(&thread_id)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;

        if msg_uids.is_empty() {
            continue;
        }

        let any_in_inbox = msg_uids.iter().any(|(uid_str,)| {
            uid_str
                .parse::<u32>()
                .map(|u| current_inbox_uids.contains(&u))
                .unwrap_or(false)
        });

        if !any_in_inbox {
            sqlx::query("UPDATE threads SET archived = 1 WHERE id = ?")
                .bind(&thread_id)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn sync_inbox(
    app: tauri::AppHandle,
    pool: tauri::State<'_, SqlitePool>,
    email: String,
) -> Result<usize, String> {
    let password = crate::commands::auth::load_password(&app, &email)?;
    let email_for_imap = email.clone();

    let (metas, inbox_uids, db_since) =
        tokio::task::spawn_blocking(move || fetch_headers(&email_for_imap, &password))
            .await
            .map_err(|e| e.to_string())??;

    let count = write_metas_to_db(pool.inner(), &email, "inbox", metas).await?;
    reconcile_removed(pool.inner(), &email, &db_since, inbox_uids).await?;
    Ok(count)
}

#[tauri::command]
pub async fn sync_older(
    app: tauri::AppHandle,
    pool: tauri::State<'_, SqlitePool>,
    email: String,
    before_date: String,
) -> Result<usize, String> {
    let password = crate::commands::auth::load_password(&app, &email)?;
    let email_for_imap = email.clone();

    // Parse SQLite timestamp and reformat as IMAP date (DD-Mon-YYYY)
    let imap_date = NaiveDateTime::parse_from_str(&before_date, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(&before_date, "%Y-%m-%d %H:%M:%S"))
        .map(|dt| dt.format("%d-%b-%Y").to_string())
        .unwrap_or_else(|_| before_date.chars().take(10).collect::<String>()
            .replace('-', "-")); // fallback: keep YYYY-MM-DD as-is (won't match but won't crash)

    let metas = tokio::task::spawn_blocking(move || {
        fetch_older_headers(&email_for_imap, &password, &imap_date)
    })
    .await
    .map_err(|e| e.to_string())??;

    write_metas_to_db(pool.inner(), &email, "inbox", metas).await
}

#[tauri::command]
pub async fn sync_sent(
    app: tauri::AppHandle,
    pool: tauri::State<'_, SqlitePool>,
    email: String,
) -> Result<usize, String> {
    let password = crate::commands::auth::load_password(&app, &email)?;
    let email_for_imap = email.clone();
    let metas = tokio::task::spawn_blocking(move || {
        fetch_folder_headers(&email_for_imap, &password, "\\Sent", "[Gmail]/Sent Mail")
    })
    .await
    .map_err(|e| e.to_string())??;
    write_metas_to_db(pool.inner(), &email, "sent", metas).await
}

#[tauri::command]
pub async fn sync_drafts(
    app: tauri::AppHandle,
    pool: tauri::State<'_, SqlitePool>,
    email: String,
) -> Result<usize, String> {
    let password = crate::commands::auth::load_password(&app, &email)?;
    let email_for_imap = email.clone();
    let metas = tokio::task::spawn_blocking(move || {
        fetch_folder_headers(&email_for_imap, &password, "\\Drafts", "[Gmail]/Drafts")
    })
    .await
    .map_err(|e| e.to_string())??;
    write_metas_to_db(pool.inner(), &email, "drafts", metas).await
}


#[tauri::command]
pub async fn fetch_message_body(
    app: tauri::AppHandle,
    pool: tauri::State<'_, SqlitePool>,
    email: String,
    message_id: String,
    force: Option<bool>,
) -> Result<crate::commands::db::MessageRow, String> {
    // Use i64 for body_fetched — SQLite stores booleans as INTEGER
    let row: Option<(i64, String)> = sqlx::query_as(
        "SELECT body_fetched, imap_uid FROM messages WHERE id = ?",
    )
    .bind(&message_id)
    .fetch_optional(pool.inner())
    .await
    .map_err(|e| e.to_string())?;

    let (body_fetched_int, imap_uid) = row
        .ok_or_else(|| format!("Message {} not found in DB", message_id))?;
    let body_fetched = body_fetched_int != 0;

    let thread_id: String =
        sqlx::query_scalar("SELECT thread_id FROM messages WHERE id = ?")
            .bind(&message_id)
            .fetch_one(pool.inner())
            .await
            .map_err(|e| e.to_string())?;

    if body_fetched && !force.unwrap_or(false) {
        let messages = crate::commands::db::get_messages(pool, thread_id).await?;
        return messages
            .into_iter()
            .find(|m| m.id == message_id)
            .ok_or_else(|| "Message not found".to_string());
    }

    let folder: String = sqlx::query_scalar(
        "SELECT t.folder FROM threads t JOIN messages m ON m.thread_id = t.id WHERE m.id = ?",
    )
    .bind(&message_id)
    .fetch_one(pool.inner())
    .await
    .unwrap_or_else(|_| "inbox".to_string());

    let password = crate::commands::auth::load_password(&app, &email)?;
    let message_id_clone = message_id.clone();

    let (body_html, body_text, has_attachments) = tokio::task::spawn_blocking(move || {
        let mut session = connect(&email, &password)?;

        let mailbox = match folder.as_str() {
            "sent"   => find_special_mailbox(&mut session, "\\Sent")
                            .unwrap_or_else(|| "[Gmail]/Sent Mail".to_string()),
            "drafts" => find_special_mailbox(&mut session, "\\Drafts")
                            .unwrap_or_else(|| "[Gmail]/Drafts".to_string()),
            _        => "INBOX".to_string(),
        };
        session.select(&mailbox).map_err(|e| e.to_string())?;

        // If imap_uid is missing (old row), find it via Message-ID header search
        let uid_to_fetch = if imap_uid.is_empty() {
            let search = session
                .uid_search(format!("HEADER MESSAGE-ID \"{}\"", message_id))
                .map_err(|e| e.to_string())?;
            search
                .into_iter()
                .next()
                .map(|u| u.to_string())
                .ok_or_else(|| format!("Could not locate message {} via search", message_id))?
        } else {
            imap_uid
        };

        let msgs = session
            .uid_fetch(&uid_to_fetch, "BODY.PEEK[]")
            .map_err(|e| e.to_string())?;

        let fetch = msgs.first()
            .ok_or_else(|| format!("UID {} not found in INBOX", uid_to_fetch))?;

        let raw = fetch.body()
            .ok_or_else(|| format!("UID {} returned no body data", uid_to_fetch))?;

        let parsed = parse_mail(raw).map_err(|e| e.to_string())?;
        let result = extract_body_parts(&parsed);
        session.logout().ok();
        Ok::<_, String>(result)
    })
    .await
    .map_err(|e| e.to_string())??;

    sqlx::query(
        "UPDATE messages SET body_html = ?, body_text = ?, has_attachments = ?, body_fetched = 1 WHERE id = ?",
    )
    .bind(&body_html)
    .bind(&body_text)
    .bind(has_attachments as i64)
    .bind(&message_id_clone)
    .execute(pool.inner())
    .await
    .map_err(|e| e.to_string())?;

    let messages = crate::commands::db::get_messages(pool, thread_id).await?;
    messages
        .into_iter()
        .find(|m| m.id == message_id_clone)
        .ok_or_else(|| "Message not found after fetch".to_string())
}

#[derive(sqlx::FromRow)]
struct PrefetchMessageRow {
    id: String,
    imap_uid: String,
    folder: String,
}

/// Fetch up to three upcoming thread bodies over one IMAP connection. This is
/// deliberately read-only (BODY.PEEK) and never invokes local AI processing.
#[tauri::command]
pub async fn prefetch_thread_bodies(
    app: tauri::AppHandle,
    pool: tauri::State<'_, SqlitePool>,
    email: String,
    thread_ids: Vec<String>,
) -> Result<usize, String> {
    let thread_ids: Vec<String> = thread_ids.into_iter().take(3).collect();
    if thread_ids.is_empty() { return Ok(0); }

    let placeholders = std::iter::repeat("?")
        .take(thread_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        r#"SELECT m.id, m.imap_uid, t.folder
           FROM messages m JOIN threads t ON t.id = m.thread_id
           WHERE m.thread_id IN ({placeholders})
             AND t.account_id = ?
             AND m.body_fetched = 0
             AND m.imap_uid != ''
             AND m.sent_at = (SELECT MAX(sent_at) FROM messages WHERE thread_id = m.thread_id)
           LIMIT 3"#,
    );
    let mut query = sqlx::query_as::<_, PrefetchMessageRow>(&sql);
    for thread_id in &thread_ids { query = query.bind(thread_id); }
    let mut rows = query.bind(&email).fetch_all(pool.inner()).await.map_err(|e| e.to_string())?;
    let Some(first) = rows.first() else { return Ok(0); };

    // A sidebar normally contains one folder. Search can mix folders, so only
    // prefetch the matching folder here rather than opening extra connections.
    let folder = first.folder.clone();
    rows.retain(|row| row.folder == folder);
    let uid_to_id: HashMap<u32, String> = rows.iter()
        .filter_map(|row| row.imap_uid.parse::<u32>().ok().map(|uid| (uid, row.id.clone())))
        .collect();
    if uid_to_id.is_empty() { return Ok(0); }

    let password = crate::commands::auth::load_password(&app, &email)?;
    let uid_set = uid_to_id.keys().map(u32::to_string).collect::<Vec<_>>().join(",");
    let fetched = tokio::task::spawn_blocking(move || {
        let mut session = connect(&email, &password)?;
        let mailbox = match folder.as_str() {
            "sent" => find_special_mailbox(&mut session, "\\Sent").unwrap_or_else(|| "[Gmail]/Sent Mail".to_string()),
            "drafts" => find_special_mailbox(&mut session, "\\Drafts").unwrap_or_else(|| "[Gmail]/Drafts".to_string()),
            _ => "INBOX".to_string(),
        };
        session.select(&mailbox).map_err(|e| e.to_string())?;
        let responses = session.uid_fetch(&uid_set, "BODY.PEEK[]").map_err(|e| e.to_string())?;
        let mut bodies = Vec::new();
        for response in responses.iter() {
            let Some(uid) = response.uid else { continue; };
            let Some(message_id) = uid_to_id.get(&uid) else { continue; };
            let Some(raw) = response.body() else { continue; };
            let parsed = parse_mail(raw).map_err(|e| e.to_string())?;
            let (html, text, attachments) = extract_body_parts(&parsed);
            bodies.push((message_id.clone(), html, text, attachments));
        }
        session.logout().ok();
        Ok::<_, String>(bodies)
    }).await.map_err(|e| e.to_string())??;

    let mut saved = 0;
    for (message_id, body_html, body_text, has_attachments) in fetched {
        let result = sqlx::query(
            "UPDATE messages SET body_html = ?, body_text = ?, has_attachments = ?, body_fetched = 1 WHERE id = ? AND body_fetched = 0",
        )
        .bind(body_html)
        .bind(body_text)
        .bind(has_attachments as i64)
        .bind(message_id)
        .execute(pool.inner())
        .await
        .map_err(|e| e.to_string())?;
        saved += result.rows_affected() as usize;
    }
    Ok(saved)
}

// Find a special-use mailbox by its RFC 6154 attribute (e.g. "\\Trash", "\\Sent").
fn find_special_mailbox(session: &mut ImapSession, attr: &str) -> Option<String> {
    let names = session.list(None, Some("*")).ok()?;
    let attr_lc = attr.to_lowercase();
    for name in names.iter() {
        let has_attr = name.attributes().iter().any(|a| {
            match a {
                imap::types::NameAttribute::Custom(s) => s.to_lowercase() == attr_lc,
                _ => false,
            }
        });
        if has_attr {
            return Some(name.name().to_string());
        }
    }
    None
}

// Resolve IMAP UIDs for a thread by searching Message-ID in the selected mailbox.
// Always searches rather than trusting stored UIDs, since stored values can be stale.
fn resolve_uids(session: &mut ImapSession, rows: &[(String, String)]) -> Result<Vec<String>, String> {
    let mut uids = Vec::new();

    for (msg_id, stored_uid) in rows {
        let found = session
            .uid_search(format!("HEADER MESSAGE-ID \"{}\"", msg_id))
            .unwrap_or_default();

        if !found.is_empty() {
            uids.extend(found.into_iter().map(|u| u.to_string()));
        } else if !stored_uid.is_empty() {
            // Fall back to stored UID only if Message-ID search finds nothing
            uids.push(stored_uid.clone());
        }
    }

    Ok(uids)
}

// special_attr: RFC 6154 attribute to resolve destination dynamically, e.g. "\\Trash", "\\All"
// fallback: hardcoded name used if attribute lookup fails
async fn imap_move_thread(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    thread_id: &str,
    special_attr: &'static str,
    fallback: &'static str,
) -> Result<(), String> {
    let account_id: String =
        sqlx::query_scalar("SELECT account_id FROM threads WHERE id = ?")
            .bind(thread_id)
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;

    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, imap_uid FROM messages WHERE thread_id = ?",
    )
    .bind(thread_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let source_folder: String = sqlx::query_scalar("SELECT folder FROM threads WHERE id = ?")
        .bind(thread_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "inbox".to_string());

    let password = crate::commands::auth::load_password(app, &account_id)?;
    let email = account_id.clone();

    tokio::task::spawn_blocking(move || {
        let mut session = connect(&email, &password)?;

        let dest = find_special_mailbox(&mut session, special_attr)
            .unwrap_or_else(|| fallback.to_string());

        let source_mailbox = match source_folder.as_str() {
            "sent"   => find_special_mailbox(&mut session, "\\Sent")
                            .unwrap_or_else(|| "[Gmail]/Sent Mail".to_string()),
            "drafts" => find_special_mailbox(&mut session, "\\Drafts")
                            .unwrap_or_else(|| "[Gmail]/Drafts".to_string()),
            _        => "INBOX".to_string(),
        };
        session.select(&source_mailbox).map_err(|e| e.to_string())?;

        let uids = resolve_uids(&mut session, &rows)?;
        if uids.is_empty() {
            session.logout().ok();
            return Err("No IMAP UIDs found for this thread — try syncing first".to_string());
        }

        session
            .uid_mv(&uids.join(","), &dest)
            .map_err(|e| e.to_string())?;

        session.logout().ok();
        Ok::<_, String>(())
    })
    .await
    .map_err(|e| e.to_string())??;

    Ok(())
}

#[tauri::command]
pub async fn mark_thread_read(
    app: tauri::AppHandle,
    pool: tauri::State<'_, SqlitePool>,
    worker: tauri::State<'_, MailOperationWorker>,
    thread_id: String,
) -> Result<(), String> {
    // Update local DB immediately so the UI responds without waiting for IMAP
    sqlx::query("UPDATE threads SET unread = 0 WHERE id = ?")
        .bind(&thread_id)
        .execute(pool.inner())
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("UPDATE messages SET unread = 0 WHERE thread_id = ?")
        .bind(&thread_id)
        .execute(pool.inner())
        .await
        .map_err(|e| e.to_string())?;

    enqueue_mail_flag_operation(pool.inner(), &thread_id, Some(true), None).await?;
    start_operation_worker(app, pool.inner().clone(), worker.inner().clone());

    Ok(())
}

/// Mark a thread unread in the local app database. This intentionally does
/// not change the remote IMAP mailbox; it is useful for resurfacing mail for
/// local workflows such as inbox triage.
#[tauri::command]
pub async fn mark_thread_unread(
    pool: tauri::State<'_, SqlitePool>,
    thread_id: String,
) -> Result<(), String> {
    sqlx::query("UPDATE threads SET unread = 1 WHERE id = ?")
        .bind(&thread_id)
        .execute(pool.inner())
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("UPDATE messages SET unread = 1 WHERE thread_id = ?")
        .bind(&thread_id)
        .execute(pool.inner())
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn star_thread(
    app: tauri::AppHandle,
    pool: tauri::State<'_, SqlitePool>,
    worker: tauri::State<'_, MailOperationWorker>,
    thread_id: String,
    starred: bool,
) -> Result<(), String> {
    sqlx::query("UPDATE threads SET starred = ? WHERE id = ?")
        .bind(starred as i64)
        .bind(&thread_id)
        .execute(pool.inner())
        .await
        .map_err(|e| e.to_string())?;

    enqueue_mail_flag_operation(pool.inner(), &thread_id, None, Some(starred)).await?;
    start_operation_worker(app, pool.inner().clone(), worker.inner().clone());

    Ok(())
}

#[derive(sqlx::FromRow)]
struct MailOperation {
    id: String,
    thread_id: String,
    operation: String,
    attempt_count: i64,
}

#[derive(sqlx::FromRow)]
struct MailFlagOperation {
    id: String,
    thread_id: String,
    seen: Option<bool>,
    starred: Option<bool>,
    attempt_count: i64,
    revision: i64,
}

#[derive(Clone, Serialize)]
struct MailOperationFailure {
    thread_id: String,
    operation: String,
    error: String,
}

async fn enqueue_mail_operation(
    pool: &SqlitePool,
    thread_id: &str,
    operation: &str,
) -> Result<(), String> {
    let account_id: String = sqlx::query_scalar("SELECT account_id FROM threads WHERE id = ?")
        .bind(thread_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Email thread no longer exists locally".to_string())?;
    let now = Utc::now();
    let execute_after = (now + chrono::Duration::seconds(OPERATION_UNDO_WINDOW_SECONDS)).to_rfc3339();
    let now = now.to_rfc3339();

    // One current intent per thread. Repeating an action is safe and puts a
    // previously failed attempt back into the queue.
    sqlx::query(
        r#"INSERT INTO mail_operations
           (id, account_id, thread_id, operation, status, attempt_count, next_retry_at, created_at, updated_at)
           VALUES (?, ?, ?, ?, 'pending', 0, ?, ?, ?)
           ON CONFLICT(thread_id) DO UPDATE SET
             operation = excluded.operation,
             status = 'pending',
             attempt_count = 0,
             next_retry_at = excluded.next_retry_at,
             last_error = NULL,
             updated_at = excluded.updated_at"#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(account_id)
    .bind(thread_id)
    .bind(operation)
    .bind(&execute_after)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    // Moving a thread out of Inbox supersedes any not-yet-applied flag change.
    sqlx::query("DELETE FROM mail_flag_operations WHERE thread_id = ?")
        .bind(thread_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Coalesce rapid local read/star changes into one durable desired state.
async fn enqueue_mail_flag_operation(
    pool: &SqlitePool,
    thread_id: &str,
    seen: Option<bool>,
    starred: Option<bool>,
) -> Result<(), String> {
    let account_id: String = sqlx::query_scalar("SELECT account_id FROM threads WHERE id = ?")
        .bind(thread_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Email thread no longer exists locally".to_string())?;
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        r#"INSERT INTO mail_flag_operations
           (id, account_id, thread_id, seen, starred, status, attempt_count, next_retry_at, revision, created_at, updated_at)
           VALUES (?, ?, ?, ?, ?, 'pending', 0, ?, 1, ?, ?)
           ON CONFLICT(thread_id) DO UPDATE SET
             seen = COALESCE(excluded.seen, mail_flag_operations.seen),
             starred = COALESCE(excluded.starred, mail_flag_operations.starred),
             status = 'pending',
             attempt_count = 0,
             next_retry_at = excluded.next_retry_at,
             last_error = NULL,
             revision = mail_flag_operations.revision + 1,
             updated_at = excluded.updated_at"#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(account_id)
    .bind(thread_id)
    .bind(seen)
    .bind(starred)
    .bind(&now)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

async fn imap_sync_thread_flags(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    thread_id: &str,
    seen: Option<bool>,
    starred: Option<bool>,
) -> Result<(), String> {
    let account_id: String = sqlx::query_scalar("SELECT account_id FROM threads WHERE id = ?")
        .bind(thread_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Email thread no longer exists locally".to_string())?;
    let folder: String = sqlx::query_scalar("SELECT folder FROM threads WHERE id = ?")
        .bind(thread_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "inbox".to_string());
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, imap_uid FROM messages WHERE thread_id = ?",
    )
    .bind(thread_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    let password = crate::commands::auth::load_password(app, &account_id)?;

    tokio::task::spawn_blocking(move || {
        let mut session = connect(&account_id, &password)?;
        let mailbox = match folder.as_str() {
            "sent" => find_special_mailbox(&mut session, "\\Sent")
                .unwrap_or_else(|| "[Gmail]/Sent Mail".to_string()),
            "drafts" => find_special_mailbox(&mut session, "\\Drafts")
                .unwrap_or_else(|| "[Gmail]/Drafts".to_string()),
            _ => "INBOX".to_string(),
        };
        session.select(&mailbox).map_err(|e| e.to_string())?;
        let uids = resolve_uids(&mut session, &rows)?;
        if !uids.is_empty() {
            let uid_set = uids.join(",");
            if let Some(read) = seen {
                let operation = if read { "+FLAGS.SILENT (\\Seen)" } else { "-FLAGS.SILENT (\\Seen)" };
                session.uid_store(&uid_set, operation).map_err(|e| e.to_string())?;
            }
            if let Some(is_starred) = starred {
                let operation = if is_starred { "+FLAGS.SILENT (\\Flagged)" } else { "-FLAGS.SILENT (\\Flagged)" };
                session.uid_store(&uid_set, operation).map_err(|e| e.to_string())?;
            }
        }
        session.logout().ok();
        Ok::<_, String>(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Attempts one queued Gmail change. Keeping the worker deliberately
/// single-item and serialized avoids competing IMAP connections and lets the
/// normal 60-second sync cycle handle retries without consuming resources.
pub async fn process_mail_operations(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    worker: &MailOperationWorker,
) -> Result<(), String> {
    let _guard = worker.lock.lock().await;
    let now = Utc::now().to_rfc3339();

    // A process may have stopped while IMAP was in flight. It is safe to retry
    // that operation, and avoids a permanently hidden thread after restart.
    sqlx::query("UPDATE mail_operations SET status = 'pending', updated_at = ? WHERE status = 'in_progress'")
        .bind(&now)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("UPDATE mail_flag_operations SET status = 'pending', updated_at = ? WHERE status = 'in_progress'")
        .bind(&now)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    let item: Option<MailOperation> = sqlx::query_as(
        "SELECT id, thread_id, operation, attempt_count FROM mail_operations \
         WHERE status = 'pending' AND next_retry_at <= ? ORDER BY created_at ASC LIMIT 1",
    )
    .bind(&now)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    let Some(item) = item else {
        return process_mail_flag_operation(app, pool, &now).await;
    };

    sqlx::query("UPDATE mail_operations SET status = 'in_progress', updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(&item.id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    let result = match item.operation.as_str() {
        "archive" => imap_move_thread(app, pool, &item.thread_id, "\\All", "[Gmail]/All Mail").await,
        "trash" => imap_move_thread(app, pool, &item.thread_id, "\\Trash", "[Gmail]/Trash").await,
        _ => Err("Unknown queued mail operation".to_string()),
    };

    match result {
        Ok(()) => {
            sqlx::query("UPDATE threads SET archived = 1 WHERE id = ?")
                .bind(&item.thread_id)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
            sqlx::query("DELETE FROM mail_operations WHERE id = ?")
                .bind(&item.id)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
        }
        Err(error) => {
            let attempts = item.attempt_count + 1;
            let status = if attempts >= MAX_OPERATION_ATTEMPTS { "failed" } else { "pending" };
            // 10s, 30s, then surface the thread again with a clear error.
            let delay_secs = match attempts { 1 => 10, 2 => 30, _ => 0 };
            let retry_at = (Utc::now() + chrono::Duration::seconds(delay_secs)).to_rfc3339();
            sqlx::query(
                "UPDATE mail_operations SET status = ?, attempt_count = ?, next_retry_at = ?, last_error = ?, updated_at = ? WHERE id = ?",
            )
            .bind(status)
            .bind(attempts)
            .bind(retry_at)
            .bind(&error)
            .bind(Utc::now().to_rfc3339())
            .bind(&item.id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
            if status == "failed" {
                let _ = app.emit(
                    "mail-operation-failed",
                    MailOperationFailure {
                        thread_id: item.thread_id,
                        operation: item.operation,
                        error,
                    },
                );
            }
        }
    }
    Ok(())
}

async fn process_mail_flag_operation(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    now: &str,
) -> Result<(), String> {
    // Archive/delete owns the thread while it is queued, so do not spend an
    // IMAP round trip applying flags to mail about to leave the Inbox.
    let item: Option<MailFlagOperation> = sqlx::query_as(
        "SELECT id, thread_id, seen, starred, attempt_count, revision FROM mail_flag_operations \
         WHERE status = 'pending' AND next_retry_at <= ? \
           AND NOT EXISTS (SELECT 1 FROM mail_operations o WHERE o.thread_id = mail_flag_operations.thread_id AND o.status IN ('pending', 'in_progress')) \
         ORDER BY created_at ASC LIMIT 1",
    )
    .bind(now)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    let Some(item) = item else { return Ok(()); };

    sqlx::query("UPDATE mail_flag_operations SET status = 'in_progress', updated_at = ? WHERE id = ? AND revision = ?")
        .bind(now)
        .bind(&item.id)
        .bind(item.revision)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    let result = imap_sync_thread_flags(app, pool, &item.thread_id, item.seen, item.starred).await;
    match result {
        Ok(()) => {
            // A newer local toggle may have arrived while IMAP was running.
            // In that case its higher revision remains pending for a later run.
            sqlx::query("DELETE FROM mail_flag_operations WHERE id = ? AND revision = ?")
                .bind(&item.id)
                .bind(item.revision)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
        }
        Err(error) => {
            let attempts = item.attempt_count + 1;
            let status = if attempts >= MAX_OPERATION_ATTEMPTS { "failed" } else { "pending" };
            let delay_secs = match attempts { 1 => 10, 2 => 30, _ => 0 };
            let retry_at = (Utc::now() + chrono::Duration::seconds(delay_secs)).to_rfc3339();
            let updated = sqlx::query(
                "UPDATE mail_flag_operations SET status = ?, attempt_count = ?, next_retry_at = ?, last_error = ?, updated_at = ? WHERE id = ? AND revision = ?",
            )
            .bind(status)
            .bind(attempts)
            .bind(retry_at)
            .bind(&error)
            .bind(Utc::now().to_rfc3339())
            .bind(&item.id)
            .bind(item.revision)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
            if status == "failed" && updated.rows_affected() > 0 {
                let _ = app.emit(
                    "mail-operation-failed",
                    MailOperationFailure {
                        thread_id: item.thread_id,
                        operation: "flags".to_string(),
                        error,
                    },
                );
            }
        }
    }
    Ok(())
}

fn start_operation_worker(app: tauri::AppHandle, pool: SqlitePool, worker: MailOperationWorker) {
    tauri::async_runtime::spawn(async move {
        if let Err(error) = process_mail_operations(&app, &pool, &worker).await {
            eprintln!("Unable to process queued Gmail operation: {error}");
        }
    });
}

fn start_delayed_operation_worker(app: tauri::AppHandle, pool: SqlitePool, worker: MailOperationWorker) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(OPERATION_UNDO_WINDOW_SECONDS as u64)).await;
        if let Err(error) = process_mail_operations(&app, &pool, &worker).await {
            eprintln!("Unable to process queued Gmail operation: {error}");
        }
    });
}

#[tauri::command]
pub async fn process_pending_mail_operations(
    app: tauri::AppHandle,
    pool: tauri::State<'_, SqlitePool>,
    worker: tauri::State<'_, MailOperationWorker>,
) -> Result<(), String> {
    process_mail_operations(&app, pool.inner(), worker.inner()).await
}

/// Cancels still-pending intents during the Undo window. Once an operation is
/// in progress, Gmail may already be changing it, so it is deliberately not
/// cancelled here.
#[tauri::command]
pub async fn cancel_mail_operations(
    pool: tauri::State<'_, SqlitePool>,
    thread_ids: Vec<String>,
) -> Result<usize, String> {
    if thread_ids.is_empty() { return Ok(0); }
    let placeholders = std::iter::repeat("?")
        .take(thread_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let query = format!("DELETE FROM mail_operations WHERE status = 'pending' AND thread_id IN ({placeholders})");
    let mut request = sqlx::query(&query);
    for thread_id in thread_ids { request = request.bind(thread_id); }
    let result = request.execute(pool.inner()).await.map_err(|e| e.to_string())?;
    Ok(result.rows_affected() as usize)
}

#[tauri::command]
pub async fn archive_thread(
    app: tauri::AppHandle,
    pool: tauri::State<'_, SqlitePool>,
    worker: tauri::State<'_, MailOperationWorker>,
    thread_id: String,
) -> Result<(), String> {
    enqueue_mail_operation(pool.inner(), &thread_id, "archive").await?;
    start_delayed_operation_worker(app, pool.inner().clone(), worker.inner().clone());
    Ok(())
}

#[tauri::command]
pub async fn delete_thread(
    app: tauri::AppHandle,
    pool: tauri::State<'_, SqlitePool>,
    worker: tauri::State<'_, MailOperationWorker>,
    thread_id: String,
) -> Result<(), String> {
    // UID MOVE to [Gmail]/Trash explicitly puts the message in Gmail Trash.
    enqueue_mail_operation(pool.inner(), &thread_id, "trash").await?;
    start_delayed_operation_worker(app, pool.inner().clone(), worker.inner().clone());
    Ok(())
}

fn parse_from(raw: &str) -> (String, String) {
    if let Some(lt) = raw.find('<') {
        let name = raw[..lt].trim().trim_matches('"').to_string();
        let email = raw[lt + 1..].trim_end_matches('>').to_string();
        (name, email)
    } else {
        (String::new(), raw.trim().to_string())
    }
}

fn parse_date(raw: &str) -> String {
    mailparse::dateparse(raw)
        .map(|ts| {
            chrono::DateTime::from_timestamp(ts, 0)
                .unwrap_or_else(Utc::now)
                .to_rfc3339()
        })
        .unwrap_or_else(|_| Utc::now().to_rfc3339())
}

fn html_visible_content_score(html: &str) -> usize {
    // Email senders sometimes include an empty text/html placeholder before
    // the real rich part. Score visible content rather than accepting the
    // first HTML leaf in the MIME tree. Remove non-visible *blocks* without
    // truncating the email at a <style> in its <head>.
    let without_non_visible = remove_html_blocks(html, "style");
    let without_non_visible = remove_html_blocks(&without_non_visible, "script");
    let lower = without_non_visible.to_ascii_lowercase();
    let visual_element_score = lower.matches("<img").count() * 10;
    let visible: String = without_non_visible
        .chars()
        .scan(false, |inside_tag, c| {
            match c {
                '<' => *inside_tag = true,
                '>' => *inside_tag = false,
                _ => {}
            }
            Some(if *inside_tag || matches!(c, '<' | '>') { ' ' } else { c })
        })
        .filter(|c| !c.is_whitespace() && *c != '\u{00a0}')
        .collect();
    visible.len() + visual_element_score
}

fn remove_html_blocks(html: &str, tag: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let open = format!("<{}", tag);
    let close = format!("</{}", tag);
    let mut result = String::with_capacity(html.len());
    let mut cursor = 0;

    while let Some(relative_start) = lower[cursor..].find(&open) {
        let start = cursor + relative_start;
        result.push_str(&html[cursor..start]);
        let Some(open_end_relative) = lower[start..].find('>') else { break; };
        let after_open = start + open_end_relative + 1;
        let Some(close_relative) = lower[after_open..].find(&close) else { break; };
        let close_start = after_open + close_relative;
        let Some(close_end_relative) = lower[close_start..].find('>') else { break; };
        cursor = close_start + close_end_relative + 1;
    }
    result.push_str(&html[cursor..]);
    result
}

struct HtmlCandidate {
    body: String,
    // Standard HTML is safest to render in our sandbox. AMP/XHTML remains a
    // useful fallback for senders such as LinkedIn that omit text/html.
    is_standard_html: bool,
}

fn decoded_body_lossy(mail: &mailparse::ParsedMail) -> String {
    match mail.get_body() {
        Ok(body) => body,
        Err(error) => {
            // Real marketing and banking mail occasionally declares an invalid
            // charset or transfer encoding. Gmail renders these tolerantly;
            // never turn a recoverable decoding error into an empty email.
            eprintln!(
                "Falling back to lossy MIME decoding for {}: {}",
                mail.ctype.mimetype, error
            );
            mail.get_body_raw()
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .unwrap_or_else(|raw_error| {
                    eprintln!("Unable to recover raw MIME body: {raw_error}");
                    String::new()
                })
        }
    }
}

fn looks_like_html(body: &str) -> bool {
    let prefix = body.get(..2048).unwrap_or(body).to_ascii_lowercase();
    prefix.contains("<!doctype html")
        || prefix.contains("<html")
        || prefix.contains("<body")
        || (prefix.contains("<table") && prefix.contains("</table>"))
}

fn collect_body_parts(
    mail: &mailparse::ParsedMail,
    html_candidates: &mut Vec<HtmlCandidate>,
    text_candidates: &mut Vec<String>,
    has_attachments: &mut bool,
) {
    if !mail.subparts.is_empty() {
        for part in &mail.subparts {
            collect_body_parts(part, html_candidates, text_candidates, has_attachments);
        }
        return;
    }

    let ct = mail.ctype.mimetype.to_lowercase();
    let disposition = mail
        .headers
        .get_first_value("Content-Disposition")
        .unwrap_or_default()
        .to_lowercase();
    // Inline images (for example, a company logo) are intentionally not
    // treated as attachments. Only explicit MIME attachments trigger the
    // conservative review signal.
    if disposition.contains("attachment") {
        *has_attachments = true;
        return;
    }

    let body = decoded_body_lossy(mail);
    let is_standard_html = ct == "text/html";
    let is_supported_rich_html = (ct.starts_with("text/") && ct.ends_with("html"))
        || ct == "application/xhtml+xml"
        // Some bulk senders incorrectly declare HTML as a generic or custom
        // content type. Trust the body shape only after MIME decoding.
        || looks_like_html(&body);
    if is_standard_html || is_supported_rich_html {
        if html_visible_content_score(&body) > 0 {
            html_candidates.push(HtmlCandidate { body, is_standard_html });
        }
    } else if ct == "text/plain" && !body.trim().is_empty() {
        text_candidates.push(body);
    }
}

fn extract_body_parts(mail: &mailparse::ParsedMail) -> (Option<String>, Option<String>, bool) {
    let mut html_candidates = Vec::new();
    let mut text_candidates = Vec::new();
    let mut has_attachments = false;
    collect_body_parts(mail, &mut html_candidates, &mut text_candidates, &mut has_attachments);

    // Prefer a meaningful standard text/html representation whenever present.
    // If a sender only supplies AMP HTML or XHTML, retain the best meaningful
    // alternative instead of degrading immediately to raw plain text.
    let html = html_candidates
        .iter()
        .filter(|candidate| candidate.is_standard_html)
        .max_by_key(|candidate| html_visible_content_score(&candidate.body))
        .or_else(|| html_candidates.iter().max_by_key(|candidate| html_visible_content_score(&candidate.body)))
        .map(|candidate| candidate.body.clone());

    let text = text_candidates
        .into_iter()
        .max_by_key(|body| body.trim().len())
        // Some recruiting systems send HTML only. Keep a derived text version
        // as a durable fallback if a particular sender's rich markup cannot be
        // rendered by the local WebView.
        .or_else(|| html.as_ref().map(|body| html2text::from_read(body.as_bytes(), 100)));

    (html, text, has_attachments)
}

#[cfg(test)]
mod tests {
    use super::html_visible_content_score;

    #[test]
    fn html_with_head_styles_and_body_image_is_meaningful() {
        let html = r#"<html><head><style>body { color: red; }</style></head>
            <body><img src="https://example.test/hero.jpg" alt="Branch closed" /></body></html>"#;
        assert!(html_visible_content_score(html) > 0);
    }

    #[test]
    fn empty_html_shell_is_not_meaningful() {
        assert_eq!(html_visible_content_score("<html><head><style></style></head><body></body></html>"), 0);
    }
}
