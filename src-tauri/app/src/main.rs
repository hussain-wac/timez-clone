#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use single_instance::SingleInstance;
use std::io::Write;
use std::os::unix::net::UnixStream;

const INSTANCE_SOCKET_PATH: &str = "/tmp/timez-app.sock";

fn main() {
    std::env::set_var("GTK_WARNINGS", "0");
    std::env::set_var("LIBAPPINDICATOR_SILENCE_WARNS", "1");

    let instance = match SingleInstance::new("com.wac.timez-clone") {
        Ok(instance) => instance,
        Err(err) => {
            eprintln!("[single-instance] Failed to create lock: {err}");
            return;
        }
    };

    if !instance.is_single() {
        if let Ok(mut stream) = UnixStream::connect(INSTANCE_SOCKET_PATH) {
            let _ = stream.write_all(b"show\n");
            let _ = stream.flush();
        }
        eprintln!("[single-instance] App already running; requested existing window.");
        return;
    }

    timez_pro_lib::run();
}
