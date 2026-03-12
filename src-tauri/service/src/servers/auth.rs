use timez_core::api;
use timez_core::protocol::{Request, ResponseData};

use crate::auth_store;
use crate::runtime;
use crate::ServiceKind;

pub fn run(parent_pid: Option<u32>) -> Result<(), String> {
    runtime::run_server(ServiceKind::Auth.socket_path(), parent_pid, move |request| {
        handle_request(request)
    })
}

fn handle_request(request: Request) -> Result<ResponseData, String> {
    match request {
        Request::GoogleLogin { google_id_token } => {
            let response = api::google_login(&google_id_token)?;
            auth_store::write_token(Some(response.access_token.clone()))?;
            notify_task_refresh();
            Ok(ResponseData::AuthResponse(response))
        }
        Request::StartGoogleAuth {
            client_id,
            client_secret,
        } => {
            let response = api::google_oauth_via_browser(&client_id, &client_secret)?;
            auth_store::write_token(Some(response.access_token.clone()))?;
            notify_task_refresh();
            Ok(ResponseData::AuthResponse(response))
        }
        Request::ValidateToken { token } => {
            let user = api::get_me(&token)?;
            auth_store::write_token(Some(token))?;
            notify_task_refresh();
            Ok(ResponseData::AuthUser(user))
        }
        Request::Logout => {
            auth_store::write_token(None)?;
            Ok(ResponseData::Unit)
        }
        Request::Shutdown => Ok(ResponseData::Unit),
        _ => Err("Unsupported request for auth service".to_string()),
    }
}

fn notify_task_refresh() {
    let _ = runtime::send_request(&ServiceKind::Task.socket_path(), Request::RefreshTasks);
}
