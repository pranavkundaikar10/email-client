mod commands;

use commands::{agent, auth, compose, db, splits, sync};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_handle = app.handle().clone();
            let operation_worker = sync::MailOperationWorker::default();
            let inbox_idle_worker = sync::InboxIdleWorker::default();
            let worker_for_startup = operation_worker.clone();
            tauri::async_runtime::block_on(async move {
                if let Err(error) = db::migrate_legacy_app_data(&app_handle) {
                    // A migration problem should never prevent someone from
                    // opening the app or setting it up normally.
                    eprintln!("Could not migrate legacy application data: {error}");
                }
                let pool = db::init_db(&app_handle)
                    .await
                    .expect("failed to initialize database");
                if let Err(error) = db::merge_legacy_app_data(&app_handle, &pool).await {
                    eprintln!("Could not merge legacy application data: {error}");
                }
                app_handle.manage(pool.clone());
                tauri::async_runtime::spawn(async move {
                    let _ = sync::process_mail_operations(&app_handle, &pool, &worker_for_startup).await;
                });
            });
            app.manage(operation_worker);
            app.manage(inbox_idle_worker);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            auth::add_account,
            auth::connect_google_account,
            auth::get_accounts,
            auth::get_account_profile,
            auth::remove_account,
            db::get_threads,
            db::get_messages,
            db::get_unread_counts,
            db::search_threads,
            sync::mark_thread_read,
            sync::mark_thread_unread,
            sync::sync_inbox,
            sync::sync_older,
            sync::sync_sent,
            sync::sync_drafts,
            sync::start_inbox_idle,
            sync::stop_inbox_idle,
            sync::fetch_message_body,
            sync::prefetch_thread_bodies,
            sync::archive_thread,
            sync::delete_thread,
            sync::cancel_mail_operations,
            sync::process_pending_mail_operations,
            sync::star_thread,
            splits::get_splits,
            splits::create_split,
            splits::update_split,
            splits::delete_split,
            splits::reorder_splits,
            splits::recategorize_threads,
            compose::send_email,
            agent::analyze_thread,
            agent::analyze_inbox,
            agent::get_ollama_models,
            agent::get_ai_model,
            agent::set_ai_model,
            agent::get_triage_preferences,
            agent::set_triage_preferences,
            agent::get_thinking_settings,
            agent::set_thinking_settings,
            agent::get_auto_analysis_candidates,
            agent::get_auto_analysis_pending_count,
            agent::get_review_queue,
            agent::record_review_decision,
            agent::schedule_follow_up,
            agent::complete_follow_up,
            agent::get_follow_ups,
            agent::get_digest,
            agent::get_thread_analysis,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
