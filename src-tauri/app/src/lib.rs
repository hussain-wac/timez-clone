mod instance;
mod ipc;

use std::time::Duration;

use ipc::ServiceManager;
use tauri::image::Image;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, State};
use timez_core::api;
use timez_core::models::{ActivityStats, AuthResponse, AuthUser, IdleEvent, Task, TimerStatus};
use timez_core::protocol::Request;

const POLL_INTERVAL_SECS: u64 = 2;
const TRAY_ID: &str = "main-tray";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_deep_link::init())
        .setup(|app| {
            #[cfg(desktop)]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                app.deep_link().register("timezpro").ok();
            }

            instance::spawn_show_listener(app.handle().clone())?;

            let service = ServiceManager::new();
            service.ensure_running(&app.handle())?;
            app.manage(service);

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

            let _tray = TrayIconBuilder::with_id(TRAY_ID)
                .icon(build_status_icon(false))
                .tooltip("Timez Pro")
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
                        match service
                            .send(Request::GetStatus)
                            .and_then(ipc::decode_status)
                        {
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
                                        tasks
                                            .iter()
                                            .max_by_key(|task| task.elapsed_secs)
                                            .map(|task| task.id)
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

            update_app_running_icon(&app.handle(), false);
            spawn_event_bridge(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main"
                && matches!(event, tauri::WindowEvent::CloseRequested { .. })
            {
                let tauri::WindowEvent::CloseRequested { api, .. } = event else {
                    unreachable!();
                };
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_tasks,
            start_timer,
            stop_timer,
            get_status,
            get_idle_event,
            add_idle_time,
            discard_idle_time,
            resolve_idle_event,
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
        let _ = window.set_visible_on_all_workspaces(true);
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.set_always_on_top(true);
        let _ = window.set_always_on_top(false);
    }
}

fn maintain_idle_window_state<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>) {
    if let Some(window) = app_handle.get_webview_window("main") {
        let _ = window.set_visible_on_all_workspaces(true);
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.set_always_on_top(true);
        let _ = window.set_always_on_top(false);
    }
}

fn clear_idle_window_state<R: tauri::Runtime>(_app_handle: &tauri::AppHandle<R>) {}

fn spawn_event_bridge<R: tauri::Runtime>(app_handle: tauri::AppHandle<R>) {
    std::thread::spawn(move || {
        let mut last_running = false;
        let mut last_idle_event: Option<IdleEvent> = None;

        loop {
            std::thread::sleep(Duration::from_secs(POLL_INTERVAL_SECS));

            let service = app_handle.state::<ServiceManager>();

            if let Ok(activity) = service
                .send(Request::GetActivityStats)
                .and_then(ipc::decode_activity)
            {
                let _ = app_handle.emit("activity-update", activity);
            }

            if let Ok(idle_event) = service
                .send(Request::GetIdleEvent)
                .and_then(ipc::decode_idle_event)
            {
                if idle_event.is_some() {
                    maintain_idle_window_state(&app_handle);
                }

                if idle_event != last_idle_event {
                    match idle_event.clone() {
                        Some(event) => {
                            let should_focus = match &last_idle_event {
                                None => true,
                                Some(previous) => {
                                    previous.task_id != event.task_id
                                        || (previous.tracking_active && !event.tracking_active)
                                }
                            };
                            if should_focus {
                                focus_main_window(&app_handle);
                            }
                            let _ = app_handle.emit("idle-detected", event);
                            let _ = app_handle.emit("timer-stopped", ());
                        }
                        None => {
                            clear_idle_window_state(&app_handle);
                        }
                    }
                    last_idle_event = idle_event;
                }
            }

            if let Ok(status) = service
                .send(Request::GetStatus)
                .and_then(ipc::decode_status)
            {
                if status.running != last_running {
                    update_app_running_icon(&app_handle, status.running);
                }
                if last_running && !status.running {
                    let _ = app_handle.emit("timer-stopped", ());
                }
                last_running = status.running;
            }
        }
    });
}

fn request(
    service: State<'_, ServiceManager>,
    request: Request,
) -> Result<timez_core::protocol::ResponseData, String> {
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
fn get_idle_event(service: State<'_, ServiceManager>) -> Result<Option<IdleEvent>, String> {
    ipc::decode_idle_event(request(service, Request::GetIdleEvent)?)
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
fn discard_idle_time(
    task_id: i64,
    service: State<'_, ServiceManager>,
) -> Result<Vec<Task>, String> {
    ipc::decode_tasks(request(service, Request::DiscardIdleTime { task_id })?)
}

#[tauri::command]
fn resolve_idle_event(service: State<'_, ServiceManager>) -> Result<(), String> {
    ipc::decode_unit(request(service, Request::ResolveIdleEvent)?)
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
    ipc::decode_auth_response(request(service, Request::GoogleLogin { google_id_token })?)
}

#[tauri::command]
fn start_google_auth(
    client_id: String,
    client_secret: String,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    std::thread::spawn(move || {
        let result = api::google_oauth_via_browser(&client_id, &client_secret);

        match result {
            Ok(response) => {
                let _ = ipc::send_auth_login(&response.access_token);
                let _ = app_handle.emit("auth-success", &response);
            }
            Err(e) => {
                let _ = app_handle.emit("auth-error", &e);
            }
        }
    });

    Ok("OAuth started".to_string())
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
fn quit_app(
    app_handle: tauri::AppHandle,
    service: State<'_, ServiceManager>,
) -> Result<(), String> {
    service.shutdown();
    app_handle.exit(0);
    Ok(())
}

fn update_app_running_icon<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>, running: bool) {
    let icon = build_status_icon(running);

    if let Some(tray) = app_handle.tray_by_id(TRAY_ID) {
        let _ = tray.set_icon(Some(icon.clone()));
    }

    if let Some(window) = app_handle.get_webview_window("main") {
        let _ = window.set_icon(icon);
    }
}

fn build_status_icon(running: bool) -> Image<'static> {
    let size: usize = 64;
    let mut rgba = vec![0_u8; size * size * 4];
    let color = if running {
        [34_u8, 197_u8, 94_u8, 255_u8]
    } else {
        [239_u8, 68_u8, 68_u8, 255_u8]
    };

    let cx = 32.0_f32;
    let cy = 32.0_f32;
    let outer_r = 27.0_f32;
    let ring_w = 6.5_f32;
    let inner_r = outer_r - ring_w;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist <= outer_r && dist >= inner_r {
                put_pixel(&mut rgba, size, x, y, color);
            }
        }
    }

    draw_rounded_rect(&mut rgba, size, 29, 14, 35, 32, 4.0, color);
    draw_thick_line(&mut rgba, size, (32.0, 31.0), (45.0, 40.5), 6.0, color);
    draw_disc(&mut rgba, size, 32.0, 31.0, 3.5, color);

    Image::new_owned(rgba, size as u32, size as u32)
}

