use std::sync::{Arc, Mutex};

use timez_core::api::AuthTokenState;
use timez_core::idle::{ActivityState, ActivityTracker};
use timez_core::models::IdleEvent;
use timez_core::timer_state::TimerStateInner;

pub struct ServiceState {
    pub auth_state: Arc<Mutex<AuthTokenState>>,
    pub timer_state: Arc<Mutex<TimerStateInner>>,
    pub activity_state: Arc<ActivityState>,
    pub pending_idle_event: Arc<Mutex<Option<IdleEvent>>>,
}

impl ServiceState {
    pub fn new() -> Self {
        Self {
            auth_state: Arc::new(Mutex::new(AuthTokenState::new())),
            timer_state: Arc::new(Mutex::new(TimerStateInner::new())),
            activity_state: Arc::new(Mutex::new(ActivityTracker::new())),
            pending_idle_event: Arc::new(Mutex::new(None)),
        }
    }

    pub fn current_token(&self) -> Result<Option<String>, String> {
        let auth = self.auth_state.lock().map_err(|err| err.to_string())?;
        Ok(auth.access_token.clone())
    }

    pub fn store_access_token(&self, token: &str) -> Result<(), String> {
        let mut auth = self.auth_state.lock().map_err(|err| err.to_string())?;
        auth.access_token = Some(token.to_string());
        Ok(())
    }

    pub fn clear_access_token(&self) -> Result<(), String> {
        let mut auth = self.auth_state.lock().map_err(|err| err.to_string())?;
        auth.access_token = None;
        Ok(())
    }
}
