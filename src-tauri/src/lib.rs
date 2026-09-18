mod commands;
mod edge_tts;
pub mod error;
pub mod export;
mod infrastructure;
pub mod proactive;
pub mod reminders;
pub mod schedule_intent;

use infrastructure::database::Database;
use tauri::{Manager, WindowEvent, menu::MenuBuilder, tray::TrayIconBuilder};

fn show_window(app: &tauri::AppHandle, label: &str) {
    if let Some(window) = app.get_webview_window(label) {
        if let Err(error) = window.show().and_then(|_| window.set_focus()) {
            eprintln!("failed to show {label} window: {error}");
        }
    }
}

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
            let settings_window = app
                .get_webview_window("settings")
                .ok_or_else(|| error::AppError::internal("failed to find the settings window"))?;
            app.manage(database);

            let menu = MenuBuilder::new(app)
                .text("open-chat", "打开聊天")
                .text("open-settings", "设置")
                .separator()
                .text("quit", "退出 Nova")
                .build()?;
            let mut tray = TrayIconBuilder::with_id("nova-tray")
                .menu(&menu)
                .tooltip("Nova · AI 陪伴")
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "open-chat" => {
                        let database = app.state::<Database>();
                        let should_open_chat =
                            commands::onboarding_complete(&database).unwrap_or(false);
                        show_window(app, if should_open_chat { "chat" } else { "settings" });
                    }
                    "open-settings" => show_window(app, "settings"),
                    "quit" => app.exit(0),
                    _ => {}
                });
            if let Some(icon) = app.default_window_icon().cloned() {
                tray = tray.icon(icon);
            }
            tray.build(app)?;

            let database = app.state::<Database>();
            if commands::onboarding_complete(&database)? {
                settings_window.hide().map_err(|window_error| {
                    error::AppError::internal(format!(
                        "failed to hide settings window: {window_error}"
                    ))
                })?;
                show_window(&app.handle(), "chat");
            } else {
                commands::restore_window_state(&settings_window, &window_state)?;
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
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
                if let Err(error) = window.hide() {
                    eprintln!("failed to hide {} window: {error}", window.label());
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
            commands::open_chat_window,
            commands::open_settings_window,
            commands::finish_onboarding,
            commands::clear_conversation_data,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Nova");
}