fn draw_rounded_rect(
    rgba: &mut [u8],
    size: usize,
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
    radius: f32,
    color: [u8; 4],
) {
    let width = (right.saturating_sub(left)) as f32;
    let height = (bottom.saturating_sub(top)) as f32;
    let radius = radius.min(width / 2.0).min(height / 2.0);

    for y in top..bottom {
        for x in left..right {
            let xf = x as f32 + 0.5;
            let yf = y as f32 + 0.5;
            let min_x = left as f32 + radius;
            let max_x = (right as f32 - radius).max(min_x);
            let min_y = top as f32 + radius;
            let max_y = (bottom as f32 - radius).max(min_y);
            let clamped_x = xf.clamp(min_x, max_x);
            let clamped_y = yf.clamp(min_y, max_y);
            let dx = xf - clamped_x;
            let dy = yf - clamped_y;
            if dx * dx + dy * dy <= radius * radius {
                put_pixel(rgba, size, x, y, color);
            }
        }
    }
}

fn draw_thick_line(
    rgba: &mut [u8],
    size: usize,
    start: (f32, f32),
    end: (f32, f32),
    thickness: f32,
    color: [u8; 4],
) {
    let min_x = start.0.min(end.0).floor().max(0.0) as usize;
    let max_x = start.0.max(end.0).ceil().min(size as f32 - 1.0) as usize;
    let min_y = start.1.min(end.1).floor().max(0.0) as usize;
    let max_y = start.1.max(end.1).ceil().min(size as f32 - 1.0) as usize;
    let radius = thickness / 2.0;
    let line_dx = end.0 - start.0;
    let line_dy = end.1 - start.1;
    let len_sq = line_dx * line_dx + line_dy * line_dy;

    for y in min_y.saturating_sub(4)..=(max_y + 4).min(size - 1) {
        for x in min_x.saturating_sub(4)..=(max_x + 4).min(size - 1) {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let t =
                (((px - start.0) * line_dx + (py - start.1) * line_dy) / len_sq).clamp(0.0, 1.0);
            let proj_x = start.0 + t * line_dx;
            let proj_y = start.1 + t * line_dy;
            let dx = px - proj_x;
            let dy = py - proj_y;
            if dx * dx + dy * dy <= radius * radius {
                put_pixel(rgba, size, x, y, color);
            }
        }
    }
}

fn draw_disc(rgba: &mut [u8], size: usize, cx: f32, cy: f32, radius: f32, color: [u8; 4]) {
    let min_x = (cx - radius).floor().max(0.0) as usize;
    let max_x = (cx + radius).ceil().min(size as f32 - 1.0) as usize;
    let min_y = (cy - radius).floor().max(0.0) as usize;
    let max_y = (cy + radius).ceil().min(size as f32 - 1.0) as usize;

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            if dx * dx + dy * dy <= radius * radius {
                put_pixel(rgba, size, x, y, color);
            }
        }
    }
}

fn put_pixel(rgba: &mut [u8], size: usize, x: usize, y: usize, color: [u8; 4]) {
    let idx = (y * size + x) * 4;
    rgba[idx..idx + 4].copy_from_slice(&color);
}
