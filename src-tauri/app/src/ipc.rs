use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::Manager;
use timez_core::models::{ActivityStats, AuthResponse, AuthUser, IdleEvent, Task, TimerStatus};
use timez_core::protocol::{Request, RequestEnvelope, ResponseData, ResponseEnvelope};

const REQUEST_TOKEN: &str = "timez-local";

#[derive(Clone, Copy)]
enum ServiceKind {
    Auth,
    Task,
    Tracker,
    IdleTime,
    Quit,
}

impl ServiceKind {
    fn socket_path(self) -> PathBuf {
        PathBuf::from(match self {
            Self::Auth => "/tmp/timez-auth-service.sock",
            Self::Task => "/tmp/timez-task-service.sock",
            Self::Tracker => "/tmp/timez-tracker-service.sock",
            Self::IdleTime => "/tmp/timez-idle-time-service.sock",
            Self::Quit => "/tmp/timez-quit-service.sock",
        })
    }

    fn binary_name(self) -> &'static str {
        match self {
            Self::Auth => "timez-auth-service",
            Self::Task => "timez-task-service",
            Self::Tracker => "timez-tracker-service",
            Self::IdleTime => "timez-idle-time-service",
            Self::Quit => "timez-quit-service",
        }
    }
}

struct ManagedService {
    kind: ServiceKind,
    child: Option<Child>,
}

pub struct ServiceManager {
    services: Mutex<Vec<ManagedService>>,
}

impl ServiceManager {
    pub fn new() -> Self {
        Self {
            services: Mutex::new(Vec::new()),
        }
    }

    pub fn ensure_running<R: tauri::Runtime>(
        &self,
        app_handle: &tauri::AppHandle<R>,
    ) -> Result<(), String> {
        let service_kinds = [
            ServiceKind::Task,
            ServiceKind::Auth,
            ServiceKind::Tracker,
            ServiceKind::IdleTime,
            ServiceKind::Quit,
        ];

        let mut services = self.services.lock().map_err(|err| err.to_string())?;
        services.clear();

        for kind in service_kinds {
            if try_connect(kind).is_ok() {
                services.push(ManagedService { kind, child: None });
                continue;
            }

            let child = spawn_service_process(app_handle, kind)?;
            services.push(ManagedService {
                kind,
                child: Some(child),
            });
        }

        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            let mut all_ready = true;

            for service in services.iter_mut() {
                if let Some(child) = service.child.as_mut() {
                    if let Some(status) = child.try_wait().map_err(|err| err.to_string())? {
                        return Err(format!(
                            "{} exited during startup with status {status}",
                            service.kind.binary_name()
                        ));
                    }
                }

                if try_connect(service.kind).is_err() {
                    all_ready = false;
                }
            }

            if all_ready {
                return Ok(());
            }

            std::thread::sleep(Duration::from_millis(100));
        }

        Err("One or more services did not start in time".to_string())
    }

    pub fn send(&self, request: Request) -> Result<ResponseData, String> {
        let kind = route_request(&request);
        let mut stream = try_connect(kind)?;
        let envelope = RequestEnvelope {
            token: REQUEST_TOKEN.to_string(),
            request,
        };
        let payload = serde_json::to_string(&envelope).map_err(|err| err.to_string())?;
        stream
            .write_all(payload.as_bytes())
            .map_err(|err| err.to_string())?;
        stream.write_all(b"\n").map_err(|err| err.to_string())?;
        stream.flush().map_err(|err| err.to_string())?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|err| err.to_string())?;
        let response: ResponseEnvelope =
            serde_json::from_str(&line).map_err(|err| format!("Invalid response: {err}"))?;

        if !response.ok {
            return Err(response
                .error
                .unwrap_or_else(|| "Unknown service error".to_string()));
        }

        response
            .data
            .ok_or_else(|| "Missing response payload".to_string())
    }

    pub fn shutdown(&self) {
        if let Ok(mut services) = self.services.lock() {
            for service in services.iter() {
                let _ = send_shutdown(service.kind);
            }

            for service in services.iter_mut() {
                if let Some(mut child) = service.child.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
    }
}

fn spawn_service_process<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    kind: ServiceKind,
) -> Result<Child, String> {
    let parent_pid = std::process::id().to_string();

    if cfg!(debug_assertions) {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        return Command::new("cargo")
            .arg("run")
            .arg("-p")
            .arg("timez-service")
            .arg("--bin")
            .arg(kind.binary_name())
            .arg("--manifest-path")
            .arg(manifest_dir.join("Cargo.toml"))
            .arg("--offline")
            .arg("--")
            .arg("--parent-pid")
            .arg(&parent_pid)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .current_dir(manifest_dir)
            .spawn()
            .map_err(|err| {
                format!(
                    "Failed to start {} through cargo: {err}",
                    kind.binary_name()
                )
            });
    }

    if let Ok(service_bin) = resolve_service_binary(app_handle, kind) {
        return Command::new(&service_bin)
            .arg("--parent-pid")
            .arg(&parent_pid)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|err| format!("Failed to start {}: {err}", kind.binary_name()));
    }

    Err(format!(
        "Unable to locate {} executable",
        kind.binary_name()
    ))
}

