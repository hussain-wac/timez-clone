use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use timez_core::api::{self, AuthTokenState};
use timez_core::idle::{self, ActivityState, ActivityTracker};
use timez_core::models::{AuthResponse, AuthUser, IdleEvent, Task, TimerStatus};
use timez_core::protocol::{Request, RequestEnvelope, ResponseData, ResponseEnvelope};
use timez_core::timer_state::TimerStateInner;

const SOCKET_PATH: &str = "/tmp/timez-service.sock";
const REQUEST_TOKEN: &str = "timez-local";

struct ServiceState {
    auth_state: Arc<Mutex<AuthTokenState>>,
    timer_state: Arc<Mutex<TimerStateInner>>,
    activity_state: Arc<ActivityState>,
    pending_idle_event: Arc<Mutex<Option<IdleEvent>>>,
}

fn main() {
    let parent_pid = parse_parent_pid();
    let socket_path = PathBuf::from(SOCKET_PATH);
    remove_stale_socket(&socket_path);

    let listener = match UnixListener::bind(&socket_path) {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("[service] Failed to bind {}: {err}", socket_path.display());
            return;
        }
    };

    let state = ServiceState {
        auth_state: Arc::new(Mutex::new(AuthTokenState::new())),
        timer_state: Arc::new(Mutex::new(TimerStateInner::new())),
        activity_state: Arc::new(Mutex::new(ActivityTracker::new())),
        pending_idle_event: Arc::new(Mutex::new(None)),
    };

    timez_core::timer_state::spawn_sync_thread(
        Arc::clone(&state.timer_state),
        Arc::clone(&state.auth_state),
    );
    idle::spawn_idle_monitor(
        Arc::clone(&state.activity_state),
        Arc::clone(&state.timer_state),
        Arc::clone(&state.auth_state),
        Arc::clone(&state.pending_idle_event),
        60,
    );
    spawn_parent_watchdog(parent_pid, socket_path.clone());

    for stream in listener.incoming() {
        let shutdown = match stream {
            Ok(stream) => handle_stream(stream, &state),
            Err(err) => {
                eprintln!("[service] Failed to accept connection: {err}");
                false
            }
        };

        if shutdown {
            break;
        }
    }

    remove_stale_socket(&socket_path);
}

fn parse_parent_pid() -> Option<u32> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--parent-pid" {
            return args.next().and_then(|value| value.parse::<u32>().ok());
        }
    }
    None
}

fn spawn_parent_watchdog(parent_pid: Option<u32>, socket_path: PathBuf) {
    let Some(parent_pid) = parent_pid else {
        return;
    };

    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(2));

        if !process_exists(parent_pid) {
            remove_stale_socket(&socket_path);
            std::process::exit(0);
        }
    });
}

