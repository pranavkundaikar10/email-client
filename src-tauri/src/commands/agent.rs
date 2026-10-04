use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

const DEFAULT_OLLAMA_URL: &str = "http://localhost:11434";
const DEFAULT_MODEL: &str = "gemma4:e4b";
const MAX_BODY_CHARS: usize = 1800;
const MAX_TRIAGE_PREFERENCES_CHARS: usize = 1000;

#[derive(Debug, Serialize, Deserialize, Clone, FromRow)]
pub struct AnalysisRow {
    pub thread_id: String,
    pub is_actionable: bool,
    pub importance: i64,
    pub category: String,
    pub is_job_related: bool,
    pub job_category: Option<String>,
    pub summary: String,
    pub action_items: String, // JSON array, kept as string like `rules`/`label_ids` elsewhere in this codebase
    pub deadline: Option<String>,
    pub recommended_action: String,
    pub calendar_event: Option<String>,
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
    pub message_received_at: String,
}

/// User-controlled thinking behavior for the two analysis workloads. Manual
/// analysis favors depth by default; the background review queue favors a
/// responsive, low-impact laptop experience by default.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ThinkingSettings {
    pub manual: bool,
    pub background: bool,
}

/// Master control for all local-model work. It deliberately does not erase
/// prior analysis; it only prevents new automatic or manual requests.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct AiAssistanceSettings {
    pub enabled: bool,
}

impl Default for AiAssistanceSettings {
    fn default() -> Self {
        // Preserve the behaviour people already have after upgrading. New
        // installations without Ollama can turn this off from Settings.
        Self { enabled: true }
    }
}

impl Default for ThinkingSettings {
    fn default() -> Self {
        Self { manual: true, background: false }
    }
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
    pub recommended_action: String,
    pub analysis_available: bool,
}

/// A locally scheduled follow-up attached to an inbox thread. It never moves
/// or modifies the corresponding Gmail message by itself.
#[derive(Debug, Serialize, FromRow)]
pub struct FollowUpItem {
    pub thread_id: String,
    pub due_at: String,
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
    pub category: String,
    pub analysis_importance: Option<i64>,
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
    is_job_related: bool,
    #[serde(default)]
    job_category: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    action_items: Vec<String>,
    #[serde(default)]
    deadline: Option<String>,
    #[serde(default = "default_recommended_action")]
    recommended_action: String,
    #[serde(default)]
    calendar_event: Option<RawCalendarEvent>,
}

/// A provider-neutral, user-reviewable event candidate. It is deliberately
/// data only: Google, Outlook, Apple Calendar, and ICS export are adapters.
#[derive(Debug, Serialize, Deserialize, Clone)]
struct RawCalendarEvent {
    title: String,
    date_clues: Vec<String>,
    time_text: String,
    timezone_text: String,
    duration_minutes: Option<i64>,
    #[serde(default)]
    location: String,
    #[serde(default)]
    description: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CalendarEventDraft {
    title: String,
    start_at: String,
    end_at: String,
    location: String,
    description: String,
}

fn default_importance() -> i64 { 3 }
fn default_category() -> String { "other".to_string() }
fn default_recommended_action() -> String { "review".to_string() }

#[derive(Serialize)]
struct OllamaRequest<'a> {
    model: &'a str,
    messages: Vec<OllamaMessage<'a>>,
    format: &'a str,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    think: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
}

#[derive(Serialize)]
struct OllamaOptions {
    num_predict: usize,
    temperature: f32,
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

async fn configured_triage_preferences(pool: &SqlitePool) -> Result<String, String> {
    sqlx::query_scalar("SELECT value FROM app_settings WHERE key = 'triage_preferences'")
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())
        .map(|preferences| preferences.unwrap_or_default())
}

