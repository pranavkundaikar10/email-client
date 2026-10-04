//! Shared test harness using real SQLite migrations and isolated temp files.

use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use uuid::Uuid;

pub struct TestDatabase {
    pub pool: SqlitePool,
    path: std::path::PathBuf,
}

impl TestDatabase {
    pub async fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("productive-email-test-{}.db", Uuid::new_v4()));
        let url = format!("sqlite://{}?mode=rwc", path.to_string_lossy());
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("open test database");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("apply production migrations");
        Self { pool, path }
    }

    pub async fn seed_thread(&self, thread_id: &str) {
        self.seed_account("account-1", "test@example.com").await;
        self.seed_thread_for_account(thread_id, "account-1").await;
    }

    pub async fn seed_account(&self, account_id: &str, email: &str) {
        sqlx::query("INSERT OR IGNORE INTO accounts (id, email, provider) VALUES (?, ?, 'gmail')")
            .bind(account_id)
            .bind(email)
            .execute(&self.pool)
            .await
            .expect("seed account");
    }

    pub async fn seed_thread_for_account(&self, thread_id: &str, account_id: &str) {
        sqlx::query("INSERT INTO threads (id, account_id, subject, last_message_at, folder) VALUES (?, ?, 'Test thread', '2026-01-01T00:00:00+00:00', 'inbox')")
            .bind(thread_id).bind(account_id).execute(&self.pool).await.expect("seed thread");
    }

    pub async fn seed_message_for_thread(&self, message_id: &str, thread_id: &str, sent_at: &str) {
        let account_id: String = sqlx::query_scalar("SELECT account_id FROM threads WHERE id = ?")
            .bind(thread_id)
            .fetch_one(&self.pool)
            .await
            .expect("find thread account");
        sqlx::query(
            "INSERT INTO messages (id, thread_id, account_id, from_email, subject, sent_at) VALUES (?, ?, ?, 'sender@example.com', 'Test message', ?)",
        )
        .bind(message_id)
        .bind(thread_id)
        .bind(account_id)
        .bind(sent_at)
        .execute(&self.pool)
        .await
        .expect("seed message");
    }

    pub async fn seed_active_follow_up(&self, thread_id: &str) {
        sqlx::query("INSERT INTO follow_ups (thread_id, due_at, status, created_at, updated_at) VALUES (?, '2026-01-02T00:00:00+00:00', 'active', '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00')")
            .bind(thread_id).execute(&self.pool).await.expect("seed follow-up");
    }

    pub async fn close(self) {
        self.pool.close().await;
        let _ = std::fs::remove_file(self.path);
    }
}
