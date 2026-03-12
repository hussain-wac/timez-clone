use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::Manager;
use timez_core::models::{ActivityStats, AuthResponse, AuthUser, IdleEvent, Task, TimerStatus};
use timez_core::protocol::{Request, RequestEnvelope, ResponseData, ResponseEnvelope};

const SOCKET_PATH: &str = "/tmp/timez-service.sock";
const REQUEST_TOKEN: &str = "timez-local";

pub struct ServiceManager {
    socket_path: PathBuf,
    child: Mutex<Option<Child>>,
}

impl ServiceManager {
    pub fn new() -> Self {
        Self {
            socket_path: PathBuf::from(SOCKET_PATH),
            child: Mutex::new(None),
        }
    }

    pub fn ensure_running<R: tauri::Runtime>(
        &self,
        app_handle: &tauri::AppHandle<R>,
    ) -> Result<(), String> {
        if self.try_connect().is_ok() {
            return Ok(());
        }

        let child = spawn_service_process(app_handle)?;

        {
            let mut slot = self.child.lock().map_err(|err| err.to_string())?;
            *slot = Some(child);
        }

        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            {
                let mut slot = self.child.lock().map_err(|err| err.to_string())?;
                if let Some(child) = slot.as_mut() {
                    if let Some(status) = child.try_wait().map_err(|err| err.to_string())? {
                        *slot = None;
                        return Err(format!("Service exited during startup with status {status}"));
                    }
                }
            }

            if self.try_connect().is_ok() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        Err("Service did not start in time".to_string())
    }

    pub fn send(&self, request: Request) -> Result<ResponseData, String> {
        let mut stream = self.try_connect()?;
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
            return Err(response.error.unwrap_or_else(|| "Unknown service error".to_string()));
        }

        response
            .data
            .ok_or_else(|| "Missing response payload".to_string())
    }

    pub fn shutdown(&self) {
        let _ = self.send(Request::Shutdown);
        if let Ok(mut child) = self.child.lock() {
            if let Some(mut child) = child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    fn try_connect(&self) -> Result<UnixStream, String> {
        UnixStream::connect(&self.socket_path).map_err(|err| err.to_string())
    }
}

fn spawn_service_process<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
) -> Result<Child, String> {
    let parent_pid = std::process::id().to_string();

    if let Ok(service_bin) = resolve_service_binary(app_handle) {
        return Command::new(&service_bin)
            .arg("--parent-pid")
            .arg(&parent_pid)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|err| format!("Failed to start service: {err}"));
    }

    if cfg!(debug_assertions) {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        return Command::new("cargo")
            .arg("run")
            .arg("-p")
            .arg("timez-service")
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
            .map_err(|err| format!("Failed to start service through cargo: {err}"));
    }

    Err("Unable to locate timez-service executable".to_string())
}

fn resolve_service_binary<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
) -> Result<PathBuf, String> {
    let current_exe = std::env::current_exe().map_err(|err| err.to_string())?;
    if let Some(parent) = current_exe.parent() {
        let candidate = parent.join("timez-service");
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    let resource_dir = app_handle
        .path()
        .resource_dir()
        .map_err(|err| err.to_string())?;
    let bundled = resource_dir.join("timez-service");
    if bundled.exists() {
        return Ok(bundled);
    }

    Err("Unable to locate timez-service executable".to_string())
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
