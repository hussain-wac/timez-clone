use timez_core::models::{Task, TimerStatus};

use crate::state::ServiceState;

pub fn list_tasks(state: &ServiceState) -> Result<Vec<Task>, String> {
    let timer = state.timer_state.lock().map_err(|err| err.to_string())?;
    Ok(timer.get_tasks())
}

pub fn start_timer(state: &ServiceState, task_id: i64) -> Result<Vec<Task>, String> {
    let token = state.current_token()?;
    let mut timer = state.timer_state.lock().map_err(|err| err.to_string())?;
    timer.start_task(task_id, &token)?;
    Ok(timer.get_tasks())
}

pub fn stop_timer(state: &ServiceState) -> Result<Vec<Task>, String> {
    let token = state.current_token()?;
    let mut timer = state.timer_state.lock().map_err(|err| err.to_string())?;
    timer.stop_current(&token)?;
    Ok(timer.get_tasks())
}

pub fn get_status(state: &ServiceState) -> Result<TimerStatus, String> {
    let timer = state.timer_state.lock().map_err(|err| err.to_string())?;
    Ok(TimerStatus {
        running: timer.running_task_id.is_some(),
        active_task_id: timer.running_task_id,
        current_entry_elapsed: timer
            .timer_started_at
            .map(|started| (chrono::Utc::now() - started).num_seconds().max(0))
            .unwrap_or(0),
    })
}

pub fn add_idle_time(
    state: &ServiceState,
    task_id: i64,
    duration_secs: i64,
) -> Result<Vec<Task>, String> {
    let token = state.current_token()?;
    let mut timer = state.timer_state.lock().map_err(|err| err.to_string())?;
    timer.resume_with_idle_time(task_id, duration_secs, &token)?;
    Ok(timer.get_tasks())
}

pub fn discard_idle_time(state: &ServiceState, task_id: i64) -> Result<Vec<Task>, String> {
    let token = state.current_token()?;
    let mut timer = state.timer_state.lock().map_err(|err| err.to_string())?;
    timer.start_task(task_id, &token)?;
    Ok(timer.get_tasks())
}

pub fn refresh_tasks(state: &ServiceState) -> Result<Vec<Task>, String> {
    let token = state.current_token()?;
    let mut timer = state.timer_state.lock().map_err(|err| err.to_string())?;
    timer.sync_from_api(&token);
    Ok(timer.get_tasks())
}

pub fn sync_after_auth(state: &ServiceState, token: Option<String>) -> Result<(), String> {
    let mut timer = state.timer_state.lock().map_err(|err| err.to_string())?;
    timer.sync_from_api(&token);
    Ok(())
}
