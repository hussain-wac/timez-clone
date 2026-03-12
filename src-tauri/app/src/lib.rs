mod ipc;
mod instance;

use std::time::Duration;

use ipc::ServiceManager;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, State};
use timez_core::models::{ActivityStats, AuthResponse, AuthUser, Task, TimerStatus};
use timez_core::protocol::Request;

const POLL_INTERVAL_SECS: u64 = 2;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            instance::spawn_show_listener(app.handle().clone())?;

            let service = ServiceManager::new();
            service.ensure_running(&app.handle())?;
            app.manage(service);

            let show_item = MenuItemBuilder::with_id("show", "Show Window").build(app)?;
            let toggle_item = MenuItemBuilder::with_id("toggle_timer", "Pause/Resume Task").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let menu = MenuBuilder::new(app)
                .item(&show_item)
                .item(&toggle_item)
                .separator()
                .item(&quit_item)
                .build()?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Timez Clone")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => focus_main_window(app),
                    "quit" => {
                        let running = app
                            .state::<ServiceManager>()
                            .send(Request::GetStatus)
                            .and_then(ipc::decode_status)
                            .map(|status| status.running)
                            .unwrap_or(false);
                        let _ = app.emit("request-quit-confirm", running);
                        focus_main_window(app);
                    }
                    "toggle_timer" => {
                        let service = app.state::<ServiceManager>();
                        match service.send(Request::GetStatus).and_then(ipc::decode_status) {
                            Ok(status) if status.running => {
                                if service
                                    .send(Request::StopTimer)
                                    .and_then(ipc::decode_tasks)
                                    .is_ok()
                                {
                                    let _ = app.emit("timer-stopped", ());
                                }
                            }
                            Ok(_) => {
                                let tasks = service
                                    .send(Request::ListTasks)
                                    .and_then(ipc::decode_tasks)
                                    .unwrap_or_default();
                                let task_id = tasks
                                    .iter()
                                    .find(|task| task.running)
                                    .map(|task| task.id)
                                    .or_else(|| {
                                        tasks.iter().max_by_key(|task| task.elapsed_secs).map(|task| task.id)
                                    });
                                if let Some(task_id) = task_id {
                                    let _ = service
                                        .send(Request::StartTimer { task_id })
                                        .and_then(ipc::decode_tasks);
                                } else {
                                    focus_main_window(app);
                                }
                            }
                            Err(_) => focus_main_window(app),
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click { .. } = event {
                        focus_main_window(tray.app_handle());
                    }
                })
                .build(app)?;

            spawn_event_bridge(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_tasks,
            start_timer,
            stop_timer,
            get_status,
            add_idle_time,
            discard_idle_time,
            refresh_tasks,
            get_activity_stats,
            google_login,
            start_google_auth,
            validate_token,
            logout,
            quit_app,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn focus_main_window<R: tauri::Runtime, M: Manager<R>>(manager: &M) {
    if let Some(window) = manager.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn spawn_event_bridge<R: tauri::Runtime>(app_handle: tauri::AppHandle<R>) {
    std::thread::spawn(move || {
        let mut last_running = false;

        loop {
            std::thread::sleep(Duration::from_secs(POLL_INTERVAL_SECS));

            let service = app_handle.state::<ServiceManager>();

            if let Ok(activity) = service
                .send(Request::GetActivityStats)
                .and_then(ipc::decode_activity)
            {
                let _ = app_handle.emit("activity-update", activity);
            }

            if let Ok(Some(idle_event)) = service
                .send(Request::TakeIdleEvent)
                .and_then(ipc::decode_idle_event)
            {
                let _ = app_handle.emit("idle-detected", idle_event);
                let _ = app_handle.emit("timer-stopped", ());
            }

            if let Ok(status) = service.send(Request::GetStatus).and_then(ipc::decode_status) {
                if last_running && !status.running {
                    let _ = app_handle.emit("timer-stopped", ());
                }
                last_running = status.running;
            }
        }
    });
}

fn request(service: State<'_, ServiceManager>, request: Request) -> Result<timez_core::protocol::ResponseData, String> {
    service.send(request)
}

#[tauri::command]
fn list_tasks(service: State<'_, ServiceManager>) -> Result<Vec<Task>, String> {
    ipc::decode_tasks(request(service, Request::ListTasks)?)
}

#[tauri::command]
fn start_timer(task_id: i64, service: State<'_, ServiceManager>) -> Result<Vec<Task>, String> {
    ipc::decode_tasks(request(service, Request::StartTimer { task_id })?)
}

#[tauri::command]
fn stop_timer(service: State<'_, ServiceManager>) -> Result<Vec<Task>, String> {
    ipc::decode_tasks(request(service, Request::StopTimer)?)
}

#[tauri::command]
fn get_status(service: State<'_, ServiceManager>) -> Result<TimerStatus, String> {
    ipc::decode_status(request(service, Request::GetStatus)?)
}

#[tauri::command]
fn add_idle_time(
    task_id: i64,
    duration_secs: i64,
    service: State<'_, ServiceManager>,
) -> Result<Vec<Task>, String> {
    ipc::decode_tasks(request(
        service,
        Request::AddIdleTime {
            task_id,
            duration_secs,
        },
    )?)
}

#[tauri::command]
fn discard_idle_time(task_id: i64, service: State<'_, ServiceManager>) -> Result<Vec<Task>, String> {
    ipc::decode_tasks(request(service, Request::DiscardIdleTime { task_id })?)
}

#[tauri::command]
fn refresh_tasks(service: State<'_, ServiceManager>) -> Result<Vec<Task>, String> {
    ipc::decode_tasks(request(service, Request::RefreshTasks)?)
}

#[tauri::command]
fn get_activity_stats(service: State<'_, ServiceManager>) -> Result<ActivityStats, String> {
    ipc::decode_activity(request(service, Request::GetActivityStats)?)
}

#[tauri::command]
fn google_login(
    google_id_token: String,
    service: State<'_, ServiceManager>,
) -> Result<AuthResponse, String> {
    ipc::decode_auth_response(request(
        service,
        Request::GoogleLogin { google_id_token },
    )?)
}

#[tauri::command]
fn start_google_auth(
    client_id: String,
    client_secret: String,
    app_handle: tauri::AppHandle,
    service: State<'_, ServiceManager>,
) -> Result<AuthResponse, String> {
    let response = ipc::decode_auth_response(request(
        service,
        Request::StartGoogleAuth {
            client_id,
            client_secret,
        },
    )?)?;
    focus_main_window(&app_handle);
    Ok(response)
}

#[tauri::command]
fn validate_token(token: String, service: State<'_, ServiceManager>) -> Result<AuthUser, String> {
    ipc::decode_auth_user(request(service, Request::ValidateToken { token })?)
}

#[tauri::command]
fn logout(service: State<'_, ServiceManager>) -> Result<(), String> {
    ipc::decode_unit(request(service, Request::Logout)?)
}

#[tauri::command]
fn quit_app(app_handle: tauri::AppHandle, service: State<'_, ServiceManager>) -> Result<(), String> {
    service.shutdown();
    app_handle.exit(0);
    Ok(())
}
