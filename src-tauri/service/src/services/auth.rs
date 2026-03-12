use timez_core::api;
use timez_core::models::{AuthResponse, AuthUser};

use crate::services::task;
use crate::state::ServiceState;

pub fn google_login(state: &ServiceState, google_id_token: &str) -> Result<AuthResponse, String> {
    let response = api::google_login(google_id_token)?;
    state.store_access_token(&response.access_token)?;
    task::sync_after_auth(state, Some(response.access_token.clone()))?;
    Ok(response)
}

pub fn start_google_auth(
    state: &ServiceState,
    client_id: &str,
    client_secret: &str,
) -> Result<AuthResponse, String> {
    let response = api::google_oauth_via_browser(client_id, client_secret)?;
    state.store_access_token(&response.access_token)?;
    task::sync_after_auth(state, Some(response.access_token.clone()))?;
    Ok(response)
}

pub fn validate_token(state: &ServiceState, token: &str) -> Result<AuthUser, String> {
    let user = api::get_me(token)?;
    state.store_access_token(token)?;
    task::sync_after_auth(state, Some(token.to_string()))?;
    Ok(user)
}

pub fn logout(state: &ServiceState) -> Result<(), String> {
    state.clear_access_token()
}