fn process_exists(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

fn handle_stream(stream: UnixStream, state: &ServiceState) -> bool {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if let Err(err) = reader.read_line(&mut line) {
        eprintln!("[service] Failed to read request: {err}");
        return false;
    }

    let envelope = match serde_json::from_str::<RequestEnvelope>(&line) {
        Ok(envelope) => envelope,
        Err(err) => {
            let _ = write_response(
                reader.get_mut(),
                ResponseEnvelope {
                    ok: false,
                    data: None,
                    error: Some(format!("Invalid request: {err}")),
                },
            );
            return false;
        }
    };

    if envelope.token != REQUEST_TOKEN {
        let _ = write_response(
            reader.get_mut(),
            ResponseEnvelope {
                ok: false,
                data: None,
                error: Some("Unauthorized request".to_string()),
            },
        );
        return false;
    }

    let request = envelope.request;
    let shutdown = matches!(request, Request::Shutdown);
    let response = match handle_request(request, state) {
        Ok(data) => ResponseEnvelope {
            ok: true,
            data: Some(data),
            error: None,
        },
        Err(error) => ResponseEnvelope {
            ok: false,
            data: None,
            error: Some(error),
        },
    };

    if let Err(err) = write_response(reader.get_mut(), response) {
        eprintln!("[service] Failed to write response: {err}");
    }

    shutdown
}

fn handle_request(request: Request, state: &ServiceState) -> Result<ResponseData, String> {
    match request {
        Request::ListTasks => Ok(ResponseData::Tasks(list_tasks(state)?)),
        Request::StartTimer { task_id } => Ok(ResponseData::Tasks(start_timer(state, task_id)?)),
        Request::StopTimer => Ok(ResponseData::Tasks(stop_timer(state)?)),
        Request::GetStatus => Ok(ResponseData::Status(get_status(state)?)),
        Request::AddIdleTime {
            task_id,
            duration_secs,
        } => Ok(ResponseData::Tasks(add_idle_time(state, task_id, duration_secs)?)),
        Request::DiscardIdleTime { task_id } => {
            Ok(ResponseData::Tasks(discard_idle_time(state, task_id)?))
        }
        Request::RefreshTasks => Ok(ResponseData::Tasks(refresh_tasks(state)?)),
        Request::GetActivityStats => Ok(ResponseData::Activity(get_activity_stats(state)?)),
        Request::GoogleLogin { google_id_token } => {
            Ok(ResponseData::AuthResponse(google_login(state, &google_id_token)?))
        }
        Request::StartGoogleAuth {
            client_id,
            client_secret,
        } => Ok(ResponseData::AuthResponse(start_google_auth(
            state,
            &client_id,
            &client_secret,
        )?)),
        Request::ValidateToken { token } => {
            Ok(ResponseData::AuthUser(validate_token(state, &token)?))
        }
        Request::Logout => {
            logout(state)?;
            Ok(ResponseData::Unit)
        }
        Request::TakeIdleEvent => Ok(ResponseData::IdleEvent(take_idle_event(state)?)),
        Request::Shutdown => Ok(ResponseData::Unit),
    }
}

fn list_tasks(state: &ServiceState) -> Result<Vec<Task>, String> {
    let timer = state.timer_state.lock().map_err(|err| err.to_string())?;
    Ok(timer.get_tasks())
}

fn start_timer(state: &ServiceState, task_id: i64) -> Result<Vec<Task>, String> {
    let token = current_token(state)?;
    let mut timer = state.timer_state.lock().map_err(|err| err.to_string())?;
    timer.start_task(task_id, &token)?;
    Ok(timer.get_tasks())
}

fn stop_timer(state: &ServiceState) -> Result<Vec<Task>, String> {
    let token = current_token(state)?;
    let mut timer = state.timer_state.lock().map_err(|err| err.to_string())?;
    timer.stop_current(&token)?;
    Ok(timer.get_tasks())
}

fn get_status(state: &ServiceState) -> Result<TimerStatus, String> {
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

fn add_idle_time(
    state: &ServiceState,
    task_id: i64,
    duration_secs: i64,
) -> Result<Vec<Task>, String> {
    let token = current_token(state)?;
    let mut timer = state.timer_state.lock().map_err(|err| err.to_string())?;
    timer.resume_with_idle_time(task_id, duration_secs, &token)?;
    Ok(timer.get_tasks())
}

fn discard_idle_time(state: &ServiceState, task_id: i64) -> Result<Vec<Task>, String> {
    let token = current_token(state)?;
    let mut timer = state.timer_state.lock().map_err(|err| err.to_string())?;
    timer.start_task(task_id, &token)?;
    Ok(timer.get_tasks())
}

fn refresh_tasks(state: &ServiceState) -> Result<Vec<Task>, String> {
    let token = current_token(state)?;
    let mut timer = state.timer_state.lock().map_err(|err| err.to_string())?;
    timer.sync_from_api(&token);
    Ok(timer.get_tasks())
}

fn get_activity_stats(state: &ServiceState) -> Result<timez_core::models::ActivityStats, String> {
    let activity = state.activity_state.lock().map_err(|err| err.to_string())?;
    Ok(activity.stats())
}

fn google_login(state: &ServiceState, google_id_token: &str) -> Result<AuthResponse, String> {
    let response = api::google_login(google_id_token)?;
    store_access_token(state, &response.access_token)?;
    sync_after_auth(state, Some(response.access_token.clone()))?;
    Ok(response)
}

fn start_google_auth(
    state: &ServiceState,
    client_id: &str,
    client_secret: &str,
) -> Result<AuthResponse, String> {
    let response = api::google_oauth_via_browser(client_id, client_secret)?;
    store_access_token(state, &response.access_token)?;
    sync_after_auth(state, Some(response.access_token.clone()))?;
    Ok(response)
}

fn validate_token(state: &ServiceState, token: &str) -> Result<AuthUser, String> {
    let user = api::get_me(token)?;
    store_access_token(state, token)?;
    sync_after_auth(state, Some(token.to_string()))?;
    Ok(user)
}

fn logout(state: &ServiceState) -> Result<(), String> {
    let mut auth = state.auth_state.lock().map_err(|err| err.to_string())?;
    auth.access_token = None;
    Ok(())
}

fn take_idle_event(state: &ServiceState) -> Result<Option<IdleEvent>, String> {
    let mut pending = state
        .pending_idle_event
        .lock()
        .map_err(|err| err.to_string())?;
    Ok(pending.take())
}

fn sync_after_auth(state: &ServiceState, token: Option<String>) -> Result<(), String> {
    let mut timer = state.timer_state.lock().map_err(|err| err.to_string())?;
    timer.sync_from_api(&token);
    Ok(())
}

fn store_access_token(state: &ServiceState, token: &str) -> Result<(), String> {
    let mut auth = state.auth_state.lock().map_err(|err| err.to_string())?;
    auth.access_token = Some(token.to_string());
    Ok(())
}

fn current_token(state: &ServiceState) -> Result<Option<String>, String> {
    let auth = state.auth_state.lock().map_err(|err| err.to_string())?;
    Ok(auth.access_token.clone())
}

fn write_response(stream: &mut UnixStream, response: ResponseEnvelope) -> Result<(), String> {
    let payload = serde_json::to_string(&response).map_err(|err| err.to_string())?;
    stream
        .write_all(payload.as_bytes())
        .map_err(|err| err.to_string())?;
    stream.write_all(b"\n").map_err(|err| err.to_string())?;
    stream.flush().map_err(|err| err.to_string())
}

fn remove_stale_socket(path: &Path) {
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
}