fn resolve_service_binary<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    kind: ServiceKind,
) -> Result<PathBuf, String> {
    let current_exe = std::env::current_exe().map_err(|err| err.to_string())?;
    if let Some(parent) = current_exe.parent() {
        let candidate = parent.join(kind.binary_name());
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    let resource_dir = app_handle
        .path()
        .resource_dir()
        .map_err(|err| err.to_string())?;
    let bundled = resource_dir.join(kind.binary_name());
    if bundled.exists() {
        return Ok(bundled);
    }

    Err(format!(
        "Unable to locate {} executable",
        kind.binary_name()
    ))
}

fn try_connect(kind: ServiceKind) -> Result<UnixStream, String> {
    UnixStream::connect(kind.socket_path()).map_err(|err| err.to_string())
}

fn send_shutdown(kind: ServiceKind) -> Result<(), String> {
    let mut stream = try_connect(kind)?;
    let envelope = RequestEnvelope {
        token: REQUEST_TOKEN.to_string(),
        request: Request::Shutdown,
    };
    let payload = serde_json::to_string(&envelope).map_err(|err| err.to_string())?;
    stream
        .write_all(payload.as_bytes())
        .map_err(|err| err.to_string())?;
    stream.write_all(b"\n").map_err(|err| err.to_string())?;
    stream.flush().map_err(|err| err.to_string())
}

fn route_request(request: &Request) -> ServiceKind {
    match request {
        Request::GoogleLogin { .. }
        | Request::StartGoogleAuth { .. }
        | Request::ValidateToken { .. }
        | Request::Logout => ServiceKind::Auth,
        Request::ListTasks
        | Request::StartTimer { .. }
        | Request::StopTimer
        | Request::GetStatus
        | Request::AddIdleTime { .. }
        | Request::DiscardIdleTime { .. }
        | Request::RefreshTasks => ServiceKind::Task,
        Request::GetActivityStats => ServiceKind::Tracker,
        Request::GetIdleEvent | Request::ResolveIdleEvent => ServiceKind::IdleTime,
        Request::Shutdown => ServiceKind::Quit,
    }
}

pub fn decode_tasks(data: ResponseData) -> Result<Vec<Task>, String> {
    match data {
        ResponseData::Tasks(tasks) => Ok(tasks),
        _ => Err("Unexpected service response".to_string()),
    }
}

pub fn decode_status(data: ResponseData) -> Result<TimerStatus, String> {
    match data {
        ResponseData::Status(status) => Ok(status),
        _ => Err("Unexpected service response".to_string()),
    }
}

pub fn decode_activity(data: ResponseData) -> Result<ActivityStats, String> {
    match data {
        ResponseData::Activity(activity) => Ok(activity),
        _ => Err("Unexpected service response".to_string()),
    }
}

pub fn decode_auth_response(data: ResponseData) -> Result<AuthResponse, String> {
    match data {
        ResponseData::AuthResponse(response) => Ok(response),
        _ => Err("Unexpected service response".to_string()),
    }
}

pub fn decode_auth_user(data: ResponseData) -> Result<AuthUser, String> {
    match data {
        ResponseData::AuthUser(user) => Ok(user),
        _ => Err("Unexpected service response".to_string()),
    }
}

pub fn decode_idle_event(data: ResponseData) -> Result<Option<IdleEvent>, String> {
    match data {
        ResponseData::IdleEvent(event) => Ok(event),
        _ => Err("Unexpected service response".to_string()),
    }
}

pub fn decode_unit(data: ResponseData) -> Result<(), String> {
    match data {
        ResponseData::Unit => Ok(()),
        _ => Err("Unexpected service response".to_string()),
    }
}

pub fn send_auth_login(google_id_token: &str) -> Result<AuthResponse, String> {
    let socket_path = PathBuf::from("/tmp/timez-auth-service.sock");
    let mut stream = UnixStream::connect(&socket_path).map_err(|err| err.to_string())?;
    let envelope = RequestEnvelope {
        token: REQUEST_TOKEN.to_string(),
        request: Request::GoogleLogin {
            google_id_token: google_id_token.to_string(),
        },
    };
    let payload = serde_json::to_string(&envelope).map_err(|err| err.to_string())?;
    stream
        .write_all(payload.as_bytes())
        .map_err(|err| err.to_string())?;
    stream.write_all(b"\n").map_err(|err| err.to_string())?;
    stream.flush().map_err(|err| err.to_string())?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|err| err.to_string())?;
    let response: ResponseEnvelope =
        serde_json::from_str(&line).map_err(|err| format!("Invalid response: {err}"))?;

    if !response.ok {
        return Err(response
            .error
            .unwrap_or_else(|| "Unknown service error".to_string()));
    }

    match response.data {
        Some(ResponseData::AuthResponse(auth_response)) => Ok(auth_response),
        _ => Err("Unexpected service response".to_string()),
    }
}
