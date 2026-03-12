use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use dbus::blocking::Connection;
use timez_core::models::{IdleEvent, Task};
use timez_core::protocol::{Request, ResponseData};

use crate::runtime;
use crate::ServiceKind;

pub fn run(parent_pid: Option<u32>) -> Result<(), String> {
    let pending_idle_events = Arc::new(Mutex::new(VecDeque::new()));
    spawn_idle_monitor(Arc::clone(&pending_idle_events));

    runtime::run_server(ServiceKind::IdleTime.socket_path(), parent_pid, move |request| {
        match request {
            Request::TakeIdleEvent => {
                let mut pending = pending_idle_events.lock().map_err(|err| err.to_string())?;
                Ok(ResponseData::IdleEvent(pending.pop_front()))
            }
            Request::Shutdown => Ok(ResponseData::Unit),
            _ => Err("Unsupported request for idle-time service".to_string()),
        }
    })
}

fn spawn_idle_monitor(pending_idle_events: Arc<Mutex<VecDeque<IdleEvent>>>) {
    std::thread::spawn(move || {
        let conn = match Connection::new_session() {
            Ok(conn) => conn,
            Err(err) => {
                eprintln!("[idle-time] D-Bus connect failed: {err}");
                return;
            }
        };

        let mut is_idle = false;
        let mut idle_started_at: Option<chrono::DateTime<Utc>> = None;
        let mut paused_task: Option<Task> = None;

        loop {
            std::thread::sleep(Duration::from_secs(2));
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
                Err(err) => {
                    eprintln!("[idle-time] GetIdletime failed: {err}");
                    continue;
                }
            };

            let system_idle_secs = idle_ms / 1000;
            let user_is_active = system_idle_secs < 3;

            if user_is_active {
                if is_idle {
                    if let (Some(task), Some(started_at)) = (paused_task.take(), idle_started_at) {
                        let idle_duration_secs = (Utc::now() - started_at).num_seconds().max(0);
                        if let Ok(mut pending) = pending_idle_events.lock() {
                            pending.push_back(IdleEvent {
                                idle_duration_secs,
                                task_id: task.id,
                                task_name: task.name,
                            });
                        }
                    }

                    is_idle = false;
                    idle_started_at = None;
                }
                continue;
            }

            if !is_idle && system_idle_secs >= 60 {
                let running_task = current_running_task();
                if let Some(task) = running_task {
                    let _ = runtime::send_request(&ServiceKind::Task.socket_path(), Request::StopTimer);
                    paused_task = Some(task);
                    idle_started_at =
                        Some(Utc::now() - chrono::Duration::seconds(system_idle_secs as i64));
                    is_idle = true;
                }
            }
        }
    });
}

fn current_running_task() -> Option<Task> {
    let response = runtime::send_request(&ServiceKind::Task.socket_path(), Request::ListTasks).ok()?;
    match response {
        ResponseData::Tasks(tasks) => tasks.into_iter().find(|task| task.running),
        _ => None,
    }
}
