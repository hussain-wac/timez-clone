use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use timez_core::protocol::{Request, RequestEnvelope, ResponseData, ResponseEnvelope};

use crate::REQUEST_TOKEN;

pub fn parse_parent_pid() -> Option<u32> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--parent-pid" {
            return args.next().and_then(|value| value.parse::<u32>().ok());
        }
    }
    None
}

pub fn run_server<F>(socket_path: PathBuf, parent_pid: Option<u32>, handler: F) -> Result<(), String>
where
    F: Fn(Request) -> Result<ResponseData, String> + Send + Sync + 'static,
{
    remove_stale_socket(&socket_path);
    let listener = UnixListener::bind(&socket_path)
        .map_err(|err| format!("Failed to bind {}: {err}", socket_path.display()))?;

    spawn_parent_watchdog(parent_pid, socket_path.clone());

    for stream in listener.incoming() {
        let shutdown = match stream {
            Ok(stream) => handle_stream(stream, &handler),
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
    Ok(())
}

pub fn send_request(
    socket_path: &Path,
    request: Request,
) -> Result<ResponseData, String> {
    let mut stream = UnixStream::connect(socket_path).map_err(|err| err.to_string())?;
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

fn handle_stream<F>(stream: UnixStream, handler: &F) -> bool
where
    F: Fn(Request) -> Result<ResponseData, String>,
{
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
    let response = match handler(request) {
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

    let _ = write_response(reader.get_mut(), response);
    shutdown
}

fn write_response(stream: &mut UnixStream, response: ResponseEnvelope) -> Result<(), String> {
    let payload = serde_json::to_string(&response).map_err(|err| err.to_string())?;
    stream
        .write_all(payload.as_bytes())
        .map_err(|err| err.to_string())?;
    stream.write_all(b"\n").map_err(|err| err.to_string())?;
    stream.flush().map_err(|err| err.to_string())
}

fn spawn_parent_watchdog(parent_pid: Option<u32>, socket_path: PathBuf) {
    let Some(parent_pid) = parent_pid else {
        return;
    };

    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(2));
        if !Path::new(&format!("/proc/{parent_pid}")).exists() {
            remove_stale_socket(&socket_path);
            std::process::exit(0);
        }
    });
}

fn remove_stale_socket(path: &Path) {
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
}
