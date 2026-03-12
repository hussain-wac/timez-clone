use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: i64,
    pub name: String,
    pub budget_secs: i64,
    pub elapsed_secs: i64,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimerStatus {
    pub running: bool,
    pub active_task_id: Option<i64>,
    pub current_entry_elapsed: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IdleEvent {
    pub idle_duration_secs: i64,
    pub task_id: i64,
    pub task_name: String,
    pub tracking_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityStats {
    pub active_secs: i64,
    pub idle_secs: i64,
    pub total_secs: i64,
    pub activity_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthUser {
    pub id: i64,
    pub email: String,
    pub name: Option<String>,
    pub picture: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    pub access_token: String,
    pub user: AuthUser,
}
