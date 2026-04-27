use std::time::Duration;

use janiform_event_log::RunStateSnapshot;

use crate::commands::shared::artifact_documents::elapsed_between_timestamps;

pub(super) fn run_elapsed(state: &RunStateSnapshot) -> Option<Duration> {
    elapsed_between_timestamps(
        state.first_event_timestamp_utc.as_deref(),
        state.latest_event_timestamp_utc.as_deref(),
    )
}

pub(super) fn stage_duration(state: &RunStateSnapshot, stage_id: &str) -> Option<Duration> {
    let stage = state
        .stage_states
        .iter()
        .find(|stage| stage.stage.stage_id == stage_id)?;

    elapsed_between_timestamps(
        stage.started_at_utc.as_deref(),
        stage.completed_at_utc.as_deref(),
    )
}
