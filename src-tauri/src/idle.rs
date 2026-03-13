use std::sync::Mutex as StdMutex;
use std::time::Duration;

use chrono::Utc;
use dbus::blocking::Connection;
use tauri::{Emitter, Manager};

use crate::api::AuthToken;
use crate::models::{ActivityStats, IdleEvent};
use crate::timer_state::TimerState;

const POLL_INTERVAL_SECS: u64 = 2;

pub struct ActivityTracker {
    pub active_secs: i64,
    pub idle_secs: i64,
    pub last_activity_at: chrono::DateTime<Utc>,
}

impl ActivityTracker {
    pub fn new() -> Self {
        Self {
            active_secs: 0,
            idle_secs: 0,
            last_activity_at: Utc::now(),
        }
    }

    pub fn stats(&self) -> ActivityStats {
        let total = self.active_secs + self.idle_secs;
        let percent = if total > 0 {
            (self.active_secs as f64 / total as f64) * 100.0
        } else {
            100.0
        };
        ActivityStats {
            active_secs: self.active_secs,
            idle_secs: self.idle_secs,
            total_secs: total,
            activity_percent: (percent * 10.0).round() / 10.0,
        }
    }
}

pub type ActivityState = StdMutex<ActivityTracker>;

fn is_session_locked(conn: &Connection) -> bool {
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

fn check_screen_locked(conn: &Connection) -> bool {
    let proxy = conn.with_proxy(
        "org.freedesktop.ScreenSaver",
        "/org/freedesktop/ScreenSaver",
        Duration::from_millis(500),
    );

    proxy
        .method_call::<_, (bool,)>("org.freedesktop.ScreenSaver", "GetActive", ())
        .map(|(active,)| active)
        .unwrap_or(false)
}

fn check_logind(conn: &Connection) -> bool {
    let proxy = conn.with_proxy(
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        Duration::from_millis(500),
    );

    proxy
        .method_call::<_, (String,)>("org.freedesktop.login1.Manager", "GetSession", ())
        .map(|(session,)| session.contains("self"))
        .unwrap_or(false)
}

/// Helper to read the current auth token
fn get_token(app_handle: &tauri::AppHandle) -> Option<String> {
    let auth = app_handle.state::<AuthToken>();
    auth.inner()
        .lock()
        .ok()
        .and_then(|s| s.access_token.clone())
}

pub fn spawn_idle_monitor(app_handle: tauri::AppHandle, idle_threshold_secs: u64) {
    std::thread::spawn(move || {
        eprintln!(
            "[idle] Idle monitor thread started (threshold={}s)",
            idle_threshold_secs
        );

        // Reuse a single D-Bus connection for the lifetime of this thread
        let conn = match Connection::new_session() {
            Ok(c) => {
                eprintln!("[idle] D-Bus session connected");
                c
            }
            Err(e) => {
                eprintln!("[idle] FATAL: Cannot connect to D-Bus: {}", e);
                return;
            }
        };

        let mut is_idle = false;
        let mut idle_started_at: Option<chrono::DateTime<Utc>> = None;
        let mut paused_task_id: Option<i64> = None;
        let mut paused_task_name: Option<String> = None;
        let mut log_counter: u64 = 0;
        let mut was_locked = false;

        loop {
            std::thread::sleep(Duration::from_secs(POLL_INTERVAL_SECS));

            let is_locked = is_session_locked(&conn);

            // Get system-wide idle time via D-Bus (Mutter IdleMonitor)
            // If D-Bus fails (system might be sleeping), treat as idle
            let proxy = conn.with_proxy(
                "org.gnome.Mutter.IdleMonitor",
                "/org/gnome/Mutter/IdleMonitor/Core",
                Duration::from_millis(2000),
            );
            let idle_ms: u64 =
                match proxy.method_call("org.gnome.Mutter.IdleMonitor", "GetIdletime", ()) {
                    Ok((ms,)) => ms,
                    Err(e) => {
                        eprintln!("[idle] D-Bus failed (system may be sleeping): {}", e);
                        // Treat D-Bus failure as idle (system might be sleeping/suspended)
                        (idle_threshold_secs * 1000) + 1
                    }
                };

            let system_idle_secs = idle_ms / 1000;
            let user_is_active = system_idle_secs < POLL_INTERVAL_SECS + 1 && !is_locked;

            if is_locked && !was_locked {
                was_locked = true;
                eprintln!("[idle] Session locked - treating as idle");
            } else if !is_locked && was_locked {
                was_locked = false;
                eprintln!("[idle] Session unlocked");
            }

            // Log every 10 iterations (~20 seconds) to reduce noise
            log_counter += 1;
            if log_counter % 10 == 0 {
                eprintln!(
                    "[idle] status: idle_ms={}, is_idle={}, paused_task={:?}",
                    idle_ms, is_idle, paused_task_id
                );
            }

            // Update activity tracker
            {
                let state = app_handle.state::<ActivityState>();
                if let Ok(mut t) = state.inner().lock() {
                    if is_idle {
                        t.idle_secs += POLL_INTERVAL_SECS as i64;
                    } else {
                        t.active_secs += POLL_INTERVAL_SECS as i64;
                        if user_is_active {
                            t.last_activity_at = Utc::now();
                        }
                    }
                    app_handle.emit("activity-update", t.stats()).ok();
                }
            }

            if user_is_active {
                if is_idle {
                    // ============================================
                    // USER RETURNED FROM IDLE — show popup
                    // ============================================
                    let idle_secs = idle_started_at
                        .map(|start| (Utc::now() - start).num_seconds())
                        .unwrap_or(0);

                    eprintln!("[idle] User returned after {}s idle", idle_secs);

                    if let (Some(task_id), Some(ref task_name)) =
                        (paused_task_id, &paused_task_name)
                    {
                        eprintln!(
                            "[idle] Emitting idle-detected event: task_id={}, task={}, idle={}s",
                            task_id, task_name, idle_secs
                        );
                        let emit_result = app_handle.emit(
                            "idle-detected",
                            IdleEvent {
                                idle_duration_secs: idle_secs,
                                task_id,
                                task_name: task_name.clone(),
                                tracking_active: false,
                            },
                        );
                        eprintln!("[idle] Emit result: {:?}", emit_result);
                    } else {
                        eprintln!("[idle] No task was paused during idle, no popup");
                    }

                    // Reset idle state
                    is_idle = false;
                    idle_started_at = None;
                    paused_task_id = None;
                    paused_task_name = None;
                }
            } else {
                // User is idle
                if !is_idle && (system_idle_secs >= idle_threshold_secs || is_locked) {
                    // ============================================
                    // IDLE DETECTED — stop running timer
                    // ============================================
                    eprintln!(
                        "[idle] *** IDLE THRESHOLD REACHED ({}s idle, locked={}) ***",
                        system_idle_secs, is_locked
                    );

                    idle_started_at =
                        Some(Utc::now() - chrono::Duration::seconds(system_idle_secs as i64));
                    is_idle = true;

                    // Read timer state and stop if running
                    // Do NOT hold the lock while making API calls
                    let task_info = {
                        let timer_state = app_handle.state::<TimerState>();
                        match timer_state.inner().lock() {
                            Ok(s) => {
                                if let Some(task_id) = s.running_task_id {
                                    let task_name = s
                                        .cached_tasks
                                        .iter()
                                        .find(|t| t.id == task_id)
                                        .map(|t| t.name.clone())
                                        .unwrap_or_else(|| format!("Task {}", task_id));
                                    Some((task_id, task_name))
                                } else {
                                    eprintln!("[idle] No timer was running");
                                    None
                                }
                            }
                            Err(e) => {
                                eprintln!("[idle] Failed to lock timer state: {}", e);
                                None
                            }
                        }
                    };

                    if let Some((task_id, task_name)) = task_info {
                        eprintln!("[idle] Stopping timer for task {}: {}", task_id, task_name);

                        let token = get_token(&app_handle);

                        // Now stop the timer (separate lock scope)
                        let timer_state = app_handle.state::<TimerState>();
                        if let Ok(mut s) = timer_state.inner().lock() {
                            match s.stop_current(&token) {
                                Ok(()) => eprintln!("[idle] Timer stopped successfully"),
                                Err(e) => eprintln!("[idle] Failed to stop timer: {}", e),
                            }
                        }

                        paused_task_id = Some(task_id);
                        paused_task_name = Some(task_name);
                        app_handle.emit("timer-stopped", ()).ok();
                    }
                }
            }
        }
    });
}
