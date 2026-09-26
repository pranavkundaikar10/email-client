use chrono::TimeZone;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

const DEFAULT_OLLAMA_URL: &str = "http://localhost:11434";
const DEFAULT_MODEL: &str = "gemma4:e4b";
const MAX_BODY_CHARS: usize = 1800;

#[derive(Debug, Serialize, Deserialize, Clone, FromRow)]
pub struct AnalysisRow {
    pub thread_id: String,
    pub is_actionable: bool,
    pub importance: i64,
    pub category: String,
    pub summary: String,
    pub action_items: String, // JSON array, kept as string like `rules`/`label_ids` elsewhere in this codebase
    pub deadline: Option<String>,
    pub model: String,
    pub analyzed_at: String,
}

/// Enriched row joined with thread metadata, for the digest panel.
#[derive(Debug, Serialize, Deserialize, Clone, FromRow)]
pub struct DigestItem {
    pub thread_id: String,
    pub subject: String,
    pub from_name: String,
    pub from_email: String,
    pub unread: bool,
    pub is_actionable: bool,
    pub importance: i64,
    pub category: String,
    pub summary: String,
    pub action_items: String,
    pub deadline: Option<String>,
}

/// The newest message in a thread that can be safely processed by the
/// low-priority background triage queue.
#[derive(Debug, Serialize, FromRow)]
pub struct AutoAnalysisCandidate {
    pub thread_id: String,
    pub message_id: String,
}

/// An analyzed email waiting for the user's explicit triage decision.
#[derive(Debug, Serialize, FromRow)]
pub struct ReviewItem {
    pub thread_id: String,
    pub id: String,
    pub account_id: String,
    pub subject: String,
    pub snippet: String,
    pub unread: bool,
    pub starred: bool,
    pub archived: bool,
    pub last_message_at: String,
    pub label_ids: String,
    pub folder: String,
    pub from_name: String,
    pub from_email: String,
    pub to_emails: String,
    pub importance: i64,
    pub category: String,
    pub summary: String,
    pub action_items: String,
    pub deadline: Option<String>,
    pub is_actionable: bool,
}

/// What we ask the model to return. `format: "json"` on the Ollama request
/// constrains output to valid JSON; this struct is what we parse it into.
#[derive(Debug, Deserialize)]
struct ModelPayload {
    #[serde(default)]
    is_actionable: bool,
    #[serde(default = "default_importance")]
    importance: i64,
    #[serde(default = "default_category")]
    category: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    action_items: Vec<String>,
    #[serde(default)]
    deadline: Option<String>,
}

fn default_importance() -> i64 { 3 }
fn default_category() -> String { "other".to_string() }

#[derive(Serialize)]
struct OllamaRequest<'a> {
    model: &'a str,
    messages: Vec<OllamaMessage<'a>>,
    format: &'a str,
    stream: bool,
}

#[derive(Serialize)]
struct OllamaMessage<'a> {
    role: &'a str,
    content: String,
}

#[derive(Deserialize)]
struct OllamaResponse {
    message: OllamaResponseMessage,
}

#[derive(Deserialize)]
struct OllamaResponseMessage {
    content: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OllamaModel {
    pub name: String,
    pub size: u64,
    pub modified_at: Option<String>,
}

#[derive(Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModel>,
}

async fn configured_model(pool: &SqlitePool) -> Result<String, String> {
    sqlx::query_scalar("SELECT value FROM app_settings WHERE key = 'ai_model'")
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())
        .map(|model| model.unwrap_or_else(|| DEFAULT_MODEL.to_string()))
}

fn system_prompt() -> &'static str {
    r#"You are an assistant that triages a job-seeker's email inbox. For the
single email given, decide whether it needs action and extract concrete next
steps. Categories: interview, assessment, offer, rejection, application_update,
networking, deadline, newsletter, other.

