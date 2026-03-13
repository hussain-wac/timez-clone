use tauri::{AppHandle, State};

use crate::api::AuthToken;
use crate::idle::ActivityState;
use crate::local_store::LocalTimeStorage;
use crate::models::{ActivityStats, AuthResponse, AuthUser, Task, TimerStatus};
use crate::services;
use crate::timer_state::TimerState;

#[tauri::command]
pub fn list_tasks(
    timer: State<'_, TimerState>,
    _auth: State<'_, AuthToken>,
) -> Result<Vec<Task>, String> {
    services::tasks::list_tasks(timer)
}

#[tauri::command]
pub fn start_timer(
    task_id: i64,
    timer: State<'_, TimerState>,
    auth: State<'_, AuthToken>,
    local_store: State<'_, LocalTimeStorage>,
) -> Result<Vec<Task>, String> {
    services::tasks::start_timer(task_id, timer, auth, local_store)
}

#[tauri::command]
pub fn stop_timer(
    timer: State<'_, TimerState>,
    auth: State<'_, AuthToken>,
    local_store: State<'_, LocalTimeStorage>,
) -> Result<Vec<Task>, String> {
    services::tasks::stop_timer(timer, auth, local_store)
}

#[tauri::command]
pub fn get_status(timer: State<'_, TimerState>) -> Result<TimerStatus, String> {
    services::tasks::get_status(timer)
}

#[tauri::command]
pub fn add_idle_time(
    task_id: i64,
    duration_secs: i64,
    timer: State<'_, TimerState>,
    auth: State<'_, AuthToken>,
    local_store: State<'_, LocalTimeStorage>,
) -> Result<Vec<Task>, String> {
    services::tasks::add_idle_time(task_id, duration_secs, timer, auth, local_store)
}

#[tauri::command]
pub fn discard_idle_time(
    task_id: i64,
    timer: State<'_, TimerState>,
    auth: State<'_, AuthToken>,
    local_store: State<'_, LocalTimeStorage>,
) -> Result<Vec<Task>, String> {
    services::tasks::discard_idle_time(task_id, timer, auth, local_store)
}

#[tauri::command]
pub fn refresh_tasks(
    timer: State<'_, TimerState>,
    auth: State<'_, AuthToken>,
) -> Result<Vec<Task>, String> {
    services::tasks::refresh_tasks(timer, auth)
}

#[tauri::command]
pub fn get_activity_stats(activity: State<'_, ActivityState>) -> Result<ActivityStats, String> {
    services::activity::get_activity_stats(activity)
}

#[tauri::command]
pub fn google_login(
    google_id_token: String,
    auth: State<'_, AuthToken>,
    timer: State<'_, TimerState>,
    local_store: State<'_, LocalTimeStorage>,
) -> Result<AuthResponse, String> {
    services::auth::google_login(google_id_token, auth, timer, local_store)
}

#[tauri::command]
pub fn start_google_auth(
    client_id: String,
    client_secret: String,
    app_handle: AppHandle,
    auth: State<'_, AuthToken>,
    timer: State<'_, TimerState>,
    local_store: State<'_, LocalTimeStorage>,
) -> Result<String, String> {
    services::auth::start_google_auth(
        client_id,
        client_secret,
        app_handle,
        auth,
        timer,
        local_store,
    )
}

#[tauri::command]
pub fn validate_token(
    token: String,
    auth: State<'_, AuthToken>,
    timer: State<'_, TimerState>,
    local_store: State<'_, LocalTimeStorage>,
) -> Result<AuthUser, String> {
    services::auth::validate_token(token, auth, timer, local_store)
}

#[tauri::command]
pub fn logout(
    auth: State<'_, AuthToken>,
    local_store: State<'_, LocalTimeStorage>,
) -> Result<(), String> {
    services::auth::logout(auth, local_store)
}

#[tauri::command]
pub fn quit_app(app_handle: AppHandle) -> Result<(), String> {
    services::quit::quit_app(app_handle)
}
