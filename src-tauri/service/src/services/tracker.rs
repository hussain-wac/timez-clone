use timez_core::models::ActivityStats;

use crate::state::ServiceState;

pub fn get_activity_stats(state: &ServiceState) -> Result<ActivityStats, String> {
    let activity = state.activity_state.lock().map_err(|err| err.to_string())?;
    Ok(activity.stats())
}