Respond with ONLY a JSON object, no other text, matching exactly:
{
  "is_actionable": boolean,
  "importance": integer 1-5 (5 = urgent, e.g. interview invite or offer with a deadline; 1 = fluff/newsletter),
  "category": one of the categories above,
  "summary": "one sentence, under 25 words",
  "action_items": ["short imperative next step", "..."],
  "deadline": "YYYY-MM-DD" or null if none stated
}
If the email is a newsletter, marketing, or has nothing to act on, set
is_actionable to false, importance to 1 or 2, and action_items to []."#
}

fn strip_html(html: &str) -> String {
    html2text::from_read(html.as_bytes(), 100)
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    s.chars().take(max_chars).collect::<String>() + "…"
}

fn build_user_prompt(
    from_name: &str,
    from_email: &str,
    subject: &str,
    body: &str,
    has_attachments: bool,
) -> String {
    format!(
        "From: {} <{}>\nSubject: {}\nAttachments: {}\n\nBody:\n{}",
        from_name,
        from_email,
        subject,
        if has_attachments { "yes — content has not been read" } else { "none" },
        truncate(body.trim(), MAX_BODY_CHARS)
    )
}

async fn call_model(
    base_url: &str,
    model: &str,
    user_prompt: String,
) -> Result<ModelPayload, String> {
    let client = reqwest::Client::new();
    let req = OllamaRequest {
        model,
        messages: vec![
            OllamaMessage { role: "system", content: system_prompt().to_string() },
            OllamaMessage { role: "user", content: user_prompt },
        ],
        format: "json",
        stream: false,
    };

    let resp = client
        .post(format!("{}/api/chat", base_url.trim_end_matches('/')))
        .json(&req)
        .send()
        .await
        .map_err(|e| format!("could not reach Ollama at {}: {}", base_url, e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Ollama returned {}: {}", status, body));
    }

    let parsed: OllamaResponse = resp
        .json()
        .await
        .map_err(|e| format!("unexpected Ollama response shape: {}", e))?;

    serde_json::from_str::<ModelPayload>(&parsed.message.content)
        .map_err(|e| format!("model did not return valid JSON ({}): {}", e, parsed.message.content))
}

/// Fetch the most recent message in a thread and reduce it to plain text
/// suitable for prompting, falling back to the thread snippet if no message
/// body has been fetched yet.
async fn latest_message_text(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<(String, String, String, String, bool), String> {
    // (from_name, from_email, subject, body, has_attachments)
    let row: Option<(String, String, String, Option<String>, Option<String>, i64)> = sqlx::query_as(
        r#"
        SELECT from_name, from_email, subject, body_text, body_html, has_attachments
        FROM messages
        WHERE thread_id = ?
        ORDER BY sent_at DESC
        LIMIT 1
        "#,
    )
    .bind(thread_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    let Some((from_name, from_email, subject, body_text, body_html, has_attachments_int)) = row else {
        return Err(format!("no messages found for thread {}", thread_id));
    };

    let body = match (body_text, body_html) {
        (Some(t), _) if !t.trim().is_empty() => t,
        (_, Some(h)) if !h.trim().is_empty() => strip_html(&h),
        _ => {
            let snippet: String = sqlx::query_scalar("SELECT snippet FROM threads WHERE id = ?")
                .bind(thread_id)
                .fetch_optional(pool)
                .await
                .map_err(|e| e.to_string())?
                .unwrap_or_default();
            snippet
        }
    };

    Ok((from_name, from_email, subject, body, has_attachments_int != 0))
}

/// Analyze a single thread and upsert the result into `email_analysis`.
#[tauri::command]
pub async fn analyze_thread(
    pool: tauri::State<'_, SqlitePool>,
    thread_id: String,
    model: Option<String>,
    base_url: Option<String>,
) -> Result<AnalysisRow, String> {
    let model = match model {
        Some(model) => model,
        None => configured_model(pool.inner()).await?,
    };
    let base_url = base_url.unwrap_or_else(|| DEFAULT_OLLAMA_URL.to_string());

    let (from_name, from_email, subject, body, has_attachments) =
        latest_message_text(pool.inner(), &thread_id).await?;

    let prompt = build_user_prompt(&from_name, &from_email, &subject, &body, has_attachments);
    let mut payload = call_model(&base_url, &model, prompt).await?;

    // An attached document may contain the actual assessment, contract, or
    // request. Until attachment extraction exists, never let the model label
    // such a message low-risk or safe to archive without a human review.
    if has_attachments {
        payload.is_actionable = true;
        payload.importance = payload.importance.max(3);
        if !payload.action_items.iter().any(|item| item.contains("attachment")) {
            payload.action_items.insert(0, "Review the attachment before archiving.".to_string());
        }
        payload.summary = if payload.summary.trim().is_empty() {
            "Attachment present — review before archiving.".to_string()
        } else {
            format!("Attachment present — {}", payload.summary)
        };
    }

    let action_items_json =
        serde_json::to_string(&payload.action_items).unwrap_or_else(|_| "[]".to_string());
    let importance = payload.importance.clamp(1, 5);
    let now = chrono::Utc::now().to_rfc3339();

    sqlx::query(
        r#"
        INSERT INTO email_analysis
            (thread_id, is_actionable, importance, category, summary, action_items, deadline, model, analyzed_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(thread_id) DO UPDATE SET
            is_actionable = excluded.is_actionable,
            importance    = excluded.importance,
            category      = excluded.category,
            summary       = excluded.summary,
            action_items  = excluded.action_items,
            deadline      = excluded.deadline,
            model         = excluded.model,
            analyzed_at   = excluded.analyzed_at
        "#,
    )
    .bind(&thread_id)
    .bind(payload.is_actionable)
    .bind(importance)
    .bind(&payload.category)
    .bind(&payload.summary)
    .bind(&action_items_json)
    .bind(&payload.deadline)
    .bind(&model)
    .bind(&now)
    .execute(pool.inner())
    .await
    .map_err(|e| e.to_string())?;

    Ok(AnalysisRow {
        thread_id,
        is_actionable: payload.is_actionable,
        importance,
        category: payload.category,
        summary: payload.summary,
        action_items: action_items_json,
        deadline: payload.deadline,
        model,
        analyzed_at: now,
    })
}

/// List models installed in the local Ollama instance. This never pulls a
/// model or sends mailbox content anywhere.
#[tauri::command]
pub async fn get_ollama_models() -> Result<Vec<OllamaModel>, String> {
    let response = reqwest::Client::new()
        .get(format!("{}/api/tags", DEFAULT_OLLAMA_URL))
        .send()
        .await
        .map_err(|e| format!("could not reach Ollama at {}: {}", DEFAULT_OLLAMA_URL, e))?;

    if !response.status().is_success() {
        return Err(format!("Ollama returned {}", response.status()));
    }

    let mut models = response
        .json::<OllamaTagsResponse>()
        .await
        .map_err(|e| format!("unexpected Ollama model list: {}", e))?
        .models;
    models.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(models)
}

#[tauri::command]
pub async fn get_ai_model(pool: tauri::State<'_, SqlitePool>) -> Result<String, String> {
    configured_model(pool.inner()).await
}

#[tauri::command]
pub async fn set_ai_model(
    pool: tauri::State<'_, SqlitePool>,
    model: String,
) -> Result<(), String> {
    let model = model.trim();
    if model.is_empty() {
        return Err("model name cannot be empty".to_string());
    }

    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES ('ai_model', ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(model)
    .execute(pool.inner())
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// Analyze every unread inbox thread that is new or has changed since it was
/// last analyzed. Runs sequentially (one Ollama call per email) and skips
/// threads that fail rather than aborting the whole batch, so one bad email
/// doesn't block the digest. Returns the threads it (re)analyzed.
#[tauri::command]
pub async fn analyze_inbox(
    pool: tauri::State<'_, SqlitePool>,
    model: Option<String>,
    base_url: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<AnalysisRow>, String> {
    let limit = limit.unwrap_or(25);

    let stale_thread_ids: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT t.id
        FROM threads t
        LEFT JOIN email_analysis a ON a.thread_id = t.id
        WHERE t.folder = 'inbox' AND t.archived = 0 AND t.unread = 1
          AND NOT EXISTS (SELECT 1 FROM mail_operations o WHERE o.thread_id = t.id AND o.status IN ('pending', 'in_progress'))
          AND (a.thread_id IS NULL OR a.analyzed_at < t.last_message_at)
        ORDER BY t.last_message_at DESC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool.inner())
    .await
    .map_err(|e| e.to_string())?;

    let mut results = Vec::new();
    for thread_id in stale_thread_ids {
        match analyze_thread(pool.clone(), thread_id.clone(), model.clone(), base_url.clone()).await {
            Ok(row) => results.push(row),
            Err(e) => {
                // Don't let one unparseable/unreachable email kill the batch.
                eprintln!("analyze_inbox: skipping thread {}: {}", thread_id, e);
            }
        }
    }

    Ok(results)
}

/// Return recent inbox messages that have not yet been analyzed. The client
/// deliberately takes only one at a time so local inference never competes
/// with the UI or starts multiple Ollama requests concurrently.
#[tauri::command]
pub async fn get_auto_analysis_candidates(
    pool: tauri::State<'_, SqlitePool>,
    limit: Option<i64>,
) -> Result<Vec<AutoAnalysisCandidate>, String> {
    let limit = limit.unwrap_or(1).clamp(1, 5);
    let today_start = chrono::Local::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|time| chrono::Local.from_local_datetime(&time).earliest())
        .map(|time| time.with_timezone(&chrono::Utc).to_rfc3339())
        .unwrap_or_else(|| chrono::Utc::now().date_naive().to_string() + "T00:00:00+00:00");

    sqlx::query_as::<_, AutoAnalysisCandidate>(
        r#"
        SELECT t.id AS thread_id, m.id AS message_id
        FROM threads t
        JOIN messages m ON m.thread_id = t.id
            AND m.sent_at = (SELECT MAX(sent_at) FROM messages WHERE thread_id = t.id)
        LEFT JOIN email_analysis a ON a.thread_id = t.id
        WHERE t.folder = 'inbox' AND t.archived = 0
          AND NOT EXISTS (SELECT 1 FROM mail_operations o WHERE o.thread_id = t.id AND o.status IN ('pending', 'in_progress'))
          AND t.last_message_at >= ?
          AND (a.thread_id IS NULL OR a.analyzed_at < t.last_message_at)
        ORDER BY t.last_message_at DESC
        LIMIT ?
        "#,
    )
    .bind(today_start)
    .bind(limit)
    .fetch_all(pool.inner())
    .await
    .map_err(|e| e.to_string())
}

/// The review queue contains today's analyzed inbox threads until the user
/// explicitly keeps, follows up on, or archives them.
#[tauri::command]
pub async fn get_review_queue(
    pool: tauri::State<'_, SqlitePool>,
    limit: Option<i64>,
    sort: Option<String>,
) -> Result<Vec<ReviewItem>, String> {
    let limit = limit.unwrap_or(50).clamp(1, 100);
    let order_by = match sort.as_deref() {
        Some("oldest") => "t.last_message_at ASC",
        Some("newest") => "t.last_message_at DESC",
        _ => "a.importance DESC, t.last_message_at DESC",
    };
    let today_start = chrono::Local::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|time| chrono::Local.from_local_datetime(&time).earliest())
        .map(|time| time.with_timezone(&chrono::Utc).to_rfc3339())
        .unwrap_or_else(|| chrono::Utc::now().date_naive().to_string() + "T00:00:00+00:00");

    let query = r#"
        SELECT a.thread_id, t.id, t.account_id, t.subject, t.snippet, t.unread,
               t.starred, t.archived, t.last_message_at, t.label_ids, t.folder,
               COALESCE(m.from_name, '') AS from_name,
               COALESCE(m.from_email, '') AS from_email,
               COALESCE(m.to_emails, '[]') AS to_emails,
               a.importance, a.category, a.summary, a.action_items,
               a.deadline, a.is_actionable
        FROM email_analysis a
        JOIN threads t ON t.id = a.thread_id
        LEFT JOIN messages m ON m.thread_id = t.id
            AND m.sent_at = (SELECT MAX(sent_at) FROM messages WHERE thread_id = t.id)
        LEFT JOIN email_reviews r ON r.thread_id = t.id
        WHERE t.folder = 'inbox' AND t.archived = 0
          AND NOT EXISTS (SELECT 1 FROM mail_operations o WHERE o.thread_id = t.id AND o.status IN ('pending', 'in_progress'))
          AND t.last_message_at >= ? AND r.thread_id IS NULL
        ORDER BY {order_by}
        LIMIT ?
        "#
        .replace("{order_by}", order_by);

    sqlx::query_as::<_, ReviewItem>(&query)
    .bind(today_start)
    .bind(limit)
    .fetch_all(pool.inner())
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn record_review_decision(
    pool: tauri::State<'_, SqlitePool>,
    thread_id: String,
    decision: String,
) -> Result<(), String> {
    if !matches!(decision.as_str(), "keep" | "follow_up" | "archived") {
        return Err("invalid review decision".to_string());
    }

    sqlx::query(
        r#"
        INSERT INTO email_reviews (thread_id, decision, reviewed_at)
        VALUES (?, ?, ?)
        ON CONFLICT(thread_id) DO UPDATE SET
            decision = excluded.decision,
            reviewed_at = excluded.reviewed_at
        "#,
    )
    .bind(thread_id)
    .bind(decision)
    .bind(chrono::Utc::now().to_rfc3339())
    .execute(pool.inner())
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// Digest for the panel: analyzed, actionable-first, most important first,
/// soonest deadline first.
#[tauri::command]
pub async fn get_digest(
    pool: tauri::State<'_, SqlitePool>,
    limit: Option<i64>,
) -> Result<Vec<DigestItem>, String> {
    let limit = limit.unwrap_or(50);

    sqlx::query_as::<_, DigestItem>(
        r#"
        SELECT
            a.thread_id, t.subject,
            COALESCE(m.from_name, '') AS from_name,
            COALESCE(m.from_email, '') AS from_email,
            t.unread,
            a.is_actionable, a.importance, a.category, a.summary, a.action_items, a.deadline
        FROM email_analysis a
        JOIN threads t ON t.id = a.thread_id
        LEFT JOIN messages m ON m.thread_id = t.id
            AND m.sent_at = (SELECT MAX(sent_at) FROM messages WHERE thread_id = t.id)
        WHERE t.folder = 'inbox' AND t.archived = 0
          AND NOT EXISTS (SELECT 1 FROM mail_operations o WHERE o.thread_id = t.id AND o.status IN ('pending', 'in_progress'))
        ORDER BY
            a.is_actionable DESC,
            a.importance DESC,
            CASE WHEN a.deadline IS NULL THEN 1 ELSE 0 END,
            a.deadline ASC,
            t.last_message_at DESC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool.inner())
    .await
    .map_err(|e| e.to_string())
}

/// Single-thread analysis lookup, for the per-thread badge in the preview pane.
#[tauri::command]
pub async fn get_thread_analysis(
    pool: tauri::State<'_, SqlitePool>,
    thread_id: String,
) -> Result<Option<AnalysisRow>, String> {
    sqlx::query_as::<_, AnalysisRow>("SELECT * FROM email_analysis WHERE thread_id = ?")
        .bind(thread_id)
        .fetch_optional(pool.inner())
        .await
        .map_err(|e| e.to_string())
}
