use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use chrono::Utc;
use dbus::blocking::Connection;

use crate::api::AuthTokenState;
use crate::models::{ActivityStats, IdleEvent};
use crate::timer_state::TimerStateInner;

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

fn get_token(auth_state: &Arc<StdMutex<AuthTokenState>>) -> Option<String> {
    auth_state.lock().ok().and_then(|s| s.access_token.clone())
}

pub fn spawn_idle_monitor(
    activity_state: Arc<ActivityState>,
    timer_state: Arc<StdMutex<TimerStateInner>>,
    auth_state: Arc<StdMutex<AuthTokenState>>,
    pending_idle_event: Arc<StdMutex<Option<IdleEvent>>>,
    idle_threshold_secs: u64,
) {
    std::thread::spawn(move || {
        eprintln!("[idle] Idle monitor thread started (threshold={}s)", idle_threshold_secs);

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

        loop {
            std::thread::sleep(Duration::from_secs(POLL_INTERVAL_SECS));

            // Get system-wide idle time via D-Bus (Mutter IdleMonitor)
            let proxy = conn.with_proxy(
                "org.gnome.Mutter.IdleMonitor",
                "/org/gnome/Mutter/IdleMonitor/Core",
                Duration::from_millis(2000),
            );
            let idle_ms: u64 = match proxy.method_call(
                "org.gnome.Mutter.IdleMonitor",
                "GetIdletime",
                (),
            ) {
                Ok((ms,)) => ms,
                Err(e) => {
                    eprintln!("[idle] D-Bus GetIdletime failed: {}", e);
                    continue;
                }
            };

            let system_idle_secs = idle_ms / 1000;
            let user_is_active = system_idle_secs < POLL_INTERVAL_SECS + 1;

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
                if let Ok(mut t) = activity_state.lock() {
                    if is_idle {
                        t.idle_secs += POLL_INTERVAL_SECS as i64;
                    } else {
                        t.active_secs += POLL_INTERVAL_SECS as i64;
                        if user_is_active {
                            t.last_activity_at = Utc::now();
                        }
                    }
                }
            }

            if user_is_active {
                if is_idle {
                    let idle_secs = idle_started_at
                        .map(|start| (Utc::now() - start).num_seconds())
                        .unwrap_or(0);

                    if let (Some(task_id), Some(ref task_name)) =
                        (paused_task_id, &paused_task_name)
                    {
                        let event = IdleEvent {
                            idle_duration_secs: idle_secs,
                            task_id,
                            task_name: task_name.clone(),
                            tracking_active: false,
                        };
                        if let Ok(mut pending) = pending_idle_event.lock() {
                            *pending = Some(event);
                        }
                    }

                    // Reset idle state
                    is_idle = false;
                    idle_started_at = None;
                    paused_task_id = None;
                    paused_task_name = None;
                }
            } else if !is_idle && system_idle_secs >= idle_threshold_secs {
                // IDLE DETECTED — stop running timer
                idle_started_at = Some(Utc::now() - chrono::Duration::seconds(system_idle_secs as i64));
                is_idle = true;

                let task_info = {
                    match timer_state.lock() {
                        Ok(s) => {
                            if let Some(task_id) = s.running_task_id {
                                let task_name = s.cached_tasks
                                    .iter()
                                    .find(|t| t.id == task_id)
                                    .map(|t| t.name.clone())
                                    .unwrap_or_else(|| format!("Task {}", task_id));
                                Some((task_id, task_name))
                            } else {
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
                    let token = get_token(&auth_state);
                    if let Ok(mut s) = timer_state.lock() {
                        if let Err(e) = s.stop_current(&token) {
                            eprintln!("[idle] Failed to stop timer: {}", e);
                        }
                    }

                    paused_task_id = Some(task_id);
                    paused_task_name = Some(task_name);
                }
            }
        }
    });
}
