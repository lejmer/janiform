use std::fs;
use std::path::Path;

use janiform_event_log::RUN_STATE_SNAPSHOT_SCHEMA_VERSION;
use janiform_event_log::RunStateSnapshot;
use janiform_event_log::default_run_event_paths;
use janiform_event_log::load_run_state_snapshot;
use janiform_event_log::replay_event_log_allowing_trailing_partial;
use serde::Serialize;

use crate::commands::inspection::InspectRunCommand;
use crate::commands::shared::output::write_json_payload;
use crate::context::CommandContext;
use crate::context::OutputMode;
use crate::error::CliError;

mod rendering;

use rendering::render_run_state;

pub(crate) fn run_inspect_run(
    context: &CommandContext,
    command: InspectRunCommand,
) -> Result<(), CliError> {
    let run_directory = context.paths().resolve_process_path(&command.run_directory);
    let event_paths = default_run_event_paths(&run_directory);
    let inspected_state = load_inspected_run_state(&run_directory)?;

    let payload = RunInspectionPayload {
        state: inspected_state.state.clone(),
        inspection: RunInspectionMetadata {
            run_directory: run_directory.display().to_string(),
            event_log_path: event_paths.event_log_path.display().to_string(),
            latest_state_snapshot_path: event_paths
                .latest_state_snapshot_path
                .display()
                .to_string(),
            state_source: inspected_state.state_source,
            replay_fallback_attempted: inspected_state.replay_fallback_attempted,
        },
    };

    match context.output_mode() {
        OutputMode::Json => write_json_payload(&payload),
        OutputMode::Silent => Ok(()),
        OutputMode::Human | OutputMode::Quiet => {
            let rendered = render_run_state(
                &payload.state,
                &run_directory,
                command.include_events || context.verbosity() > 0,
                &event_paths.event_log_path,
                payload.inspection.state_source,
                &context.stdout_renderer(),
            );
            context
                .stdout_renderer()
                .write_stdout(&rendered)
                .map_err(|source| CliError::OutputWrite { source })
        }
    }
}

#[derive(Clone, Debug)]
struct InspectedRunState {
    state: RunStateSnapshot,
    state_source: RunStateSource,
    replay_fallback_attempted: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RunStateSource {
    LatestStateSnapshot,
    EventLogReplay,
}

#[derive(Clone, Debug, Serialize)]
struct RunInspectionPayload {
    #[serde(flatten)]
    state: RunStateSnapshot,
    inspection: RunInspectionMetadata,
}

#[derive(Clone, Debug, Serialize)]
struct RunInspectionMetadata {
    run_directory: String,
    event_log_path: String,
    latest_state_snapshot_path: String,
    state_source: RunStateSource,
    replay_fallback_attempted: bool,
}

fn load_inspected_run_state(run_directory: &Path) -> Result<InspectedRunState, CliError> {
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
