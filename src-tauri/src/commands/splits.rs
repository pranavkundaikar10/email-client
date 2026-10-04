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
    // An explicit sender choice is more specific than a broad pattern such as
    // "newsletter" or a subject keyword. This makes the Cmd+K "route future
    // mail" action reliable even when an older, general split also matches.
    let mut ordered: Vec<&SplitRow> = splits.iter().collect();
    ordered.sort_by_key(|split| split.position);
    for split in &ordered {
        let rules: Vec<SplitRule> = serde_json::from_str(&split.rules).unwrap_or_default();
        if rules.iter().any(|rule| {
            rule.rule_type == "from_email" && matches_rule(rule, from_email, subject, is_newsletter)
        }) {
            return split.id.clone();
        }
    }

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
        "from_email" => rule.value.as_deref().map_or(false, |values| {
            values
                .split('|')
                .any(|value| value.trim().eq_ignore_ascii_case(from_email))
        }),
        "from_domain" => rule.value.as_deref().map_or(false, |domain| {
            from_email
                .split('@')
                .nth(1)
                .map_or(false, |d| d.eq_ignore_ascii_case(domain))
        }),
        "subject_contains" => rule.value.as_deref().map_or(false, |kw| {
            let sub_lc = subject.to_lowercase();
            kw.split('|')
                .any(|p| sub_lc.contains(p.trim().to_lowercase().as_str()))
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
    let position: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(position) + 1, 0) FROM splits")
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

    Ok(SplitRow {
        id,
        name,
        position,
        rules: "[]".to_string(),
    })
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
pub async fn delete_split(pool: tauri::State<'_, SqlitePool>, id: String) -> Result<(), String> {
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
pub async fn recategorize_threads(pool: tauri::State<'_, SqlitePool>) -> Result<(), String> {
    recategorize_threads_in_pool(pool.inner()).await
}

async fn recategorize_threads_in_pool(pool: &SqlitePool) -> Result<(), String> {
    let splits = load_splits(pool).await?;

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
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    for (thread_id, from_email, subject, is_newsletter_int) in &rows {
        let category = evaluate_splits(&splits, from_email, subject, *is_newsletter_int != 0);
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

/// Add the current thread's sender to a split's exact-sender rule. The rule is
/// local-only: it categorizes future synced messages but never changes Gmail.
#[tauri::command]
pub async fn add_thread_sender_to_split_rule(
    pool: tauri::State<'_, SqlitePool>,
    account_id: String,
    thread_id: String,
    split_id: String,
) -> Result<String, String> {
    add_thread_sender_to_split_rule_in_pool(pool.inner(), &account_id, &thread_id, &split_id).await
}

async fn add_thread_sender_to_split_rule_in_pool(
    pool: &SqlitePool,
    account_id: &str,
    thread_id: &str,
    split_id: &str,
) -> Result<String, String> {
    let mut split: SplitRow =
        sqlx::query_as("SELECT id, name, position, rules FROM splits WHERE id = ?")
            .bind(&split_id)
            .fetch_one(pool)
            .await
            .map_err(|_| "That split no longer exists.".to_string())?;

    // Prefer the first inbound message so a thread whose latest message is the
    // user's own reply still teaches the original sender, not the user.
    let sender: Option<String> = sqlx::query_scalar(
        r#"
        SELECT m.from_email
        FROM messages m
        WHERE m.thread_id = ? AND m.account_id = ?
          AND LOWER(m.from_email) != LOWER((SELECT email FROM accounts WHERE id = ?))
        ORDER BY m.sent_at ASC
        LIMIT 1
        "#,
    )
    .bind(thread_id)
    .bind(account_id)
    .bind(account_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    let sender = sender
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "This thread has no sender email to add as a rule.".to_string())?;

    let mut rules: Vec<SplitRule> = serde_json::from_str(&split.rules).unwrap_or_default();
    if let Some(rule) = rules.iter_mut().find(|rule| rule.rule_type == "from_email") {
        let mut values: Vec<String> = rule
            .value
            .as_deref()
            .unwrap_or_default()
            .split('|')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        if !values
            .iter()
            .any(|value| value.eq_ignore_ascii_case(&sender))
        {
            values.push(sender);
        }
        rule.value = Some(values.join("|"));
    } else {
        rules.push(SplitRule {
            rule_type: "from_email".to_string(),
            value: Some(sender),
        });
    }
    split.rules = serde_json::to_string(&rules).map_err(|e| e.to_string())?;

    sqlx::query("UPDATE splits SET rules = ? WHERE id = ?")
        .bind(&split.rules)
        .bind(split_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    recategorize_threads_in_pool(pool).await?;
    sqlx::query_scalar("SELECT category FROM threads WHERE id = ? AND account_id = ?")
        .bind(thread_id)
        .bind(account_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Email not found for this account.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::TestDatabase;

    #[tokio::test]
    async fn sender_rule_is_appended_and_categorizes_the_current_thread() {
        let database = TestDatabase::new().await;
        database.seed_thread("thread-1").await;
        database
            .seed_message_for_thread("message-1", "thread-1", "2026-01-01T00:00:00+00:00")
            .await;
        sqlx::query(
            "INSERT INTO splits (id, name, position, rules) VALUES ('updates', 'Updates', 0, '[{\"type\":\"from_email\",\"value\":\"first@example.com\"}]')",
        )
        .execute(&database.pool)
        .await
        .expect("seed split");

        add_thread_sender_to_split_rule_in_pool(&database.pool, "account-1", "thread-1", "updates")
            .await
            .expect("add sender rule");

        let rules: String = sqlx::query_scalar("SELECT rules FROM splits WHERE id = 'updates'")
            .fetch_one(&database.pool)
            .await
            .expect("read rules");
        assert!(rules.contains("first@example.com|sender@example.com"));
        let category: String =
            sqlx::query_scalar("SELECT category FROM threads WHERE id = 'thread-1'")
                .fetch_one(&database.pool)
                .await
                .expect("read thread category");
        assert_eq!(category, "updates");
        database.close().await;
    }

    #[tokio::test]
    async fn sender_rule_cannot_be_added_for_another_accounts_thread() {
        let database = TestDatabase::new().await;
        database.seed_thread("thread-1").await;
        sqlx::query(
            "INSERT INTO splits (id, name, position, rules) VALUES ('updates', 'Updates', 0, '[]')",
        )
        .execute(&database.pool)
        .await
        .expect("seed split");

        let error = add_thread_sender_to_split_rule_in_pool(
            &database.pool,
            "other-account",
            "thread-1",
            "updates",
        )
        .await
        .expect_err("cross-account rule creation must fail");
        assert_eq!(error, "This thread has no sender email to add as a rule.");
        database.close().await;
    }

    #[test]
    fn exact_sender_rule_beats_a_broader_earlier_split_rule() {
        let newsletter = SplitRow {
            id: "newsletter".to_string(),
            name: "Newsletter".to_string(),
            position: 0,
            rules: r#"[{"type":"from_pattern","value":"newsletter"}]"#.to_string(),
        };
        let promotions = SplitRow {
            id: "promotions".to_string(),
            name: "Promotions".to_string(),
            position: 1,
            rules: r#"[{"type":"from_email","value":"newsletters@em.walmart.com"}]"#.to_string(),
        };

        assert_eq!(
            evaluate_splits(
                &[newsletter, promotions],
                "newsletters@em.walmart.com",
                "New arrivals",
                true,
            ),
            "promotions",
        );
    }
}
