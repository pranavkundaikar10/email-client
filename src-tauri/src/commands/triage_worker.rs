//! Durable scheduling primitives for the low-impact background triage worker.
//!
//! These functions deliberately do not know about IMAP, Ollama, or Tauri. The
//! worker orchestration layer will supply those adapters later; keeping local
//! job ownership here makes it safe to test without network access.

use chrono::{DateTime, Duration, Utc};
use sqlx::{FromRow, SqlitePool};
use std::{future::Future, pin::Pin, sync::Arc};
use tauri::Emitter;
use tokio::sync::Mutex;

const LEASE_DURATION: Duration = Duration::minutes(10);

/// Serializes the local model workload across startup, sync, and IMAP IDLE
/// wake-ups. Durable database leases still protect recovery after interruption.
#[derive(Clone, Default)]
pub struct BackgroundTriageWorker {
    lock: Arc<Mutex<()>>,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct TriageJob {
    pub thread_id: String,
    pub message_id: String,
}

/// Adapter boundary for the actual read-only IMAP fetch plus local-model
/// analysis. Tests provide a fake implementation; the live adapter is added
/// only when this worker is connected to the sync lifecycle.
pub trait TriageExecutor {
    fn process<'a>(
        &'a self,
        job: &'a TriageJob,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessOneResult {
    Idle,
    Completed,
    Retrying,
}

pub async fn enqueue_triage_job(
    pool: &SqlitePool,
    thread_id: &str,
    message_id: &str,
    message_received_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<(), String> {
    let now = now.to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO background_triage_jobs
            (thread_id, message_id, message_received_at, status, attempt_count,
             next_attempt_at, created_at, updated_at)
        VALUES (?, ?, ?, 'pending', 0, ?, ?, ?)
        ON CONFLICT(thread_id) DO UPDATE SET
            message_id = excluded.message_id,
            message_received_at = excluded.message_received_at,
            status = 'pending',
            attempt_count = 0,
            next_attempt_at = excluded.next_attempt_at,
            lease_expires_at = NULL,
            last_error = NULL,
            updated_at = excluded.updated_at
        WHERE background_triage_jobs.status = 'pending'
          AND background_triage_jobs.message_id != excluded.message_id
        "#,
    )
    .bind(thread_id)
    .bind(message_id)
    .bind(message_received_at.to_rfc3339())
    .bind(&now)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// Atomically takes one ready job. SQLite serializes this short transaction,
/// so simultaneous wake-ups cannot both receive the same email.
pub async fn claim_next_triage_job(
    pool: &SqlitePool,
    now: DateTime<Utc>,
) -> Result<Option<TriageJob>, String> {
    let now_text = now.to_rfc3339();
    let lease_expires_at = (now + LEASE_DURATION).to_rfc3339();
    let mut transaction = pool.begin().await.map_err(|error| error.to_string())?;

    sqlx::query(
        "UPDATE background_triage_jobs \
         SET status = 'pending', lease_expires_at = NULL, updated_at = ? \
         WHERE status = 'leased' AND lease_expires_at <= ?",
    )
    .bind(&now_text)
    .bind(&now_text)
    .execute(&mut *transaction)
    .await
    .map_err(|error| error.to_string())?;

    let candidate: Option<TriageJob> = sqlx::query_as(
        "SELECT thread_id, message_id FROM background_triage_jobs \
         WHERE status = 'pending' AND next_attempt_at <= ? \
         ORDER BY message_received_at DESC, created_at ASC LIMIT 1",
    )
    .bind(&now_text)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| error.to_string())?;

    let Some(job) = candidate else {
        transaction.commit().await.map_err(|error| error.to_string())?;
        return Ok(None);
    };

    let claimed = sqlx::query(
        "UPDATE background_triage_jobs \
         SET status = 'leased', lease_expires_at = ?, updated_at = ? \
         WHERE thread_id = ? AND message_id = ? AND status = 'pending'",
    )
    .bind(&lease_expires_at)
    .bind(&now_text)
    .bind(&job.thread_id)
    .bind(&job.message_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| error.to_string())?;
    transaction.commit().await.map_err(|error| error.to_string())?;

    if claimed.rows_affected() == 1 { Ok(Some(job)) } else { Ok(None) }
}

pub async fn complete_triage_job(pool: &SqlitePool, job: &TriageJob) -> Result<(), String> {
    sqlx::query("DELETE FROM background_triage_jobs WHERE thread_id = ? AND message_id = ? AND status = 'leased'")
        .bind(&job.thread_id)
        .bind(&job.message_id)
        .execute(pool)
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Returns a failed job to the queue after a short delay. Other ready jobs can
/// continue immediately, so a temporary Ollama failure never stalls triage.
pub async fn retry_triage_job(
    pool: &SqlitePool,
    job: &TriageJob,
    error: &str,
    now: DateTime<Utc>,
) -> Result<(), String> {
    let attempt_count: i64 = sqlx::query_scalar(
        "SELECT attempt_count FROM background_triage_jobs WHERE thread_id = ? AND message_id = ? AND status = 'leased'",
    )
    .bind(&job.thread_id)
    .bind(&job.message_id)
    .fetch_optional(pool)
    .await
    .map_err(|database_error| database_error.to_string())?
    .unwrap_or(0);
    let delay_minutes = 1_i64 << attempt_count.min(4);
    let now_text = now.to_rfc3339();
    sqlx::query(
        "UPDATE background_triage_jobs \
         SET status = CASE WHEN attempt_count + 1 >= 3 THEN 'failed' ELSE 'pending' END, \
             attempt_count = attempt_count + 1, \
             next_attempt_at = ?, lease_expires_at = NULL, last_error = ?, updated_at = ? \
         WHERE thread_id = ? AND message_id = ? AND status = 'leased'",
    )
    .bind((now + Duration::minutes(delay_minutes)).to_rfc3339())
    .bind(error)
    .bind(&now_text)
    .bind(&job.thread_id)
    .bind(&job.message_id)
    .execute(pool)
    .await
    .map_err(|database_error| database_error.to_string())?;
    Ok(())
}

/// Runs at most one leased job. It does not discover new mail itself, so a
/// future wake-up coordinator can decide when candidate discovery is safe.
/// A failed executor result is persisted as a delayed retry rather than
/// escaping and preventing later jobs from being processed.
pub async fn process_one_triage_job<E: TriageExecutor>(
    pool: &SqlitePool,
    executor: &E,
    now: DateTime<Utc>,
) -> Result<ProcessOneResult, String> {
    let Some(job) = claim_next_triage_job(pool, now).await? else {
        return Ok(ProcessOneResult::Idle);
    };
    match executor.process(&job).await {
        Ok(()) => {
            complete_triage_job(pool, &job).await?;
            Ok(ProcessOneResult::Completed)
        }
        Err(error) => {
            retry_triage_job(pool, &job, &error, now).await?;
            Ok(ProcessOneResult::Retrying)
        }
    }
}

struct LiveTriageExecutor {
    app: tauri::AppHandle,
    pool: SqlitePool,
}

impl TriageExecutor for LiveTriageExecutor {
    fn process<'a>(
        &'a self,
        job: &'a TriageJob,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            let email: String = sqlx::query_scalar(
                "SELECT a.email FROM threads t JOIN accounts a ON a.id = t.account_id WHERE t.id = ?",
            )
            .bind(&job.thread_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|error| error.to_string())?;
            crate::commands::sync::fetch_message_body_for_account(
                &self.app,
                &self.pool,
                &email,
                &job.message_id,
                false,
            )
            .await?;
            crate::commands::agent::analyze_thread_with_pool(
                &self.pool,
                &job.thread_id,
                None,
                None,
                Some("background"),
            )
            .await?;
            Ok(())
        })
    }
}