async fn configured_thinking_settings(pool: &SqlitePool) -> Result<ThinkingSettings, String> {
    let stored: Option<String> = sqlx::query_scalar(
        "SELECT value FROM app_settings WHERE key = 'ai_thinking_settings'",
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(stored
        .and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_default())
}

async fn configured_ai_assistance(pool: &SqlitePool) -> Result<AiAssistanceSettings, String> {
    let stored: Option<String> = sqlx::query_scalar(
        "SELECT value FROM app_settings WHERE key = 'ai_assistance_settings'",
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(stored
        .and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_default())
}

fn system_prompt(triage_preferences: &str, include_calendar_event: bool) -> String {
    let calendar_schema = if include_calendar_event {
        ",\n  \"calendar_event\": {\"title\":\"...\",\"date_clues\":[\"exact words from email\"],\"time_text\":\"exact time\",\"timezone_text\":\"exact timezone\",\"duration_minutes\":number,\"location\":\"...\"} or null"
    } else {
        ""
    };
    let mut prompt = r#"You are an assistant that triages a job-seeker's email inbox. For the
single email given, decide whether it needs action and extract concrete next
steps. General triage categories: interview, assessment, offer, rejection,
application_update, networking, deadline, newsletter, other.

Also classify whether the email is directly related to the recipient's active
job search. Job-related means a specific role, application, hiring process, or
recruiter outreach. For job-related email choose exactly one job_category:
confirmation (application received/submitted), rejection, assessment,
screening (recruiter outreach, phone screen, or scheduling), interview, offer,
or other. Set is_job_related to false and job_category to an empty string for
all other mail, including receipts, banking, generic marketing, and unrelated
newsletters. Do not infer job-relatedness merely from the sender's company.

Respond with ONLY a JSON object, no other text, matching exactly:
{
  "is_actionable": boolean,
  "importance": integer 1-5 (5 = urgent, e.g. interview invite or offer with a deadline; 1 = fluff/newsletter),
  "category": one of the categories above,
  "is_job_related": boolean,
  "job_category": "confirmation|rejection|assessment|screening|interview|offer|other" or "",
  "summary": "one sentence, under 25 words",
  "action_items": ["short imperative next step", "..."],
  "deadline": "YYYY-MM-DD" or null if none stated,
  "recommended_action": "keep|follow_up|archive|delete|review"{calendar_schema}
}
If the email is a newsletter, marketing, or has nothing to act on, set
is_actionable to false, importance to 1 or 2, and action_items to []. Choose
delete only for an unmistakably promotional or marketing email when the user's
preferences support it; otherwise prefer archive for low-value email. Never
recommend delete for an email with an attachment. Use review when uncertain.
Provide calendar_event for any concrete event that explicitly states a date,
time, timezone or offset, and duration/end time. This is an optional “Event
mentioned” suggestion for the user; it does not mean the recipient registered
or must attend. Never invent a time, duration, attendees, or meeting link; use
null when any required detail is uncertain."#.replace("{calendar_schema}", calendar_schema);

    if !triage_preferences.trim().is_empty() {
        prompt.push_str(
            "\n\nUser triage preferences (use these to guide prioritization, but do not change the JSON schema, categories, attachment safeguards, or job-relatedness rules):\n",
        );
        prompt.push_str(triage_preferences.trim());
    }
    prompt
}

fn normalized_job_category(is_job_related: bool, category: &str) -> Option<String> {
    if !is_job_related {
        return None;
    }
    let category = category.trim().to_ascii_lowercase();
    match category.as_str() {
        "confirmation" | "rejection" | "assessment" | "screening" | "interview" | "offer" | "other" => Some(category),
        _ => Some("other".to_string()),
    }
}

fn normalized_recommended_action(action: &str) -> String {
    match action.trim().to_ascii_lowercase().as_str() {
        "keep" | "follow_up" | "archive" | "delete" | "review" => action.trim().to_ascii_lowercase(),
        _ => "review".to_string(),
    }
}

fn source_contains(body: &str, value: &str) -> bool {
    let normalize = |value: &str| value.to_ascii_lowercase().chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>();
    let value = normalize(value);
    !value.is_empty() && normalize(body).contains(&value)
}

fn resolved_calendar_event(event: Option<RawCalendarEvent>, received_at: &str, body: &str) -> Option<CalendarEventDraft> {
    let event = event?;
    if event.title.trim().is_empty() { return None; }
    if event.date_clues.iter().any(|clue| !source_contains(body, clue))
        || !source_contains(body, &event.time_text)
        || !source_contains(body, &event.timezone_text) {
        return None;
    }
    let received_at = chrono::DateTime::parse_from_rfc3339(received_at).ok()?.with_timezone(&chrono::Utc);
    let clues = event.date_clues.iter().map(String::as_str).collect::<Vec<_>>();
    let resolved = crate::calendar_resolver::resolve_free_text_event(
        received_at, &clues, Some(&event.time_text), Some(&event.timezone_text), event.duration_minutes,
    ).ok()?;
    Some(CalendarEventDraft {
        title: truncate(event.title.trim(), 160),
        start_at: resolved.start_at, end_at: resolved.end_at,
        location: truncate(event.location.trim(), 500),
        description: truncate(event.description.trim(), 2_000),
    })
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
    system_instruction: String,
    user_prompt: String,
    think: Option<bool>,
) -> Result<ModelPayload, String> {
    let client = reqwest::Client::new();
    let req = OllamaRequest {
        model,
        messages: vec![
            OllamaMessage { role: "system", content: system_instruction },
            OllamaMessage { role: "user", content: user_prompt },
        ],
        format: "json",
        stream: false,
        think,
        // When thinking is disabled this cap applies to the final JSON response,
        // preventing a malformed or overly verbose reply from monopolizing
        // the local model.
        options: think.map(|thinking_enabled| (!thinking_enabled).then_some(OllamaOptions {
            num_predict: 320,
            temperature: 0.0,
        })).flatten(),
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

fn should_retry_without_calendar(error: &str) -> bool {
    error.starts_with("model did not return valid JSON")
}

/// Fetch the most recent message in a thread and reduce it to plain text
/// suitable for prompting, falling back to the thread snippet if no message
/// body has been fetched yet.
async fn latest_message_text(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<(String, String, String, String, bool, String), String> {
    // (from_name, from_email, subject, body, has_attachments)
    let row: Option<(String, String, String, Option<String>, Option<String>, i64, String)> = sqlx::query_as(
        r#"
        SELECT from_name, from_email, subject, body_text, body_html, has_attachments, sent_at
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

    let Some((from_name, from_email, subject, body_text, body_html, has_attachments_int, received_at)) = row else {
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

    Ok((from_name, from_email, subject, body, has_attachments_int != 0, received_at))
}

/// Analyze a single thread and upsert the result into `email_analysis`.
#[tauri::command]
pub async fn analyze_thread(
    pool: tauri::State<'_, SqlitePool>,
    thread_id: String,
    model: Option<String>,
    base_url: Option<String>,
    analysis_mode: Option<String>,
) -> Result<AnalysisRow, String> {
    analyze_thread_with_pool(
        pool.inner(),
        &thread_id,
        model,
        base_url,
        analysis_mode.as_deref(),
    ).await
}

/// Shared manual/background analysis implementation. Callers only choose the
/// mode; policy, attachment protection, model settings, and persistence stay
/// identical across both entry points.
pub(crate) async fn analyze_thread_with_pool(
    pool: &SqlitePool,
    thread_id: &str,
    model: Option<String>,
    base_url: Option<String>,
    analysis_mode: Option<&str>,
) -> Result<AnalysisRow, String> {
    if !configured_ai_assistance(pool).await?.enabled {
        return Err("AI assistance is turned off in Settings".to_string());
    }
    let model = match model {
        Some(model) => model,
        None => configured_model(pool).await?,
    };
    let base_url = base_url.unwrap_or_else(|| DEFAULT_OLLAMA_URL.to_string());
    let triage_preferences = configured_triage_preferences(pool).await?;
    let thinking_settings = configured_thinking_settings(pool).await?;
    let think = match analysis_mode {
        Some("background") => thinking_settings.background,
        Some("manual") | None => thinking_settings.manual,
        Some(_) => return Err("invalid analysis mode".to_string()),
    };

    let (from_name, from_email, subject, body, has_attachments, received_at) =
        latest_message_text(pool, thread_id).await?;

    let prompt = build_user_prompt(&from_name, &from_email, &subject, &body, has_attachments);
    let mut payload = match call_model(
        &base_url,
        &model,
        system_prompt(&triage_preferences, true),
        prompt.clone(),
        Some(think),
    )
    .await {
        Ok(payload) => payload,
        // A calendar candidate must never make ordinary triage unusable. One
        // compact retry removes only that optional field when output was cut
        // off before valid JSON could be returned.
        Err(error) if should_retry_without_calendar(&error) => call_model(
            &base_url,
            &model,
            system_prompt(&triage_preferences, false),
            prompt,
            Some(think),
        ).await?,
        Err(error) => return Err(error),
    };

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
    let job_category = normalized_job_category(payload.is_job_related, &payload.job_category);
    let recommended_action = if has_attachments {
        "review".to_string()
    } else {
        normalized_recommended_action(&payload.recommended_action)
    };
    let inferred_calendar = crate::calendar_resolver::extract_common_event_hints(&body).map(|hints| RawCalendarEvent {
            title: subject.clone(),
            date_clues: hints.date_clues,
            time_text: hints.time,
            timezone_text: hints.timezone,
            duration_minutes: Some(hints.duration_minutes),
            location: String::new(),
            description: String::new(),
        });
    let calendar_event = resolved_calendar_event(payload.calendar_event, &received_at, &body)
        .or_else(|| resolved_calendar_event(inferred_calendar, &received_at, &body));
    let calendar_event_json = calendar_event.as_ref().and_then(|event| serde_json::to_string(event).ok());
    let now = chrono::Utc::now().to_rfc3339();

    sqlx::query(
        r#"
        INSERT INTO email_analysis
            (thread_id, is_actionable, importance, category, is_job_related, job_category, summary, action_items, deadline, recommended_action, calendar_event, model, analyzed_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(thread_id) DO UPDATE SET
            is_actionable = excluded.is_actionable,
            importance    = excluded.importance,
            category      = excluded.category,
            is_job_related = excluded.is_job_related,
            job_category  = excluded.job_category,
            summary       = excluded.summary,
            action_items  = excluded.action_items,
            deadline      = excluded.deadline,
            recommended_action = excluded.recommended_action,
            calendar_event = excluded.calendar_event,
            model         = excluded.model,
            analyzed_at   = excluded.analyzed_at
        "#,
    )
    .bind(thread_id)
    .bind(payload.is_actionable)
    .bind(importance)
    .bind(&payload.category)
    .bind(payload.is_job_related)
    .bind(&job_category)
    .bind(&payload.summary)
    .bind(&action_items_json)
    .bind(&payload.deadline)
    .bind(&recommended_action)
    .bind(&calendar_event_json)
    .bind(&model)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(AnalysisRow {
        thread_id: thread_id.to_string(),
        is_actionable: payload.is_actionable,
        importance,
        category: payload.category,
        is_job_related: payload.is_job_related,
        job_category,
        summary: payload.summary,
        action_items: action_items_json,
        deadline: payload.deadline,
        recommended_action,
        calendar_event: calendar_event_json,
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

#[tauri::command]
pub async fn get_ai_assistance_settings(
    pool: tauri::State<'_, SqlitePool>,
) -> Result<AiAssistanceSettings, String> {
    configured_ai_assistance(pool.inner()).await
}

#[tauri::command]
pub async fn set_ai_assistance_settings(
    pool: tauri::State<'_, SqlitePool>,
    settings: AiAssistanceSettings,
) -> Result<(), String> {
    let value = serde_json::to_string(&settings).map_err(|e| e.to_string())?;
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES ('ai_assistance_settings', ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(value)
    .execute(pool.inner())
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn get_triage_preferences(pool: tauri::State<'_, SqlitePool>) -> Result<String, String> {
    configured_triage_preferences(pool.inner()).await
}

#[tauri::command]
pub async fn set_triage_preferences(
    pool: tauri::State<'_, SqlitePool>,
    preferences: String,
) -> Result<(), String> {
    let preferences = preferences.trim();
    if preferences.chars().count() > MAX_TRIAGE_PREFERENCES_CHARS {
        return Err(format!("Triage preferences must be at most {MAX_TRIAGE_PREFERENCES_CHARS} characters"));
    }
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES ('triage_preferences', ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(preferences)
    .execute(pool.inner())
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn get_thinking_settings(
    pool: tauri::State<'_, SqlitePool>,
) -> Result<ThinkingSettings, String> {
    configured_thinking_settings(pool.inner()).await
}

#[tauri::command]
pub async fn set_thinking_settings(
    pool: tauri::State<'_, SqlitePool>,
    settings: ThinkingSettings,
) -> Result<(), String> {
    let value = serde_json::to_string(&settings).map_err(|e| e.to_string())?;
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES ('ai_thinking_settings', ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(value)
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
    analysis_mode: Option<String>,
) -> Result<Vec<AnalysisRow>, String> {
    if !configured_ai_assistance(pool.inner()).await?.enabled {
        return Ok(Vec::new());
    }
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
        match analyze_thread(pool.clone(), thread_id.clone(), model.clone(), base_url.clone(), analysis_mode.clone()).await {
            Ok(row) => results.push(row),
            Err(e) => {
                // Don't let one unparseable/unreachable email kill the batch.
                eprintln!("analyze_inbox: skipping thread {}: {}", thread_id, e);
            }
        }
    }

    Ok(results)
}

/// Return recent inbox messages that have not yet been analyzed. The worker
/// takes one at a time so local inference never competes with the UI or starts
/// multiple Ollama requests concurrently.
#[tauri::command]
pub async fn get_auto_analysis_candidates(
    pool: tauri::State<'_, SqlitePool>,
    limit: Option<i64>,
) -> Result<Vec<AutoAnalysisCandidate>, String> {
    background_analysis_candidates(pool.inner(), limit.unwrap_or(1), Utc::now()).await
}

/// Candidate selection for the low-impact background worker. This is kept
/// separate from the Tauri command so the eventual worker and its tests use
/// the exact same account-independent eligibility rules.
pub(crate) async fn background_analysis_candidates(
    pool: &SqlitePool,
    limit: i64,
    now: DateTime<Utc>,
) -> Result<Vec<AutoAnalysisCandidate>, String> {
    if !configured_ai_assistance(pool).await?.enabled {
        return Ok(Vec::new());
    }
    let limit = limit.clamp(1, 5);
    let today_start = now.with_timezone(&chrono::Local)
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|time| chrono::Local.from_local_datetime(&time).earliest())
        .map(|time| time.with_timezone(&chrono::Utc).to_rfc3339())
        .unwrap_or_else(|| chrono::Utc::now().date_naive().to_string() + "T00:00:00+00:00");

    sqlx::query_as::<_, AutoAnalysisCandidate>(
        r#"
        SELECT t.id AS thread_id, m.id AS message_id, t.last_message_at AS message_received_at
        FROM threads t
        JOIN messages m ON m.thread_id = t.id
            AND m.sent_at = (SELECT MAX(sent_at) FROM messages WHERE thread_id = t.id)
        LEFT JOIN email_analysis a ON a.thread_id = t.id
        LEFT JOIN background_triage_jobs j ON j.thread_id = t.id
        WHERE t.folder = 'inbox' AND t.archived = 0
          AND NOT EXISTS (SELECT 1 FROM mail_operations o WHERE o.thread_id = t.id AND o.status IN ('pending', 'in_progress'))
          AND t.last_message_at >= ?
          AND (a.thread_id IS NULL OR a.analyzed_at < t.last_message_at)
          AND (j.thread_id IS NULL OR j.status != 'failed' OR j.message_id != m.id)
        ORDER BY t.last_message_at DESC
        LIMIT ?
        "#,
    )
    .bind(today_start)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())
}

pub(crate) async fn background_triage_enabled(pool: &SqlitePool) -> Result<bool, String> {
    Ok(configured_ai_assistance(pool).await?.enabled)
}

/// Count the same received-today inbox threads eligible for background
/// analysis. The Review UI uses this to distinguish an empty queue from one
/// that is still being prepared by the local model.
#[tauri::command]
pub async fn get_auto_analysis_pending_count(
    pool: tauri::State<'_, SqlitePool>,
) -> Result<i64, String> {
    background_analysis_pending_count(pool.inner(), Utc::now()).await
}

async fn background_analysis_pending_count(pool: &SqlitePool, now: DateTime<Utc>) -> Result<i64, String> {
    if !configured_ai_assistance(pool).await?.enabled {
        return Ok(0);
    }
    let today_start = now.with_timezone(&chrono::Local)
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|time| chrono::Local.from_local_datetime(&time).earliest())
        .map(|time| time.with_timezone(&chrono::Utc).to_rfc3339())
        .unwrap_or_else(|| chrono::Utc::now().date_naive().to_string() + "T00:00:00+00:00");

    sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM threads t
        LEFT JOIN email_analysis a ON a.thread_id = t.id
        LEFT JOIN background_triage_jobs j ON j.thread_id = t.id
        WHERE t.folder = 'inbox' AND t.archived = 0
          AND NOT EXISTS (SELECT 1 FROM mail_operations o WHERE o.thread_id = t.id AND o.status IN ('pending', 'in_progress'))
          AND t.last_message_at >= ?
          AND (a.thread_id IS NULL OR a.analyzed_at < t.last_message_at)
          AND (j.thread_id IS NULL OR j.status != 'failed' OR j.message_id != (
              SELECT m.id FROM messages m WHERE m.thread_id = t.id ORDER BY m.sent_at DESC LIMIT 1
          ))
        "#,
    )
    .bind(today_start)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())
}

/// The review queue shows the newest seven-day block with unresolved analyzed
/// inbox threads. Once it is cleared, it automatically moves to the next older
/// block without causing the background analyzer to process old mail.
#[tauri::command]
pub async fn get_review_queue(
    pool: tauri::State<'_, SqlitePool>,
    limit: Option<i64>,
    sort: Option<String>,
) -> Result<Vec<ReviewItem>, String> {
    let ai_assistance_enabled = configured_ai_assistance(pool.inner()).await?.enabled;
    let limit = limit.unwrap_or(50).clamp(1, 100);
    let order_by = match sort.as_deref() {
        Some("oldest") => "t.last_message_at ASC",
        Some("newest") => "t.last_message_at DESC",
        _ if ai_assistance_enabled => "a.importance DESC, t.last_message_at DESC",
        _ => "t.last_message_at DESC",
    };
    let mut review_window_start = (chrono::Local::now() - chrono::Duration::days(6))
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|time| chrono::Local.from_local_datetime(&time).earliest())
        .map(|time| time.with_timezone(&chrono::Utc).to_rfc3339())
        .unwrap_or_else(|| chrono::Utc::now().date_naive().to_string() + "T00:00:00+00:00");

    let query = r#"
        SELECT t.id AS thread_id, t.id, t.account_id, t.subject, t.snippet, t.unread,
               t.starred, t.archived, t.last_message_at, t.label_ids, t.folder,
               COALESCE(m.from_name, '') AS from_name,
               COALESCE(m.from_email, '') AS from_email,
               COALESCE(m.to_emails, '[]') AS to_emails,
               COALESCE(a.importance, 0) AS importance,
               COALESCE(a.category, '') AS category,
               COALESCE(a.summary, '') AS summary,
               COALESCE(a.action_items, '[]') AS action_items,
               a.deadline, COALESCE(a.is_actionable, 0) AS is_actionable,
               COALESCE(a.recommended_action, 'review') AS recommended_action,
               a.thread_id IS NOT NULL AS analysis_available
        FROM threads t
        LEFT JOIN email_analysis a ON a.thread_id = t.id
        LEFT JOIN messages m ON m.thread_id = t.id
            AND m.sent_at = (SELECT MAX(sent_at) FROM messages WHERE thread_id = t.id)
        LEFT JOIN email_reviews r ON r.thread_id = t.id
        WHERE t.folder = 'inbox' AND t.archived = 0
          AND NOT EXISTS (SELECT 1 FROM mail_operations o WHERE o.thread_id = t.id AND o.status IN ('pending', 'in_progress'))
          AND t.last_message_at >= ? AND t.last_message_at < ?
          AND r.thread_id IS NULL
          AND (? = 0 OR a.thread_id IS NOT NULL)
        ORDER BY {order_by}
        LIMIT ?
        "#
        .replace("{order_by}", order_by);

    let mut review_window_end = (chrono::Utc::now() + chrono::Duration::seconds(1)).to_rfc3339();
    // At most five years of weekly windows. The usual path is one query, and
    // this makes clearing a batch naturally reveal the next unresolved batch.
    for _ in 0..261 {
        let rows = sqlx::query_as::<_, ReviewItem>(&query)
            .bind(&review_window_start)
            .bind(&review_window_end)
            .bind(if ai_assistance_enabled { 1 } else { 0 })
            .bind(limit)
            .fetch_all(pool.inner())
            .await
            .map_err(|e| e.to_string())?;
        if !rows.is_empty() {
            return Ok(rows);
        }

        review_window_end = review_window_start;
        let previous_start = chrono::DateTime::parse_from_rfc3339(&review_window_end)
            .map(|time| (time - chrono::Duration::days(7)).to_rfc3339())
            .unwrap_or_else(|_| (chrono::Utc::now() - chrono::Duration::days(13)).to_rfc3339());
        review_window_start = previous_start;
    }

    Ok(Vec::new())
}

#[tauri::command]
pub async fn record_review_decision(
    pool: tauri::State<'_, SqlitePool>,
    thread_id: String,
    decision: String,
) -> Result<(), String> {
    save_review_decision(pool.inner(), &thread_id, &decision).await
}

async fn save_review_decision(pool: &SqlitePool, thread_id: &str, decision: &str) -> Result<(), String> {
    if !matches!(decision, "keep" | "follow_up" | "archived") {
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
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn schedule_follow_up(
    pool: tauri::State<'_, SqlitePool>,
    thread_id: String,
    due_at: String,
) -> Result<(), String> {
    let due_at = chrono::DateTime::parse_from_rfc3339(&due_at)
        .map_err(|_| "Follow-up time must include a valid date and time".to_string())?
        .with_timezone(&chrono::Utc)
        .to_rfc3339();
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO follow_ups (thread_id, due_at, status, created_at, updated_at)
        VALUES (?, ?, 'active', ?, ?)
        ON CONFLICT(thread_id) DO UPDATE SET
            due_at = excluded.due_at,
            status = 'active',
            updated_at = excluded.updated_at
        "#,
    )
    .bind(thread_id)
    .bind(due_at)
    .bind(&now)
    .bind(&now)
    .execute(pool.inner())
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn complete_follow_up(
    pool: tauri::State<'_, SqlitePool>,
    thread_id: String,
) -> Result<(), String> {
    complete_follow_up_for_thread(pool.inner(), &thread_id).await
}

async fn complete_follow_up_for_thread(pool: &SqlitePool, thread_id: &str) -> Result<(), String> {
    sqlx::query("UPDATE follow_ups SET status = 'completed', updated_at = ? WHERE thread_id = ?")
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(thread_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn get_follow_ups(
    pool: tauri::State<'_, SqlitePool>,
) -> Result<Vec<FollowUpItem>, String> {
    sqlx::query_as::<_, FollowUpItem>(
        r#"
        SELECT f.thread_id, f.due_at,
               t.id, t.account_id, t.subject, t.snippet, t.unread, t.starred,
               t.archived, t.last_message_at, t.label_ids, t.folder,
               COALESCE(m.from_name, '') AS from_name,
               COALESCE(m.from_email, '') AS from_email,
               COALESCE(m.to_emails, '[]') AS to_emails,
               t.category,
               a.importance AS analysis_importance
        FROM follow_ups f
        JOIN threads t ON t.id = f.thread_id
        LEFT JOIN email_analysis a ON a.thread_id = t.id
        LEFT JOIN messages m ON m.thread_id = t.id
            AND m.sent_at = (SELECT MAX(sent_at) FROM messages WHERE thread_id = t.id)
        WHERE f.status = 'active' AND t.archived = 0
          AND NOT EXISTS (
              SELECT 1 FROM mail_operations o
              WHERE o.thread_id = t.id AND o.status IN ('pending', 'in_progress')
          )
        ORDER BY f.due_at ASC
        "#,
    )
    .fetch_all(pool.inner())
    .await
    .map_err(|e| e.to_string())
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

#[cfg(test)]
mod action_contract_tests {
    use super::{
        background_analysis_candidates, background_analysis_pending_count,
        complete_follow_up_for_thread, save_review_decision, should_retry_without_calendar,
        system_prompt,
    };
    use crate::commands::test_support::TestDatabase;
    use chrono::{Duration, Utc};

    #[tokio::test]
    async fn keep_and_complete_are_local_only() {
        let db = TestDatabase::new().await;
        db.seed_thread("thread-local").await;
        db.seed_active_follow_up("thread-local").await;

        save_review_decision(&db.pool, "thread-local", "keep").await.unwrap();
        complete_follow_up_for_thread(&db.pool, "thread-local").await.unwrap();

        let decision: String = sqlx::query_scalar("SELECT decision FROM email_reviews WHERE thread_id = 'thread-local'").fetch_one(&db.pool).await.unwrap();
        let status: String = sqlx::query_scalar("SELECT status FROM follow_ups WHERE thread_id = 'thread-local'").fetch_one(&db.pool).await.unwrap();
        let remote_operations: i64 = sqlx::query_scalar("SELECT (SELECT COUNT(*) FROM mail_operations WHERE thread_id = 'thread-local') + (SELECT COUNT(*) FROM mail_flag_operations WHERE thread_id = 'thread-local')").fetch_one(&db.pool).await.unwrap();
        assert_eq!(decision, "keep");
        assert_eq!(status, "completed");
        assert_eq!(remote_operations, 0);
        db.close().await;
    }

    #[tokio::test]
    async fn background_candidates_are_recent_stale_inbox_threads_in_newest_first_order() {
        let db = TestDatabase::new().await;
        let now = Utc::now();
        let newest = (now - Duration::seconds(10)).to_rfc3339();
        let next_newest = (now - Duration::seconds(20)).to_rfc3339();
        let analyzed = (now - Duration::seconds(30)).to_rfc3339();
        let queued = (now - Duration::seconds(40)).to_rfc3339();
        let old = (now - Duration::days(1)).to_rfc3339();

        for (thread_id, message_id, timestamp) in [
            ("thread-newest", "message-newest", newest.as_str()),
            ("thread-next", "message-next", next_newest.as_str()),
            ("thread-analyzed", "message-analyzed", analyzed.as_str()),
            ("thread-queued", "message-queued", queued.as_str()),
            ("thread-old", "message-old", old.as_str()),
        ] {
            db.seed_thread(thread_id).await;
            sqlx::query("UPDATE threads SET last_message_at = ? WHERE id = ?")
                .bind(timestamp)
                .bind(thread_id)
                .execute(&db.pool)
                .await
                .unwrap();
            db.seed_message_for_thread(message_id, thread_id, timestamp).await;
        }

        // A current analysis is not eligible; a durable remote operation is
        // also excluded so the worker cannot analyze an item being removed.
        sqlx::query("INSERT INTO email_analysis (thread_id, analyzed_at) VALUES ('thread-analyzed', ?)")
            .bind(now.to_rfc3339())
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO mail_operations (id, account_id, thread_id, operation, status, next_retry_at, created_at, updated_at) VALUES ('operation-queued', 'account-1', 'thread-queued', 'archive', 'pending', ?, ?, ?)")
            .bind(now.to_rfc3339())
            .bind(now.to_rfc3339())
            .bind(now.to_rfc3339())
            .execute(&db.pool)
            .await
            .unwrap();

        let candidates = background_analysis_candidates(&db.pool, 2, now).await.unwrap();
        let pending_count = background_analysis_pending_count(&db.pool, now).await.unwrap();

        assert_eq!(
            candidates.iter().map(|candidate| candidate.thread_id.as_str()).collect::<Vec<_>>(),
            ["thread-newest", "thread-next"],
        );
        assert_eq!(pending_count, 2);
        db.close().await;
    }

    #[tokio::test]
    async fn disabled_ai_exposes_no_background_work() {
        let db = TestDatabase::new().await;
        let now = Utc::now();
        db.seed_thread("thread-disabled").await;
        let timestamp = now.to_rfc3339();
        sqlx::query("UPDATE threads SET last_message_at = ? WHERE id = 'thread-disabled'")
            .bind(&timestamp)
            .execute(&db.pool)
            .await
            .unwrap();
        db.seed_message_for_thread("message-disabled", "thread-disabled", &timestamp).await;
        sqlx::query("INSERT INTO app_settings (key, value) VALUES ('ai_assistance_settings', '{\"enabled\":false}')")
            .execute(&db.pool)
            .await
            .unwrap();

        assert!(background_analysis_candidates(&db.pool, 1, now).await.unwrap().is_empty());
        assert_eq!(background_analysis_pending_count(&db.pool, now).await.unwrap(), 0);
        db.close().await;
    }

    #[test]
    fn calendar_prompt_is_compact_and_fallback_only_handles_invalid_json() {
        let primary = system_prompt("", true);
        let fallback = system_prompt("", false);
        assert!(primary.contains("\"calendar_event\""));
        assert!(primary.contains("\"location\""));
        assert!(!primary.contains("\"description\""));
        assert!(primary.contains("any concrete event"));
        assert!(!fallback.contains("\"calendar_event\""));
        assert!(should_retry_without_calendar("model did not return valid JSON (EOF)"));
        assert!(!should_retry_without_calendar("could not reach Ollama"));
    }
}
