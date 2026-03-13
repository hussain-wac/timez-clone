use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use timez_core::models::ActivityStats;
use timez_core::protocol::{Request, ResponseData};

use crate::idle_detection;
use crate::runtime;
use crate::ServiceKind;

#[derive(Default)]
struct TrackerState {
    active_secs: i64,
    idle_secs: i64,
}

pub fn run(parent_pid: Option<u32>) -> Result<(), String> {
    let state = Arc::new(Mutex::new(TrackerState::default()));
    spawn_tracker(Arc::clone(&state));

    runtime::run_server(
        ServiceKind::Tracker.socket_path(),
        parent_pid,
        move |request| match request {
            Request::GetActivityStats => Ok(ResponseData::Activity(get_stats(&state)?)),
            Request::Shutdown => Ok(ResponseData::Unit),
            _ => Err("Unsupported request for tracker service".to_string()),
        },
    )
}

fn get_stats(state: &Arc<Mutex<TrackerState>>) -> Result<ActivityStats, String> {
    let tracker = state.lock().map_err(|err| err.to_string())?;
    let total = tracker.active_secs + tracker.idle_secs;
    let percent = if total > 0 {
        (tracker.active_secs as f64 / total as f64) * 100.0
    } else {
        100.0
    };

    Ok(ActivityStats {
        active_secs: tracker.active_secs,
        idle_secs: tracker.idle_secs,
        total_secs: total,
        activity_percent: (percent * 10.0).round() / 10.0,
    })
}

fn spawn_tracker(state: Arc<Mutex<TrackerState>>) {
    std::thread::spawn(move || {
        let conn = match idle_detection::connect_session_bus() {
            Ok(conn) => conn,
            Err(err) => {
                eprintln!("[tracker] D-Bus connect failed: {err}");
                return;
            }
        };

        loop {
            std::thread::sleep(Duration::from_secs(2));

            let is_locked = idle_detection::is_session_locked(&conn);
            let idle_secs = match idle_detection::get_idle_duration_secs(&conn) {
                Ok(secs) => secs,
                Err(err) => {
                    eprintln!("[tracker] Idle query failed: {err}");
                    0
                }
            };

            let user_is_idle = idle_secs >= 3 || is_locked;

            let _now = Utc::now();
            let mut tracker = match state.lock() {
                Ok(tracker) => tracker,
                Err(_) => continue,
            };

            if user_is_idle {
                tracker.idle_secs += 2;
            } else {
                tracker.active_secs += 2;
            }
        }
    });
}
