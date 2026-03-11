use tauri::{AppHandle, Manager, State};

use crate::api::{self, AuthToken};
use crate::idle::ActivityState;
use crate::models::{ActivityStats, AuthResponse, AuthUser, Task};
use crate::timer_state::TimerState;

fn get_token(auth: &State<'_, AuthToken>) -> Option<String> {
    auth.inner().lock().ok().and_then(|s| s.access_token.clone())
}

#[tauri::command]
pub fn list_tasks(timer: State<'_, TimerState>, _auth: State<'_, AuthToken>) -> Result<Vec<Task>, String> {
    let s = timer.lock().map_err(|e| e.to_string())?;
    Ok(s.get_tasks())
}

#[tauri::command]
pub fn start_timer(task_id: i64, timer: State<'_, TimerState>, auth: State<'_, AuthToken>) -> Result<Vec<Task>, String> {
    let token = get_token(&auth);
    let mut s = timer.lock().map_err(|e| e.to_string())?;
    s.start_task(task_id, &token)?;
    Ok(s.get_tasks())
}

#[tauri::command]
pub fn stop_timer(timer: State<'_, TimerState>, auth: State<'_, AuthToken>) -> Result<Vec<Task>, String> {
    let token = get_token(&auth);
    let mut s = timer.lock().map_err(|e| e.to_string())?;
    s.stop_current(&token)?;
    Ok(s.get_tasks())
}

#[tauri::command]
pub fn get_status(timer: State<'_, TimerState>) -> Result<crate::models::TimerStatus, String> {
    let s = timer.lock().map_err(|e| e.to_string())?;
    Ok(crate::models::TimerStatus {
        running: s.running_task_id.is_some(),
        active_task_id: s.running_task_id,
        current_entry_elapsed: s.timer_started_at
            .map(|started| (chrono::Utc::now() - started).num_seconds().max(0))
            .unwrap_or(0),
    })
}

#[tauri::command]
pub fn add_idle_time(task_id: i64, duration_secs: i64, timer: State<'_, TimerState>, auth: State<'_, AuthToken>) -> Result<Vec<Task>, String> {
    let token = get_token(&auth);
    let mut s = timer.lock().map_err(|e| e.to_string())?;
    s.resume_with_idle_time(task_id, duration_secs, &token)?;
    Ok(s.get_tasks())
}

#[tauri::command]
pub fn discard_idle_time(task_id: i64, timer: State<'_, TimerState>, auth: State<'_, AuthToken>) -> Result<Vec<Task>, String> {
    let token = get_token(&auth);
    let mut s = timer.lock().map_err(|e| e.to_string())?;
    s.start_task(task_id, &token)?;
    Ok(s.get_tasks())
}

#[tauri::command]
pub fn refresh_tasks(timer: State<'_, TimerState>, auth: State<'_, AuthToken>) -> Result<Vec<Task>, String> {
    let token = get_token(&auth);
    let mut s = timer.lock().map_err(|e| e.to_string())?;
    s.sync_from_api(&token);
    Ok(s.get_tasks())
}

#[tauri::command]
pub fn get_activity_stats(activity: State<'_, ActivityState>) -> Result<ActivityStats, String> {
    let tracker = activity.lock().map_err(|e| e.to_string())?;
    Ok(tracker.stats())
}

#[tauri::command]
pub fn google_login(google_id_token: String, auth: State<'_, AuthToken>, timer: State<'_, TimerState>) -> Result<AuthResponse, String> {
    let response = api::google_login(&google_id_token)?;

    // Store the access token
    {
        let mut auth_state = auth.inner().lock().map_err(|e| e.to_string())?;
        auth_state.access_token = Some(response.access_token.clone());
    }

    // Sync tasks now that we're authenticated
    {
        let token = Some(response.access_token.clone());
        let mut s = timer.lock().map_err(|e| e.to_string())?;
        s.sync_from_api(&token);
    }

    Ok(response)
}

#[tauri::command]
pub fn start_google_auth(client_id: String, client_secret: String, app_handle: AppHandle, auth: State<'_, AuthToken>, timer: State<'_, TimerState>) -> Result<AuthResponse, String> {
    let response = api::google_oauth_via_browser(&client_id, &client_secret)?;

    // Store the access token
    {
        let mut auth_state = auth.inner().lock().map_err(|e| e.to_string())?;
        auth_state.access_token = Some(response.access_token.clone());
    }

    // Sync tasks now that we're authenticated
    {
        let token = Some(response.access_token.clone());
        let mut s = timer.lock().map_err(|e| e.to_string())?;
        s.sync_from_api(&token);
    }

    // Bring the app window to front after successful login
    if let Some(window) = app_handle.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }

    Ok(response)
}

#[tauri::command]
pub fn validate_token(token: String, auth: State<'_, AuthToken>, timer: State<'_, TimerState>) -> Result<AuthUser, String> {
    let user = api::get_me(&token)?;

    // Restore the token in state
    {
        let mut auth_state = auth.inner().lock().map_err(|e| e.to_string())?;
        auth_state.access_token = Some(token.clone());
    }

    // Sync tasks
    {
        let mut s = timer.lock().map_err(|e| e.to_string())?;
        s.sync_from_api(&Some(token));
    }

    Ok(user)
}

#[tauri::command]
pub fn logout(auth: State<'_, AuthToken>) -> Result<(), String> {
    let mut auth_state = auth.inner().lock().map_err(|e| e.to_string())?;
    auth_state.access_token = None;
    Ok(())
}

#[tauri::command]
pub fn quit_app(app_handle: AppHandle) -> Result<(), String> {
    app_handle.exit(0);
    Ok(())
}
