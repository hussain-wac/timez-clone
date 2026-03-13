mod api;
mod commands;
mod idle;
mod local_store;
mod models;
mod services;
mod timer_state;

use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    std::panic::set_hook(Box::new(|panic_info| {
        eprintln!("[PANIC] Application panic: {:?}", panic_info);
    }));

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Auth token shared state
            app.manage(api::AuthToken::new(api::AuthTokenState::new()));

            // Activity tracker shared state
            app.manage(idle::ActivityState::new(idle::ActivityTracker::new()));

            // Local timer state (caches tasks, tracks running timer locally)
            app.manage(timer_state::TimerState::new(
                timer_state::TimerStateInner::new(),
            ));

            // Local storage for timestamps (crash recovery)
            let app_data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."));
            app.manage(local_store::LocalTimeStorage::new(app_data_dir));

            // Build system tray menu
            let show_item = MenuItemBuilder::with_id("show", "Show Window").build(app)?;
            let toggle_item =
                MenuItemBuilder::with_id("toggle_timer", "Pause/Resume Task").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let menu = MenuBuilder::new(app)
                .item(&show_item)
                .item(&toggle_item)
                .separator()
                .item(&quit_item)
                .build()?;

            // Create system tray icon
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Timez Pro")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                        let running = {
                            let timer_state = app.state::<timer_state::TimerState>();
                            timer_state
                                .inner()
                                .lock()
                                .ok()
                                .and_then(|s| s.running_task_id)
                                .is_some()
                        };
                        app.emit("request-quit-confirm", running).ok();
                    }
                    "toggle_timer" => {
                        let token = {
                            let auth = app.state::<api::AuthToken>();
                            auth.inner()
                                .lock()
                                .ok()
                                .and_then(|s| s.access_token.clone())
                        };

                        let running_task_id = {
                            let timer_state = app.state::<timer_state::TimerState>();
                            timer_state
                                .inner()
                                .lock()
                                .ok()
                                .and_then(|s| s.running_task_id)
                        };

                        if running_task_id.is_some() {
                            let timer_state = app.state::<timer_state::TimerState>();
                            let local_store = app.state::<local_store::LocalTimeStorage>();
                            if let Ok(mut s) = timer_state.inner().lock() {
                                if s.stop_current(&token, &local_store).is_ok() {
                                    app.emit("timer-stopped", ()).ok();
                                }
                            }
                        } else {
                            let last_task_id = {
                                let timer_state = app.state::<timer_state::TimerState>();
                                timer_state.inner().lock().ok().and_then(|s| s.last_task_id)
                            };

                            if let Some(task_id) = last_task_id {
                                let timer_state = app.state::<timer_state::TimerState>();
                                let local_store = app.state::<local_store::LocalTimeStorage>();
                                if let Ok(mut s) = timer_state.inner().lock() {
                                    if s.start_task(task_id, &token, &local_store).is_ok() {
                                        app.emit("timer-started", ()).ok();
                                    }
                                }
                            } else if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click { .. } = event {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            // Spawn background sync thread (syncs with API every 10 minutes)
            timer_state::spawn_sync_thread(app.handle().clone());

            // Spawn timestamp recording thread (records every 5 seconds)
            timer_state::spawn_timestamp_thread(app.handle().clone());

            // Run crash recovery on startup
            timer_state::crash_recovery_on_startup(app.handle());

            // Spawn idle monitor (10 second threshold)
            idle::spawn_idle_monitor(app.handle().clone(), 10);

            Ok(())
        })
        .on_window_event(|window, event| {
            // Hide window instead of closing — app stays in system tray
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_tasks,
            commands::start_timer,
            commands::stop_timer,
            commands::get_status,
            commands::add_idle_time,
            commands::discard_idle_time,
            commands::refresh_tasks,
            commands::get_activity_stats,
            commands::google_login,
            commands::start_google_auth,
            commands::validate_token,
            commands::logout,
            commands::quit_app,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
