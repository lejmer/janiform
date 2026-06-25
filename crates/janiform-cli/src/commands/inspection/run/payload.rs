use janiform_event_log::RunEventPaths;
use janiform_event_log::RunStateSnapshot;
use janiform_foundations::paths::format_path;
use serde::Serialize;

use super::state::{InspectedRunState, RunStateSource};

#[derive(Clone, Debug, Serialize)]
pub(super) struct RunInspectionPayload {
    #[serde(flatten)]
    pub(super) state: RunStateSnapshot,
    pub(super) inspection: RunInspectionMetadata,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct RunInspectionMetadata {
    pub(super) run_directory: String,
    pub(super) event_log_path: String,
    pub(super) latest_state_snapshot_path: String,
    pub(super) state_source: RunStateSource,
    pub(super) replay_fallback_attempted: bool,
}

pub(super) fn run_inspection_payload(
    run_directory: String,
    event_paths: &RunEventPaths,
    inspected_state: &InspectedRunState,
) -> RunInspectionPayload {
    RunInspectionPayload {
        state: inspected_state.state.clone(),
        inspection: RunInspectionMetadata {
            run_directory,
            event_log_path: format_path(&event_paths.event_log_path),
            latest_state_snapshot_path: format_path(&event_paths.latest_state_snapshot_path),
            state_source: inspected_state.state_source,
            replay_fallback_attempted: inspected_state.replay_fallback_attempted,
        },
    }
}
