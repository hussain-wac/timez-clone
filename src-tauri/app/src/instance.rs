use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixListener;
use std::path::Path;

use tauri::Manager;

const INSTANCE_SOCKET_PATH: &str = "/tmp/timez-app.sock";

pub fn spawn_show_listener<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
) -> Result<(), String> {
    remove_stale_socket();

    let listener = UnixListener::bind(INSTANCE_SOCKET_PATH)
        .map_err(|err| format!("Failed to bind instance socket: {err}"))?;

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else {
                continue;
            };

            let mut line = String::new();
            let mut reader = BufReader::new(stream);
            if reader.read_line(&mut line).is_ok() && line.trim() == "show" {
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }
        }
    });

    Ok(())
}

fn remove_stale_socket() {
    let path = Path::new(INSTANCE_SOCKET_PATH);
    if path.exists() {
        let _ = fs::remove_file(path);
    }
}
