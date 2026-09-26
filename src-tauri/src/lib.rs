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
            let worker_for_startup = operation_worker.clone();
            tauri::async_runtime::block_on(async move {
                let pool = db::init_db(&app_handle)
                    .await
                    .expect("failed to initialize database");
                app_handle.manage(pool.clone());
                tauri::async_runtime::spawn(async move {
                    let _ = sync::process_mail_operations(&app_handle, &pool, &worker_for_startup).await;
                });
            });
            app.manage(operation_worker);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            auth::add_account,
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
            sync::fetch_message_body,
            sync::prefetch_thread_bodies,
            sync::archive_thread,
            sync::delete_thread,
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
            agent::get_auto_analysis_candidates,
            agent::get_review_queue,
            agent::record_review_decision,
            agent::get_digest,
            agent::get_thread_analysis,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
