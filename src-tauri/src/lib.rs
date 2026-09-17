mod commands;
pub mod error;
pub mod export;
mod infrastructure;
pub mod proactive;
pub mod reminders;
pub mod schedule_intent;

use infrastructure::database::Database;
use tauri::{Manager, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir().map_err(|error| {
                error::AppError::internal(format!(
                    "failed to resolve the application data directory: {error}"
                ))
            })?;
            let database = Database::initialize(&app_data_dir)?;
            let window_state = database.get_window_state()?;
            let window = app.get_webview_window("main").ok_or_else(|| {
                error::AppError::internal("failed to find the main application window")
            })?;
            commands::restore_window_state(&window, &window_state)?;
            app.manage(database);
            Ok(())
        })
        .on_window_event(|window, event| {
            if matches!(event, WindowEvent::CloseRequested { .. }) {
                let size = window.inner_size();
                let position = window.outer_position();
                if let (Ok(size), Ok(position)) = (size, position) {
                    let database = window.state::<Database>();
                    if let Err(error) = database.save_window_geometry(
                        size.width,
                        size.height,
                        position.x,
                        position.y,
                    ) {
                        eprintln!("failed to persist window state: {error}");
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::get_persona,
            commands::save_persona,
            commands::get_settings,
            commands::save_settings,
            commands::get_api_profile_status,
            commands::save_api_profile,
            commands::test_api_profile,
            commands::list_messages,
            commands::send_message,
            commands::retry_message,
            commands::list_memories,
            commands::update_memory,
            commands::delete_memory,
            commands::list_schedules,
            commands::confirm_schedule,
            commands::update_schedule,
            commands::delete_schedule,
            commands::get_schedule_candidate,
            commands::export_local_data,
            commands::show_notification,
            commands::transcribe_audio,
            commands::synthesize_speech,
            commands::set_window_mode,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Nova");
}
