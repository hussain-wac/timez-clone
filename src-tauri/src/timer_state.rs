use std::sync::Mutex;
use std::time::Duration;

use chrono::Utc;
use tauri::{AppHandle, Manager};

use crate::api;
use crate::api::AuthToken;
use crate::models::Task;

/// Local timer state that tracks everything without hitting the external API.
/// The external API is only called every SYNC_INTERVAL for persistence.
pub struct TimerStateInner {
    /// Cached task list from the last API sync
    pub cached_tasks: Vec<Task>,
    /// Currently running task id
    pub running_task_id: Option<i64>,
    /// When the current timer was started (local clock)
    pub timer_started_at: Option<chrono::DateTime<Utc>>,
    /// Last time we synced with the external API
    pub last_sync_at: chrono::DateTime<Utc>,
    /// Elapsed seconds accumulated before current run (from API summary)
    pub base_elapsed: std::collections::HashMap<i64, i64>,
}

pub type TimerState = Mutex<TimerStateInner>;

const SYNC_INTERVAL_SECS: u64 = 10 * 60; // 10 minutes

impl TimerStateInner {
    pub fn new() -> Self {
        Self {
            cached_tasks: vec![],
            running_task_id: None,
            timer_started_at: None,
            last_sync_at: chrono::DateTime::<Utc>::MIN_UTC,
            base_elapsed: std::collections::HashMap::new(),
        }
    }

    /// Get tasks with locally computed elapsed time
    pub fn get_tasks(&self) -> Vec<Task> {
        let now = Utc::now();
        self.cached_tasks
            .iter()
            .map(|t| {
                let base = self.base_elapsed.get(&t.id).copied().unwrap_or(t.elapsed_secs);
                let live_elapsed = if self.running_task_id == Some(t.id) {
                    self.timer_started_at
                        .map(|started| (now - started).num_seconds().max(0))
                        .unwrap_or(0)
                } else {
                    0
                };
                Task {
                    id: t.id,
                    name: t.name.clone(),
                    budget_secs: t.budget_secs,
                    elapsed_secs: base + live_elapsed,
                    running: self.running_task_id == Some(t.id),
                }
            })
            .collect()
    }

    /// Sync cached data from the external API
    pub fn sync_from_api(&mut self, token: &Option<String>) {
        if let Ok(tasks) = api::list_tasks(token) {
            // Store base elapsed from API for each task
            for t in &tasks {
                self.base_elapsed.insert(t.id, t.elapsed_secs);
            }

            // If a task is running according to API, adopt that state
            if let Some(running) = tasks.iter().find(|t| t.running) {
                if self.running_task_id.is_none() {
                    self.running_task_id = Some(running.id);
                    self.timer_started_at = Some(Utc::now());
                }
                // Subtract the live elapsed that API already includes
                // so we don't double-count
                if self.running_task_id == Some(running.id) {
                    if let Ok(status) = api::get_status(token) {
                        let api_live = status.elapsed_seconds.unwrap_or(0);
                        if let Some(base) = self.base_elapsed.get_mut(&running.id) {
                            *base = running.elapsed_secs - api_live;
                        }
                    }
                }
            }

            self.cached_tasks = tasks
                .into_iter()
                .map(|t| Task {
                    id: t.id,
                    name: t.name,
                    budget_secs: t.budget_secs,
                    elapsed_secs: 0, // We use base_elapsed map instead
                    running: false,   // We track running state locally
                })
                .collect();
            self.last_sync_at = Utc::now();
        }
    }

    /// Start a timer for a task (calls API + updates local state)
    pub fn start_task(&mut self, task_id: i64, token: &Option<String>) -> Result<(), String> {
        // Stop current task first if one is running
        if let Some(current_id) = self.running_task_id {
            if current_id != task_id {
                self.stop_current(token)?;
            } else {
                return Ok(()); // Already running this task
            }
        }

        api::start_timer(task_id, token)?;
        self.running_task_id = Some(task_id);
        self.timer_started_at = Some(Utc::now());
        Ok(())
    }

    /// Resume a task after idle, adding the idle duration as work time
    pub fn resume_with_idle_time(&mut self, task_id: i64, idle_secs: i64, token: &Option<String>) -> Result<(), String> {
        // Add the idle duration to this task's base elapsed
        let base = self.base_elapsed.entry(task_id).or_insert(0);
        *base += idle_secs;

        // Start the timer so it keeps running
        api::start_timer(task_id, token)?;
        self.running_task_id = Some(task_id);
        self.timer_started_at = Some(Utc::now());
        Ok(())
    }

    /// Stop the currently running timer (calls API + updates local state)
    pub fn stop_current(&mut self, token: &Option<String>) -> Result<(), String> {
        if let Some(task_id) = self.running_task_id {
            // Accumulate elapsed time into base
            if let Some(started) = self.timer_started_at {
                let elapsed = (Utc::now() - started).num_seconds().max(0);
                let base = self.base_elapsed.entry(task_id).or_insert(0);
                *base += elapsed;
            }
            api::stop_timer(task_id, token)?;
            self.running_task_id = None;
            self.timer_started_at = None;
        }
        Ok(())
    }
}

/// Spawns a background thread that syncs with the external API every 10 minutes
pub fn spawn_sync_thread(app_handle: AppHandle) {
    std::thread::spawn(move || {
        // Initial sync
        {
            let token = get_token(&app_handle);
            let state = app_handle.state::<TimerState>();
            if let Ok(mut s) = state.inner().lock() {
                s.sync_from_api(&token);
                println!("[sync] Initial sync complete, {} tasks loaded", s.cached_tasks.len());
            }
        }

        loop {
            std::thread::sleep(Duration::from_secs(SYNC_INTERVAL_SECS));

            let token = get_token(&app_handle);
            let state = app_handle.state::<TimerState>();
            if let Ok(mut s) = state.inner().lock() {
                println!("[sync] Syncing with API...");
                s.sync_from_api(&token);
                println!("[sync] Sync complete");
            }
        }
    });
}

/// Helper to read the current auth token
fn get_token(app_handle: &AppHandle) -> Option<String> {
    let auth = app_handle.state::<AuthToken>();
    auth.inner().lock().ok().and_then(|s| s.access_token.clone())
}
