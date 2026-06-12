use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone, FromRow)]
pub struct SplitRow {
    pub id: String,
    pub name: String,
    pub position: i64,
    pub rules: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SplitRule {
    #[serde(rename = "type")]
    pub rule_type: String,
    pub value: Option<String>,
}

/// Evaluate splits against a message and return the matching split id.
/// Splits with rules are tried first (in position order); empty-rules splits
/// are catch-alls tried last.
pub fn evaluate_splits(
    splits: &[SplitRow],
    from_email: &str,
    subject: &str,
    is_newsletter: bool,
) -> String {
    let mut with_rules: Vec<&SplitRow> = splits
        .iter()
        .filter(|s| s.rules != "[]" && !s.rules.trim().is_empty())
        .collect();
    with_rules.sort_by_key(|s| s.position);

    let mut catch_alls: Vec<&SplitRow> = splits
        .iter()
        .filter(|s| s.rules == "[]" || s.rules.trim().is_empty())
        .collect();
    catch_alls.sort_by_key(|s| s.position);

    for split in &with_rules {
        let rules: Vec<SplitRule> = serde_json::from_str(&split.rules).unwrap_or_default();
        if rules
            .iter()
            .any(|r| matches_rule(r, from_email, subject, is_newsletter))
        {
            return split.id.clone();
        }
    }

    catch_alls
        .first()
        .map(|s| s.id.clone())
        .unwrap_or_else(|| "important".to_string())
}

fn matches_rule(rule: &SplitRule, from_email: &str, subject: &str, is_newsletter: bool) -> bool {
    match rule.rule_type.as_str() {
        "is_newsletter" => is_newsletter,
        "from_pattern" => rule.value.as_deref().map_or(false, |pattern| {
            let from_lc = from_email.to_lowercase();
            pattern.split('|').any(|p| from_lc.contains(p.trim()))
        }),
        "from_email" => rule
            .value
            .as_deref()
            .map_or(false, |v| v.eq_ignore_ascii_case(from_email)),
        "from_domain" => rule.value.as_deref().map_or(false, |domain| {
            from_email
                .split('@')
                .nth(1)
                .map_or(false, |d| d.eq_ignore_ascii_case(domain))
        }),
        "subject_contains" => rule.value.as_deref().map_or(false, |kw| {
            let sub_lc = subject.to_lowercase();
            kw.split('|').any(|p| sub_lc.contains(p.trim().to_lowercase().as_str()))
        }),
        _ => false,
    }
}

pub async fn load_splits(pool: &SqlitePool) -> Result<Vec<SplitRow>, String> {
    sqlx::query_as::<_, SplitRow>(
        "SELECT id, name, position, rules FROM splits ORDER BY position ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_splits(pool: tauri::State<'_, SqlitePool>) -> Result<Vec<SplitRow>, String> {
    load_splits(pool.inner()).await
}

#[tauri::command]
pub async fn create_split(
    pool: tauri::State<'_, SqlitePool>,
    name: String,
) -> Result<SplitRow, String> {
    let id = Uuid::new_v4().to_string();
    let position: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(position) + 1, 0) FROM splits")
            .fetch_one(pool.inner())
            .await
            .unwrap_or(0);

    sqlx::query("INSERT INTO splits (id, name, position, rules) VALUES (?, ?, ?, '[]')")
        .bind(&id)
        .bind(&name)
        .bind(position)
        .execute(pool.inner())
        .await
        .map_err(|e| e.to_string())?;

    Ok(SplitRow { id, name, position, rules: "[]".to_string() })
}

#[tauri::command]
pub async fn update_split(
    pool: tauri::State<'_, SqlitePool>,
    id: String,
    name: String,
    rules: String,
) -> Result<(), String> {
    sqlx::query("UPDATE splits SET name = ?, rules = ? WHERE id = ?")
        .bind(&name)
        .bind(&rules)
        .bind(&id)
        .execute(pool.inner())
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn delete_split(
    pool: tauri::State<'_, SqlitePool>,
    id: String,
) -> Result<(), String> {
    sqlx::query("DELETE FROM splits WHERE id = ?")
        .bind(&id)
        .execute(pool.inner())
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn reorder_splits(
    pool: tauri::State<'_, SqlitePool>,
    ids: Vec<String>,
) -> Result<(), String> {
    let mut tx = pool.inner().begin().await.map_err(|e| e.to_string())?;
    for (i, id) in ids.iter().enumerate() {
        sqlx::query("UPDATE splits SET position = ? WHERE id = ?")
            .bind(i as i64)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

/// Re-evaluate every thread against the current splits and update categories.
/// Called after the user saves split changes.
#[tauri::command]
pub async fn recategorize_threads(
    pool: tauri::State<'_, SqlitePool>,
) -> Result<(), String> {
    let splits = load_splits(pool.inner()).await?;

    // (thread_id, from_email, subject, is_newsletter)
    let rows: Vec<(String, String, String, i64)> = sqlx::query_as(
        r#"
        SELECT t.id,
               COALESCE(m.from_email, ''),
               COALESCE(t.subject, ''),
               COALESCE(m.is_newsletter, 0)
        FROM threads t
        LEFT JOIN messages m ON m.thread_id = t.id
            AND m.sent_at = (SELECT MAX(sent_at) FROM messages WHERE thread_id = t.id)
        "#,
    )
    .fetch_all(pool.inner())
    .await
    .map_err(|e| e.to_string())?;

    let mut tx = pool.inner().begin().await.map_err(|e| e.to_string())?;
    for (thread_id, from_email, subject, is_newsletter_int) in &rows {
        let category =
            evaluate_splits(&splits, from_email, subject, *is_newsletter_int != 0);
        sqlx::query("UPDATE threads SET category = ? WHERE id = ?")
            .bind(&category)
            .bind(thread_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}
