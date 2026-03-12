// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use single_instance::SingleInstance;

fn main() {
    let instance = match SingleInstance::new("com.wac.timez-clone") {
        Ok(instance) => instance,
        Err(err) => {
            eprintln!("[single-instance] Failed to create lock: {err}");
            return;
        }
    };

    if !instance.is_single() {
        eprintln!("[single-instance] App already running; exiting.");
        return;
    }

    timez_pro_lib::run();
}
