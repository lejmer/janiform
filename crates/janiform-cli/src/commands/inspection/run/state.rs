use std::fs;
use std::path::Path;

use janiform_event_log::RUN_STATE_SNAPSHOT_SCHEMA_VERSION;
use janiform_event_log::RunStateSnapshot;
use janiform_event_log::default_run_event_paths;
use janiform_event_log::load_run_state_snapshot;
use janiform_event_log::replay_event_log_allowing_trailing_partial;
use serde::Serialize;

use crate::error::CliError;

#[derive(Clone, Debug)]
pub(super) struct InspectedRunState {
    pub(super) state: RunStateSnapshot,
    pub(super) state_source: RunStateSource,
    pub(super) replay_fallback_attempted: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RunStateSource {
    LatestStateSnapshot,
    EventLogReplay,
}

pub(super) fn load_inspected_run_state(
    run_directory: &Path,
) -> Result<InspectedRunState, CliError> {
    let event_paths = default_run_event_paths(run_directory);
    let snapshot_path = event_paths.latest_state_snapshot_path;
    let event_log_path = event_paths.event_log_path;

    if !snapshot_path.exists() {
        return replay_state_with_fallback_reason(
            run_directory,
            &snapshot_path,
            &event_log_path,
            "latest-state snapshot is missing",
        );
    }

    if snapshot_is_stale(&snapshot_path, &event_log_path)? {
        return replay_state_with_fallback_reason(
            run_directory,
            &snapshot_path,
            &event_log_path,
            "latest-state snapshot is stale",
        );
    }

    let state = load_run_state_snapshot(&snapshot_path).map_err(|source| {
        run_inspection_error(
            run_directory,
            &snapshot_path,
            &event_log_path,
            false,
            format!("latest-state snapshot is malformed or unreadable: {source}"),
        )
    })?;

    validate_snapshot_schema(run_directory, &snapshot_path, &event_log_path, &state)?;

    Ok(InspectedRunState {
        state,
        state_source: RunStateSource::LatestStateSnapshot,
        replay_fallback_attempted: false,
    })
}

fn snapshot_is_stale(snapshot_path: &Path, event_log_path: &Path) -> Result<bool, CliError> {
    if !event_log_path.exists() {
        return Ok(false);
    }

    let snapshot_modified = fs::metadata(snapshot_path).and_then(|metadata| metadata.modified());
    let event_log_modified = fs::metadata(event_log_path).and_then(|metadata| metadata.modified());

    match (snapshot_modified, event_log_modified) {
        (Ok(snapshot_modified), Ok(event_log_modified)) => {
            Ok(event_log_modified > snapshot_modified)
        }
        (Err(source), _) => Err(CliError::ArtifactRead {
            artifact_name: "latest-state snapshot",
            path: snapshot_path.to_path_buf(),
            source,
        }),
        (_, Err(source)) => Err(CliError::ArtifactRead {
            artifact_name: "event log",
            path: event_log_path.to_path_buf(),
            source,
        }),
    }
}

fn replay_state_with_fallback_reason(
    run_directory: &Path,
    snapshot_path: &Path,
    event_log_path: &Path,
    fallback_reason: &str,
) -> Result<InspectedRunState, CliError> {
    let state = replay_event_log_allowing_trailing_partial(event_log_path).map_err(|source| {
        run_inspection_error(
            run_directory,
            snapshot_path,
            event_log_path,
            true,
            format!("{fallback_reason}, event-log replay failed: {source}"),
        )
    })?;

    Ok(InspectedRunState {
        state,
        state_source: RunStateSource::EventLogReplay,
        replay_fallback_attempted: true,
    })
}

fn validate_snapshot_schema(
    run_directory: &Path,
    snapshot_path: &Path,
    event_log_path: &Path,
    state: &RunStateSnapshot,
) -> Result<(), CliError> {
    if state.schema_version == RUN_STATE_SNAPSHOT_SCHEMA_VERSION {
        return Ok(());
    }

    Err(run_inspection_error(
        run_directory,
        snapshot_path,
        event_log_path,
        false,
        format!(
            "latest-state snapshot has schema `{}` but expected `{RUN_STATE_SNAPSHOT_SCHEMA_VERSION}`",
            state.schema_version
        ),
    ))
}

fn run_inspection_error(
    run_directory: &Path,
    snapshot_path: &Path,
    event_log_path: &Path,
    replay_fallback_attempted: bool,
    message: String,
) -> CliError {
    CliError::RunInspectionFailed {
        run_directory: run_directory.to_path_buf(),
        snapshot_path: snapshot_path.to_path_buf(),
        event_log_path: event_log_path.to_path_buf(),
        replay_fallback_attempted,
        message,
    }
}
