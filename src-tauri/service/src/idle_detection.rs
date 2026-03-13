use std::process::Command;
use std::time::Duration;

use dbus::blocking::Connection;

pub fn connect_session_bus() -> Result<Connection, String> {
    Connection::new_session().map_err(|err| err.to_string())
}

pub fn get_idle_duration_secs(conn: &Connection) -> Result<u64, String> {
    query_mutter_idle_ms(conn)
        .or_else(|_| query_freedesktop_idle_ms(conn))
        .or_else(|_| query_xprintidle_ms())
        .map(|idle_ms| idle_ms / 1000)
}

pub fn is_session_locked(conn: &Connection) -> bool {
    check_screen_active(conn)
}

fn check_screen_active(conn: &Connection) -> bool {
    let proxy = conn.with_proxy(
        "org.freedesktop.ScreenSaver",
        "/org/freedesktop/ScreenSaver",
        Duration::from_millis(500),
    );

    proxy
        .method_call("org.freedesktop.ScreenSaver", "GetActive", ())
        .map(|r: (bool,)| r.0)
        .unwrap_or(false)
}

fn query_mutter_idle_ms(conn: &Connection) -> Result<u64, String> {
    let proxy = conn.with_proxy(
        "org.gnome.Mutter.IdleMonitor",
        "/org/gnome/Mutter/IdleMonitor/Core",
        Duration::from_millis(2000),
    );
    let (idle_ms,): (u64,) = proxy
        .method_call("org.gnome.Mutter.IdleMonitor", "GetIdletime", ())
        .map_err(|err| err.to_string())?;
    Ok(idle_ms)
}

fn query_freedesktop_idle_ms(conn: &Connection) -> Result<u64, String> {
    let proxy = conn.with_proxy(
        "org.freedesktop.ScreenSaver",
        "/org/freedesktop/ScreenSaver",
        Duration::from_millis(2000),
    );
    let (idle_ms,): (u32,) = proxy
        .method_call("org.freedesktop.ScreenSaver", "GetSessionIdleTime", ())
        .map_err(|err| err.to_string())?;
    Ok(idle_ms as u64)
}

fn query_xprintidle_ms() -> Result<u64, String> {
    let output = Command::new("xprintidle")
        .output()
        .map_err(|err| err.to_string())?;

    if !output.status.success() {
        return Err(format!("xprintidle exited with status {}", output.status));
    }

    let stdout = String::from_utf8(output.stdout).map_err(|err| err.to_string())?;
    stdout.trim().parse::<u64>().map_err(|err| err.to_string())
}