/// Discovers a small bounded set of today’s eligible mail, persists it, then
/// processes at most one item. Calling it repeatedly is safe: the lease and
/// upsert rules preserve retries and prevent duplicate local-model calls.
pub async fn run_one_background_triage(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
) -> Result<ProcessOneResult, String> {
    if !crate::commands::agent::background_triage_enabled(pool).await? {
        return Ok(ProcessOneResult::Idle);
    }
    let now = Utc::now();
    for candidate in crate::commands::agent::background_analysis_candidates(pool, 5, now).await? {
        let received_at = DateTime::parse_from_rfc3339(&candidate.message_received_at)
            .map_err(|_| "background candidate has an invalid received timestamp".to_string())?
            .with_timezone(&Utc);
        enqueue_triage_job(
            pool,
            &candidate.thread_id,
            &candidate.message_id,
            received_at,
            now,
        )
        .await?;
    }
    let executor = LiveTriageExecutor { app: app.clone(), pool: pool.clone() };
    process_one_triage_job(pool, &executor, now).await
}

async fn process_background_triage_loop(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    worker: &BackgroundTriageWorker,
) -> Result<(), String> {
    let _guard = worker.lock.lock().await;
    loop {
        match run_one_background_triage(app, pool).await? {
            ProcessOneResult::Idle => return Ok(()),
            // A short yield keeps the UI and IMAP reconnect work responsive
            // while preserving strictly one local-model call at a time.
            ProcessOneResult::Completed | ProcessOneResult::Retrying => {
                let _ = app.emit("background-triage-updated", ());
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        }
    }
}

pub fn start_background_triage(
    app: tauri::AppHandle,
    pool: SqlitePool,
    worker: BackgroundTriageWorker,
) {
    tauri::async_runtime::spawn(async move {
        if let Err(error) = process_background_triage_loop(&app, &pool, &worker).await {
            eprintln!("Background email triage paused: {error}");
        }
    });
}

#[tauri::command]
pub async fn process_background_triage(
    app: tauri::AppHandle,
    pool: tauri::State<'_, SqlitePool>,
    worker: tauri::State<'_, BackgroundTriageWorker>,
) -> Result<(), String> {
    start_background_triage(app, pool.inner().clone(), worker.inner().clone());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        claim_next_triage_job, complete_triage_job, enqueue_triage_job,
        process_one_triage_job, retry_triage_job, ProcessOneResult, TriageExecutor, TriageJob,
    };
    use crate::commands::test_support::TestDatabase;
    use chrono::{Duration, Utc};
    use std::{future::Future, pin::Pin, sync::Mutex};

    struct FakeExecutor {
        result: Result<(), String>,
        processed: Mutex<Vec<String>>,
    }

    impl TriageExecutor for FakeExecutor {
        fn process<'a>(
            &'a self,
            job: &'a TriageJob,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
            Box::pin(async move {
                self.processed.lock().unwrap().push(job.thread_id.clone());
                self.result.clone()
            })
        }
    }

    async fn seed_job(database: &TestDatabase, thread_id: &str, message_id: &str, received_at: chrono::DateTime<Utc>) {
        database.seed_thread(thread_id).await;
        database.seed_message_for_thread(message_id, thread_id, &received_at.to_rfc3339()).await;
        enqueue_triage_job(&database.pool, thread_id, message_id, received_at, received_at).await.unwrap();
    }

    #[tokio::test]
    async fn claims_one_newest_job_and_never_claims_it_twice() {
        let database = TestDatabase::new().await;
        let now = Utc::now();
        seed_job(&database, "thread-older", "message-older", now - Duration::minutes(2)).await;
        seed_job(&database, "thread-newer", "message-newer", now - Duration::minutes(1)).await;

        let first = claim_next_triage_job(&database.pool, now).await.unwrap().unwrap();
        let second = claim_next_triage_job(&database.pool, now).await.unwrap().unwrap();
        let none_left = claim_next_triage_job(&database.pool, now).await.unwrap();

        assert_eq!(first.thread_id, "thread-newer");
        assert_eq!(second.thread_id, "thread-older");
        assert_eq!(none_left, None);
        database.close().await;
    }

    #[tokio::test]
    async fn completion_removes_only_the_claimed_job() {
        let database = TestDatabase::new().await;
        let now = Utc::now();
        seed_job(&database, "thread-complete", "message-complete", now).await;
        let job = claim_next_triage_job(&database.pool, now).await.unwrap().unwrap();

        complete_triage_job(&database.pool, &job).await.unwrap();
        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM background_triage_jobs")
            .fetch_one(&database.pool).await.unwrap();
        assert_eq!(remaining, 0);
        database.close().await;
    }

    #[tokio::test]
    async fn failure_delays_one_job_without_blocking_another_ready_job() {
        let database = TestDatabase::new().await;
        let now = Utc::now();
        seed_job(&database, "thread-first", "message-first", now).await;
        seed_job(&database, "thread-second", "message-second", now - Duration::seconds(1)).await;
        let first = claim_next_triage_job(&database.pool, now).await.unwrap().unwrap();

        retry_triage_job(&database.pool, &first, "Ollama unavailable", now).await.unwrap();
        let second = claim_next_triage_job(&database.pool, now).await.unwrap().unwrap();

        assert_eq!(second.thread_id, "thread-second");
        let retry: (String, i64, String) = sqlx::query_as(
            "SELECT status, attempt_count, last_error FROM background_triage_jobs WHERE thread_id = 'thread-first'",
        ).fetch_one(&database.pool).await.unwrap();
        assert_eq!(retry, ("pending".to_string(), 1, "Ollama unavailable".to_string()));
        database.close().await;
    }

    #[tokio::test]
    async fn expired_lease_is_recovered_after_interruption() {
        let database = TestDatabase::new().await;
        let now = Utc::now();
        seed_job(&database, "thread-recover", "message-recover", now).await;
        let first = claim_next_triage_job(&database.pool, now).await.unwrap().unwrap();

        let recovered = claim_next_triage_job(&database.pool, now + Duration::minutes(11)).await.unwrap().unwrap();
        assert_eq!(recovered, first);
        database.close().await;
    }

    #[tokio::test]
    async fn runner_completes_one_job_through_the_executor() {
        let database = TestDatabase::new().await;
        let now = Utc::now();
        seed_job(&database, "thread-run", "message-run", now).await;
        let executor = FakeExecutor { result: Ok(()), processed: Mutex::new(Vec::new()) };

        let result = process_one_triage_job(&database.pool, &executor, now).await.unwrap();

        assert_eq!(result, ProcessOneResult::Completed);
        assert_eq!(*executor.processed.lock().unwrap(), ["thread-run"]);
        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM background_triage_jobs")
            .fetch_one(&database.pool).await.unwrap();
        assert_eq!(remaining, 0);
        database.close().await;
    }

    #[tokio::test]
    async fn runner_records_failure_and_allows_a_later_job_to_run() {
        let database = TestDatabase::new().await;
        let now = Utc::now();
        seed_job(&database, "thread-fails", "message-fails", now).await;
        seed_job(&database, "thread-later", "message-later", now - Duration::seconds(1)).await;
        let failing = FakeExecutor { result: Err("model offline".to_string()), processed: Mutex::new(Vec::new()) };
        let succeeding = FakeExecutor { result: Ok(()), processed: Mutex::new(Vec::new()) };

        assert_eq!(process_one_triage_job(&database.pool, &failing, now).await.unwrap(), ProcessOneResult::Retrying);
        assert_eq!(process_one_triage_job(&database.pool, &succeeding, now).await.unwrap(), ProcessOneResult::Completed);
        assert_eq!(*failing.processed.lock().unwrap(), ["thread-fails"]);
        assert_eq!(*succeeding.processed.lock().unwrap(), ["thread-later"]);
        database.close().await;
    }

    #[tokio::test]
    async fn third_failure_is_terminal_and_cannot_be_claimed_again() {
        let database = TestDatabase::new().await;
        let now = Utc::now();
        seed_job(&database, "thread-terminal", "message-terminal", now).await;
        let executor = FakeExecutor { result: Err("invalid response".to_string()), processed: Mutex::new(Vec::new()) };

        for attempt in 0..3 {
            let result = process_one_triage_job(&database.pool, &executor, now + Duration::minutes(20 * attempt)).await.unwrap();
            assert_eq!(result, ProcessOneResult::Retrying);
        }

        let state: (String, i64) = sqlx::query_as(
            "SELECT status, attempt_count FROM background_triage_jobs WHERE thread_id = 'thread-terminal'",
        ).fetch_one(&database.pool).await.unwrap();
        assert_eq!(state, ("failed".to_string(), 3));
        assert_eq!(claim_next_triage_job(&database.pool, now + Duration::hours(2)).await.unwrap(), None);
        database.close().await;
    }
}
