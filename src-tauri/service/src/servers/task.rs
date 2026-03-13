use std::sync::{Arc, Mutex};

use chrono::Timelike;
use timez_core::models::{Task, TimerStatus};
use timez_core::protocol::{Request, ResponseData};
use timez_core::timer_state::TimerStateInner;

use crate::auth_store;
use crate::runtime;
use crate::ServiceKind;

pub fn run(parent_pid: Option<u32>) -> Result<(), String> {
    let timer_state = Arc::new(Mutex::new(TimerStateInner::new()));
    spawn_sync_thread(Arc::clone(&timer_state));

    runtime::run_server(
        ServiceKind::Task.socket_path(),
        parent_pid,
        move |request| handle_request(request, &timer_state),
    )
}

fn handle_request(
    request: Request,
    timer_state: &Arc<Mutex<TimerStateInner>>,
) -> Result<ResponseData, String> {
    match request {
        Request::ListTasks => Ok(ResponseData::Tasks(list_tasks(timer_state)?)),
        Request::StartTimer { task_id } => {
            Ok(ResponseData::Tasks(start_timer(timer_state, task_id)?))
        }
        Request::StopTimer => Ok(ResponseData::Tasks(stop_timer(timer_state)?)),
        Request::GetStatus => Ok(ResponseData::Status(get_status(timer_state)?)),
        Request::AddIdleTime {
            task_id,
            duration_secs,
        } => Ok(ResponseData::Tasks(add_idle_time(
            timer_state,
            task_id,
            duration_secs,
        )?)),
        Request::DiscardIdleTime { task_id } => Ok(ResponseData::Tasks(discard_idle_time(
            timer_state,
            task_id,
        )?)),
        Request::RefreshTasks => Ok(ResponseData::Tasks(refresh_tasks(timer_state)?)),
        Request::Shutdown => Ok(ResponseData::Unit),
        _ => Err("Unsupported request for task service".to_string()),
    }
}

fn list_tasks(timer_state: &Arc<Mutex<TimerStateInner>>) -> Result<Vec<Task>, String> {
    let timer = timer_state.lock().map_err(|err| err.to_string())?;
    Ok(timer.get_tasks())
}

fn start_timer(
    timer_state: &Arc<Mutex<TimerStateInner>>,
    task_id: i64,
) -> Result<Vec<Task>, String> {
    let token = auth_store::read_token();
    let mut timer = timer_state.lock().map_err(|err| err.to_string())?;
    timer.start_task(task_id, &token)?;
    Ok(timer.get_tasks())
}

fn stop_timer(timer_state: &Arc<Mutex<TimerStateInner>>) -> Result<Vec<Task>, String> {
    let token = auth_store::read_token();
    let mut timer = timer_state.lock().map_err(|err| err.to_string())?;
    timer.stop_current(&token)?;
    Ok(timer.get_tasks())
}

fn get_status(timer_state: &Arc<Mutex<TimerStateInner>>) -> Result<TimerStatus, String> {
    let timer = timer_state.lock().map_err(|err| err.to_string())?;
    Ok(TimerStatus {
        running: timer.running_task_id.is_some(),
        active_task_id: timer.running_task_id,
        current_entry_elapsed: timer
            .timer_started_at
            .map(|started| (chrono::Utc::now() - started).num_seconds().max(0))
            .unwrap_or(0),
    })
}

fn add_idle_time(
    timer_state: &Arc<Mutex<TimerStateInner>>,
    task_id: i64,
    duration_secs: i64,
) -> Result<Vec<Task>, String> {
    let token = auth_store::read_token();
    let mut timer = timer_state.lock().map_err(|err| err.to_string())?;
    timer.resume_with_idle_time(task_id, duration_secs, &token)?;
    Ok(timer.get_tasks())
}

fn discard_idle_time(
    timer_state: &Arc<Mutex<TimerStateInner>>,
    task_id: i64,
) -> Result<Vec<Task>, String> {
    let token = auth_store::read_token();
    let mut timer = timer_state.lock().map_err(|err| err.to_string())?;
    timer.start_task(task_id, &token)?;
    Ok(timer.get_tasks())
}

fn refresh_tasks(timer_state: &Arc<Mutex<TimerStateInner>>) -> Result<Vec<Task>, String> {
    let token = auth_store::read_token();
    let mut timer = timer_state.lock().map_err(|err| err.to_string())?;
    timer.sync_from_api(&token);
    Ok(timer.get_tasks())
}

fn spawn_sync_thread(timer_state: Arc<Mutex<TimerStateInner>>) {
    use timez_core::api;

    std::thread::spawn(move || {
        // Initial sync
        {
            let token = auth_store::read_token();
            if let Ok(mut timer) = timer_state.lock() {
                timer.sync_from_api(&token);
                println!("[sync] Initial sync complete");
            }
        }

        loop {
            std::thread::sleep(std::time::Duration::from_secs(60)); // 1 minute

            let now = chrono::Utc::now();

            // Check for midnight reset
            if now.hour() == 0 && now.minute() == 0 {
                println!("[sync] Midnight reset - stopping timer");
                if let Ok(mut timer) = timer_state.lock() {
                    if timer.running_task_id.is_some() {
                        // Sync final time before reset
                        if let (Some(task_id), Some(started_at)) =
                            (timer.running_task_id, timer.timer_started_at)
                        {
                            let elapsed = (now - started_at).num_seconds().max(0);
                            if elapsed > 0 {
                                let client_started = started_at.to_rfc3339();
                                let _ = api::sync_time(
                                    task_id,
                                    elapsed,
                                    &client_started,
                                    Some(&now.to_rfc3339()),
                                    &auth_store::read_token(),
                                );
                            }
                        }
                        let _ = timer.stop_current(&auth_store::read_token());
                    }
                }
            }

            let token = auth_store::read_token();
            println!("[sync] Syncing with API...");

            if let Ok(mut timer) = timer_state.lock() {
                // Get running task info
                if let (Some(task_id), Some(started_at)) =
                    (timer.running_task_id, timer.timer_started_at)
                {
                    let elapsed = (chrono::Utc::now() - started_at).num_seconds().max(0);
                    let client_started = started_at.to_rfc3339();

                    if elapsed > 0 {
                        match api::sync_time(task_id, elapsed, &client_started, None, &token) {
                            Ok(_) => {
                                println!("[sync] Synced {} seconds for task {}", elapsed, task_id)
                            }
                            Err(e) => println!("[sync] Error: {}", e),
                        }
                    }
                }

                timer.sync_from_api(&token);
                println!("[sync] Sync complete");
            }
        }
    });
}
