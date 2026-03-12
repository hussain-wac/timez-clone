use tauri::State;

use crate::idle::ActivityState;
use crate::models::ActivityStats;

pub fn get_activity_stats(activity: State<'_, ActivityState>) -> Result<ActivityStats, String> {
    let tracker = activity.lock().map_err(|e| e.to_string())?;
    Ok(tracker.stats())
}
