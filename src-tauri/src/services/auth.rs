use tauri::{AppHandle, Manager, State};

use crate::api::{self, AuthToken};
use crate::models::{AuthResponse, AuthUser};
use crate::timer_state::TimerState;

pub fn google_login(
    google_id_token: String,
    auth: State<'_, AuthToken>,
    timer: State<'_, TimerState>,
) -> Result<AuthResponse, String> {
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

pub fn start_google_auth(
    client_id: String,
    client_secret: String,
    app_handle: AppHandle,
    auth: State<'_, AuthToken>,
    timer: State<'_, TimerState>,
) -> Result<AuthResponse, String> {
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

pub fn validate_token(
    token: String,
    auth: State<'_, AuthToken>,
    timer: State<'_, TimerState>,
) -> Result<AuthUser, String> {
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

pub fn logout(auth: State<'_, AuthToken>) -> Result<(), String> {
    let mut auth_state = auth.inner().lock().map_err(|e| e.to_string())?;
    auth_state.access_token = None;
    Ok(())
}
