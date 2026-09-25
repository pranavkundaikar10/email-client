mod commands;

use commands::{agent, auth, compose, db, splits, sync};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_handle = app.handle().clone();
            tauri::async_runtime::block_on(async move {
                let pool = db::init_db(&app_handle)
                    .await
                    .expect("failed to initialize database");
                app_handle.manage(pool);
            });
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
            sync::archive_thread,
            sync::delete_thread,
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
