use tauri::State;

use crate::api::AuthToken;
use crate::local_store::LocalTimeStorage;
use crate::models::{Task, TimerStatus};
use crate::timer_state::TimerState;

fn get_token(auth: &State<'_, AuthToken>) -> Option<String> {
    auth.inner()
        .lock()
        .ok()
        .and_then(|s| s.access_token.clone())
}

pub fn list_tasks(timer: State<'_, TimerState>) -> Result<Vec<Task>, String> {
    let s = timer.lock().map_err(|e| e.to_string())?;
    Ok(s.get_tasks())
}

pub fn start_timer(
    task_id: i64,
    timer: State<'_, TimerState>,
    auth: State<'_, AuthToken>,
    local_store: State<'_, LocalTimeStorage>,
) -> Result<Vec<Task>, String> {
    let token = get_token(&auth);
    let mut s = timer.lock().map_err(|e| e.to_string())?;
    s.start_task(task_id, &token, &local_store)?;
    Ok(s.get_tasks())
}

pub fn stop_timer(
    timer: State<'_, TimerState>,
    auth: State<'_, AuthToken>,
    local_store: State<'_, LocalTimeStorage>,
) -> Result<Vec<Task>, String> {
    let token = get_token(&auth);
    let mut s = timer.lock().map_err(|e| e.to_string())?;
    s.stop_current(&token, &local_store)?;
    Ok(s.get_tasks())
}

pub fn get_status(timer: State<'_, TimerState>) -> Result<TimerStatus, String> {
    let s = timer.lock().map_err(|e| e.to_string())?;
    Ok(TimerStatus {
        running: s.running_task_id.is_some(),
        active_task_id: s.running_task_id,
        current_entry_elapsed: s
            .timer_started_at
            .map(|started| (chrono::Utc::now() - started).num_seconds().max(0))
            .unwrap_or(0),
    })
}

pub fn add_idle_time(
    task_id: i64,
    duration_secs: i64,
    timer: State<'_, TimerState>,
    auth: State<'_, AuthToken>,
    local_store: State<'_, LocalTimeStorage>,
) -> Result<Vec<Task>, String> {
    let token = get_token(&auth);
    let mut s = timer.lock().map_err(|e| e.to_string())?;
    s.resume_with_idle_time(task_id, duration_secs, &token, &local_store)?;
    Ok(s.get_tasks())
}

pub fn discard_idle_time(
    task_id: i64,
    timer: State<'_, TimerState>,
    auth: State<'_, AuthToken>,
    local_store: State<'_, LocalTimeStorage>,
) -> Result<Vec<Task>, String> {
    let token = get_token(&auth);
    let mut s = timer.lock().map_err(|e| e.to_string())?;
    s.start_task(task_id, &token, &local_store)?;
    Ok(s.get_tasks())
}

pub fn refresh_tasks(
    timer: State<'_, TimerState>,
    auth: State<'_, AuthToken>,
) -> Result<Vec<Task>, String> {
    let token = get_token(&auth);
    let mut s = timer.lock().map_err(|e| e.to_string())?;
    s.sync_from_api(&token);
    Ok(s.get_tasks())
}
