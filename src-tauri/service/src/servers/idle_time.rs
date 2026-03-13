use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use timez_core::models::{IdleEvent, Task};
use timez_core::protocol::{Request, ResponseData};

use crate::idle_detection;
use crate::runtime;
use crate::ServiceKind;

pub fn run(parent_pid: Option<u32>) -> Result<(), String> {
    let pending_idle_event = Arc::new(Mutex::new(None));
    spawn_idle_monitor(Arc::clone(&pending_idle_event));

    runtime::run_server(
        ServiceKind::IdleTime.socket_path(),
        parent_pid,
        move |request| match request {
            Request::GetIdleEvent => {
                let pending = pending_idle_event.lock().map_err(|err| err.to_string())?;
                Ok(ResponseData::IdleEvent(pending.clone()))
            }
            Request::ResolveIdleEvent => {
                let mut pending = pending_idle_event.lock().map_err(|err| err.to_string())?;
                *pending = None;
                Ok(ResponseData::Unit)
            }
            Request::Shutdown => Ok(ResponseData::Unit),
            _ => Err("Unsupported request for idle-time service".to_string()),
        },
    )
}

fn spawn_idle_monitor(pending_idle_event: Arc<Mutex<Option<IdleEvent>>>) {
    std::thread::spawn(move || {
        let conn = match idle_detection::connect_session_bus() {
            Ok(conn) => conn,
            Err(err) => {
                eprintln!("[idle-time] D-Bus connect failed: {err}");
                return;
            }
        };

        let mut is_idle = false;
        let mut idle_started_at: Option<chrono::DateTime<Utc>> = None;
        let mut paused_task: Option<Task> = None;
        let mut was_locked = false;

        loop {
            std::thread::sleep(Duration::from_secs(2));

            let is_locked = idle_detection::is_session_locked(&conn);
            let system_idle_secs = match idle_detection::get_idle_duration_secs(&conn) {
                Ok(secs) => secs,
                Err(err) => {
                    eprintln!("[idle-time] Idle query failed (system may be sleeping): {err}");
                    // Treat failure as idle (system might be sleeping)
                    300
                }
            };

            let user_is_active = system_idle_secs < 3 && !is_locked;

            if is_locked && !was_locked {
                was_locked = true;
                eprintln!("[idle-time] Session locked - treating as idle");
            } else if !is_locked && was_locked {
                was_locked = false;
                eprintln!("[idle-time] Session unlocked");
            }

            if user_is_active {
                if is_idle {
                    if let (Some(task), Some(started_at)) = (paused_task.take(), idle_started_at) {
                        let idle_duration_secs = (Utc::now() - started_at).num_seconds().max(0);
                        if let Ok(mut pending) = pending_idle_event.lock() {
                            *pending = Some(IdleEvent {
                                idle_duration_secs,
                                task_id: task.id,
                                task_name: task.name,
                                tracking_active: false,
                            });
                        }
                    }

                    is_idle = false;
                    idle_started_at = None;
                }
                continue;
            }

            if !is_idle && (system_idle_secs >= 60 || is_locked) {
                let running_task = current_running_task();
                if let Some(task) = running_task {
                    let _ =
                        runtime::send_request(&ServiceKind::Task.socket_path(), Request::StopTimer);
                    paused_task = Some(task);
                    idle_started_at =
                        Some(Utc::now() - chrono::Duration::seconds(system_idle_secs as i64));
                    is_idle = true;
                    if let (Some(task), Some(started_at)) = (&paused_task, idle_started_at) {
                        if let Ok(mut pending) = pending_idle_event.lock() {
                            *pending = Some(IdleEvent {
                                idle_duration_secs: (Utc::now() - started_at).num_seconds().max(0),
                                task_id: task.id,
                                task_name: task.name.clone(),
                                tracking_active: true,
                            });
                        }
                    }
                }
            }

            if is_idle {
                if let (Some(task), Some(started_at)) = (&paused_task, idle_started_at) {
                    if let Ok(mut pending) = pending_idle_event.lock() {
                        *pending = Some(IdleEvent {
                            idle_duration_secs: (Utc::now() - started_at).num_seconds().max(0),
                            task_id: task.id,
                            task_name: task.name.clone(),
                            tracking_active: true,
                        });
                    }
                }
            }
        }
    });
}

fn current_running_task() -> Option<Task> {
    let response =
        runtime::send_request(&ServiceKind::Task.socket_path(), Request::ListTasks).ok()?;
    match response {
        ResponseData::Tasks(tasks) => tasks.into_iter().find(|task| task.running),
        _ => None,
    }
}
