mod commands;
pub mod error;
mod infrastructure;

use infrastructure::database::Database;
use tauri::{Manager, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
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
            commands::set_window_mode,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Nova");
}
