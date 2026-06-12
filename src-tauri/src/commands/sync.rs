use imap::Session;
use mailparse::{parse_mail, MailHeaderMap};
use native_tls::TlsConnector;
use sqlx::SqlitePool;
use std::net::TcpStream;
use chrono::Utc;
use chrono::NaiveDateTime;

type ImapSession = Session<native_tls::TlsStream<TcpStream>>;

const GMAIL_IMAP_HOST: &str = "imap.gmail.com";
const GMAIL_IMAP_PORT: u16 = 993;
const SYNC_DAYS: i64 = 365;
const SYNC_LIMIT: usize = 2000;

struct MessageMeta {
    imap_uid: String,
    message_id: String,
    thread_id: String,
    subject: String,
    snippet: String,
    from_name: String,
    from_email: String,
    to_emails: String, // JSON array of raw address strings
    sent_at: String,
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

    let fetch_items = "(FLAGS BODY.PEEK[HEADER.FIELDS (FROM TO CC SUBJECT DATE MESSAGE-ID REFERENCES IN-REPLY-TO LIST-UNSUBSCRIBE LIST-ID PRECEDENCE)])";
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
        let sent_at = parse_date(&date_raw);

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
            from_name, from_email, to_emails, sent_at, unread, starred,
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

    let fetch_items = "(FLAGS BODY.PEEK[HEADER.FIELDS (FROM TO CC SUBJECT DATE MESSAGE-ID REFERENCES IN-REPLY-TO LIST-UNSUBSCRIBE LIST-ID PRECEDENCE)])";
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
        .bind(m.unread as i64).bind(m.starred as i64).bind(&m.sent_at).bind(&category).bind(folder)
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
                to_emails = CASE WHEN to_emails = '' OR to_emails = '[]' THEN excluded.to_emails ELSE to_emails END
            "#,
        )
        .bind(&m.message_id).bind(&m.thread_id).bind(email)
        .bind(&m.from_email).bind(&m.from_name).bind(&m.to_emails).bind(&m.subject).bind(&m.sent_at)
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

    if body_fetched {
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

    let (body_html, body_text) = tokio::task::spawn_blocking(move || {
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
        "UPDATE messages SET body_html = ?, body_text = ?, body_fetched = 1 WHERE id = ?",
    )
    .bind(&body_html)
    .bind(&body_text)
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

    // Sync \Seen flag to Gmail in the background — don't block the response
    let account_id: Option<String> =
        sqlx::query_scalar("SELECT account_id FROM threads WHERE id = ?")
            .bind(&thread_id)
            .fetch_optional(pool.inner())
            .await
            .unwrap_or(None);

    if let Some(account_id) = account_id {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, imap_uid FROM messages WHERE thread_id = ?",
        )
        .bind(&thread_id)
        .fetch_all(pool.inner())
        .await
        .unwrap_or_default();

        let folder: String = sqlx::query_scalar("SELECT folder FROM threads WHERE id = ?")
            .bind(&thread_id)
            .fetch_optional(pool.inner())
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| "inbox".to_string());

        if let Ok(password) = crate::commands::auth::load_password(&app, &account_id) {
            let email = account_id.clone();
            tokio::spawn(async move {
                let _ = tokio::task::spawn_blocking(move || {
                    let mut session = connect(&email, &password)?;
                    let mailbox = match folder.as_str() {
                        "sent"   => find_special_mailbox(&mut session, "\\Sent")
                                        .unwrap_or_else(|| "[Gmail]/Sent Mail".to_string()),
                        "drafts" => find_special_mailbox(&mut session, "\\Drafts")
                                        .unwrap_or_else(|| "[Gmail]/Drafts".to_string()),
                        _        => "INBOX".to_string(),
                    };
                    session.select(&mailbox).map_err(|e| e.to_string())?;

                    let mut uids: Vec<String> = rows.iter()
                        .filter(|(_, u)| !u.is_empty())
                        .map(|(_, u)| u.clone())
                        .collect();

                    for (msg_id, uid) in &rows {
                        if uid.is_empty() {
                            if let Ok(found) = session.uid_search(
                                format!("HEADER MESSAGE-ID \"{}\"", msg_id)
                            ) {
                                uids.extend(found.into_iter().map(|u| u.to_string()));
                            }
                        }
                    }

                    if !uids.is_empty() {
                        let _ = session.uid_store(&uids.join(","), "+FLAGS.SILENT (\\Seen)");
                    }
                    session.logout().ok();
                    Ok::<_, String>(())
                })
                .await;
            });
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn star_thread(
    app: tauri::AppHandle,
    pool: tauri::State<'_, SqlitePool>,
    thread_id: String,
    starred: bool,
) -> Result<(), String> {
    sqlx::query("UPDATE threads SET starred = ? WHERE id = ?")
        .bind(starred as i64)
        .bind(&thread_id)
        .execute(pool.inner())
        .await
        .map_err(|e| e.to_string())?;

    let account_id: Option<String> =
        sqlx::query_scalar("SELECT account_id FROM threads WHERE id = ?")
            .bind(&thread_id)
            .fetch_optional(pool.inner())
            .await
            .unwrap_or(None);

    if let Some(account_id) = account_id {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, imap_uid FROM messages WHERE thread_id = ?",
        )
        .bind(&thread_id)
        .fetch_all(pool.inner())
        .await
        .unwrap_or_default();

        let folder: String = sqlx::query_scalar("SELECT folder FROM threads WHERE id = ?")
            .bind(&thread_id)
            .fetch_optional(pool.inner())
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| "inbox".to_string());

        if let Ok(password) = crate::commands::auth::load_password(&app, &account_id) {
            let email = account_id.clone();
            tokio::spawn(async move {
                let _ = tokio::task::spawn_blocking(move || {
                    let mut session = connect(&email, &password)?;
                    let mailbox = match folder.as_str() {
                        "sent"   => find_special_mailbox(&mut session, "\\Sent")
                                        .unwrap_or_else(|| "[Gmail]/Sent Mail".to_string()),
                        "drafts" => find_special_mailbox(&mut session, "\\Drafts")
                                        .unwrap_or_else(|| "[Gmail]/Drafts".to_string()),
                        _        => "INBOX".to_string(),
                    };
                    session.select(&mailbox).map_err(|e| e.to_string())?;
                    let uids = resolve_uids(&mut session, &rows)?;
                    if !uids.is_empty() {
                        let flag_op = if starred {
                            "+FLAGS.SILENT (\\Flagged)"
                        } else {
                            "-FLAGS.SILENT (\\Flagged)"
                        };
                        let _ = session.uid_store(&uids.join(","), flag_op);
                    }
                    session.logout().ok();
                    Ok::<_, String>(())
                })
                .await;
            });
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn archive_thread(
    app: tauri::AppHandle,
    pool: tauri::State<'_, SqlitePool>,
    thread_id: String,
) -> Result<(), String> {
    imap_move_thread(&app, pool.inner(), &thread_id, "\\All", "[Gmail]/All Mail").await?;

    sqlx::query("UPDATE threads SET archived = 1 WHERE id = ?")
        .bind(&thread_id)
        .execute(pool.inner())
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn delete_thread(
    app: tauri::AppHandle,
    pool: tauri::State<'_, SqlitePool>,
    thread_id: String,
) -> Result<(), String> {
    // UID MOVE to [Gmail]/Trash explicitly puts the message in Trash.
    // uid_store+expunge was stripping the INBOX label only (archiving), not trashing.
    imap_move_thread(&app, pool.inner(), &thread_id, "\\Trash", "[Gmail]/Trash").await?;

    sqlx::query("UPDATE threads SET archived = 1 WHERE id = ?")
        .bind(&thread_id)
        .execute(pool.inner())
        .await
        .map_err(|e| e.to_string())?;

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

fn extract_body_parts(mail: &mailparse::ParsedMail) -> (Option<String>, Option<String>) {
    let mut html: Option<String> = None;
    let mut text: Option<String> = None;

    if mail.subparts.is_empty() {
        let ct = mail.ctype.mimetype.to_lowercase();
        let body = mail.get_body().unwrap_or_default();
        if ct.contains("html") {
            html = Some(body);
        } else {
            text = Some(body);
        }
        return (html, text);
    }

    for part in &mail.subparts {
        let (h, t) = extract_body_parts(part);
        if html.is_none() { html = h; }
        if text.is_none() { text = t; }
    }

    (html, text)
}
