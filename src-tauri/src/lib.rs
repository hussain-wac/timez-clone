mod api;
mod commands;
mod idle;
mod models;
mod services;
mod timer_state;

use tauri::{Emitter, Manager};
use tauri::tray::TrayIconBuilder;
use tauri::menu::{MenuBuilder, MenuItemBuilder};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Auth token shared state
            app.manage(api::AuthToken::new(api::AuthTokenState::new()));

            // Activity tracker shared state
            app.manage(idle::ActivityState::new(idle::ActivityTracker::new()));

            // Local timer state (caches tasks, tracks running timer locally)
            app.manage(timer_state::TimerState::new(timer_state::TimerStateInner::new()));

            // Build system tray menu
            let show_item = MenuItemBuilder::with_id("show", "Show Window").build(app)?;
            let toggle_item = MenuItemBuilder::with_id("toggle_timer", "Pause/Resume Task").build(app)?;
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
                .on_menu_event(|app, event| {
                    match event.id().as_ref() {
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
                                auth.inner().lock().ok().and_then(|s| s.access_token.clone())
                            };

                            let running_task_id = {
                                let timer_state = app.state::<timer_state::TimerState>();
                                timer_state.inner().lock().ok().and_then(|s| s.running_task_id)
                            };

                            if running_task_id.is_some() {
                                let timer_state = app.state::<timer_state::TimerState>();
                                if let Ok(mut s) = timer_state.inner().lock() {
                                    if s.stop_current(&token).is_ok() {
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
                                    if let Ok(mut s) = timer_state.inner().lock() {
                                        if s.start_task(task_id, &token).is_ok() {
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
                    }
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

            // Spawn idle monitor (60 second threshold = 1 minute)
            idle::spawn_idle_monitor(app.handle().clone(), 60);

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
