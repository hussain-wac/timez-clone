use timez_core::models::IdleEvent;

use crate::state::ServiceState;

pub fn take_idle_event(state: &ServiceState) -> Result<Option<IdleEvent>, String> {
    let mut pending = state
        .pending_idle_event
        .lock()
        .map_err(|err| err.to_string())?;
    Ok(pending.take())
}
