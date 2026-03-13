use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TimeStamp {
    pub timestamp: String,
    pub elapsed_secs: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LocalTimeEntry {
    pub task_id: i64,
    pub client_started_at: String,
    pub client_stopped_at: Option<String>,
    pub synced: bool,
    pub timestamps: Vec<TimeStamp>,  // Array of timestamps every 5 seconds
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LocalTimeStore {
    pub entries: Vec<LocalTimeEntry>,
    pub last_sync_at: Option<String>,
    pub was_running: bool,
    pub last_running_task_id: Option<i64>,
    pub auth_token: Option<String>,
}

pub struct LocalTimeStorage {
    inner: Mutex<LocalTimeStore>,
    path: PathBuf,
}

impl LocalTimeStorage {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let path = app_data_dir.join("time_entries.json");
        let store = if path.exists() {
            fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            LocalTimeStore::default()
        };

        Self {
            inner: Mutex::new(store),
            path,
        }
    }

    fn save(&self) {
        if let Ok(store) = self.inner.lock() {
            if let Ok(json) = serde_json::to_string_pretty(&*store) {
                if let Some(parent) = self.path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::write(&self.path, json);
            }
        }
    }

    pub fn start_timer(&self, task_id: i64, started_at: String) {
        if let Ok(mut store) = self.inner.lock() {
            if let Some(entry) = store
                .entries
                .iter_mut()
                .find(|e| e.task_id == task_id && !e.synced)
            {
                entry.client_started_at = started_at;
                entry.client_stopped_at = None;
                entry.timestamps.clear();
            } else {
                store.entries.push(LocalTimeEntry {
                    task_id,
                    client_started_at: started_at,
                    client_stopped_at: None,
                    synced: false,
                    timestamps: vec![],
                });
            }
        }
        self.save();
    }

    pub fn add_timestamp(&self, task_id: i64, elapsed_secs: i64) {
        if let Ok(mut store) = self.inner.lock() {
            if let Some(entry) = store
                .entries
                .iter_mut()
                .find(|e| e.task_id == task_id && !e.synced)
            {
                entry.timestamps.push(TimeStamp {
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    elapsed_secs,
                });
                // Keep only last 100 timestamps to avoid growing too large
                if entry.timestamps.len() > 100 {
                    entry.timestamps.remove(0);
                }
            }
        }
        self.save();
    }

    pub fn stop_timer(&self, task_id: i64, stopped_at: String) {
        if let Ok(mut store) = self.inner.lock() {
            if let Some(entry) = store
                .entries
                .iter_mut()
                .find(|e| e.task_id == task_id && !e.synced)
            {
                entry.client_stopped_at = Some(stopped_at);
            }
        }
        self.save();
    }

    pub fn mark_synced(&self, task_id: i64) {
        if let Ok(mut store) = self.inner.lock() {
            store.entries.retain(|e| e.task_id != task_id || e.synced);
            if let Some(entry) = store
                .entries
                .iter_mut()
                .find(|e| e.task_id == task_id && !e.synced)
            {
                entry.synced = true;
            }
            store.was_running = false;
            store.last_running_task_id = None;
        }
        self.save();
    }

    pub fn set_running(&self, task_id: i64, started_at: String) {
        if let Ok(mut store) = self.inner.lock() {
            store.was_running = true;
            store.last_running_task_id = Some(task_id);
            // Also add/update the entry
            if let Some(entry) = store
                .entries
                .iter_mut()
                .find(|e| e.task_id == task_id && !e.synced)
            {
                entry.client_started_at = started_at;
                entry.client_stopped_at = None;
            } else {
                store.entries.push(LocalTimeEntry {
                    task_id,
                    client_started_at: started_at,
                    client_stopped_at: None,
                    synced: false,
                });
            }
        }
        self.save();
    }

    pub fn set_stopped(&self, task_id: i64) {
        if let Ok(mut store) = self.inner.lock() {
            store.was_running = false;
            if let Some(entry) = store
                .entries
                .iter_mut()
                .find(|e| e.task_id == task_id && !e.synced)
            {
                entry.client_stopped_at = Some(chrono::Utc::now().to_rfc3339());
            }
        }
        self.save();
    }

    pub fn was_running(&self) -> bool {
        if let Ok(store) = self.inner.lock() {
            store.was_running
        } else {
            false
        }
    }

    pub fn get_last_running_task_id(&self) -> Option<i64> {
        if let Ok(store) = self.inner.lock() {
            store.last_running_task_id
        } else {
            None
        }
    }

    pub fn clear_running_state(&self) {
        if let Ok(mut store) = self.inner.lock() {
            store.was_running = false;
            store.last_running_task_id = None;
        }
        self.save();
    }

    pub fn set_auth_token(&self, token: String) {
        if let Ok(mut store) = self.inner.lock() {
            store.auth_token = Some(token);
        }
        self.save();
    }

    pub fn get_auth_token(&self) -> Option<String> {
        if let Ok(store) = self.inner.lock() {
            store.auth_token.clone()
        } else {
            None
        }
    }

    pub fn clear_auth_token(&self) {
        if let Ok(mut store) = self.inner.lock() {
            store.auth_token = None;
        }
        self.save();
    }
}
        self.save();
    }

    pub fn get_unsynced_entries(&self) -> Vec<LocalTimeEntry> {
        if let Ok(store) = self.inner.lock() {
            store
                .entries
                .iter()
                .filter(|e| !e.synced)
                .cloned()
                .collect()
        } else {
            vec![]
        }
    }

    pub fn get_last_running_entry(&self) -> Option<LocalTimeEntry> {
        if let Ok(store) = self.inner.lock() {
            store
                .entries
                .iter()
                .rev()
                .find(|e| e.client_stopped_at.is_none())
                .cloned()
        } else {
            None
        }
    }

    pub fn get_entry(&self, task_id: i64) -> Option<LocalTimeEntry> {
        if let Ok(store) = self.inner.lock() {
            store
                .entries
                .iter()
                .find(|e| e.task_id == task_id && !e.synced)
                .cloned()
        } else {
            None
        }
    }
}
    }

    pub fn update_last_sync(&self) {
        if let Ok(mut store) = self.inner.lock() {
            store.last_sync_at = Some(chrono::Utc::now().to_rfc3339());
        }
        self.save();
    }

    pub fn get_last_sync_at(&self) -> Option<String> {
        if let Ok(store) = self.inner.lock() {
            store.last_sync_at.clone()
        } else {
            None
        }
    }

    pub fn clear(&self) {
        if let Ok(mut store) = self.inner.lock() {
            store.entries.clear();
            store.last_sync_at = None;
        }
        self.save();
    }
}
