use std::fs;
use std::path::Path;

use janiform_event_log::ProgressMeasure;
use janiform_event_log::RunPhaseRef;
use janiform_event_log::RunStageRef;
use janiform_event_log::RunStageStatus;
use janiform_event_log::RunStateSnapshot;
use janiform_event_log::RunStatus;
use janiform_terminal_ui::KeyValueRow;
use janiform_terminal_ui::StageChecklist;
use janiform_terminal_ui::StageChecklistItem;
use janiform_terminal_ui::StageChecklistStatus;
use janiform_terminal_ui::StartSummary;
use janiform_terminal_ui::TerminalRenderer;
use janiform_terminal_ui::WarningBlock;
use janiform_terminal_ui::format_elapsed_label;
use janiform_terminal_ui::format_warning_count;

use crate::commands::shared::artifact_documents::ArtifactListItem;
use crate::commands::shared::artifact_documents::render_artifact_list;
use crate::commands::shared::artifact_documents::{
    display_path_relative_to, elapsed_between_timestamps, format_file_size,
};

use super::RunStateSource;

pub(super) fn render_run_state(
    state: &RunStateSnapshot,
    run_directory: &Path,
    include_events: bool,
    event_log_path: &Path,
    state_source: RunStateSource,
    renderer: &TerminalRenderer,
) -> String {
    let mut sections = vec![render_run_summary(state, renderer)];

    if state.status == RunStatus::Running {
        sections.push(render_current_state(
            state,
            run_directory,
            event_log_path,
            renderer,
        ));
    }

    if !state.stage_states.is_empty() {
        sections.push(render_stage_state(state, include_events, renderer));
    }

    if !state.artifacts.is_empty() || state.status == RunStatus::Running {
        sections.push(render_artifacts(
            state,
            run_directory,
            include_events,
            event_log_path,
            renderer,
        ));
    }

    if state.checkpoints.available_count > 0 {
        sections.push(render_checkpoints(
            state,
            run_directory,
            include_events,
            renderer,
        ));
    }

    if !state.warnings.is_empty() {
        sections.push(render_warnings(state, renderer));
    }

    if include_events {
        sections.push(render_event_log(
            state,
            run_directory,
            event_log_path,
            state_source,
            renderer,
        ));
    }

    sections.join("\n\n")
}

fn render_run_summary(state: &RunStateSnapshot, renderer: &TerminalRenderer) -> String {
    let mut rows = vec![
        KeyValueRow::new("Run", state.run_id.clone()),
        KeyValueRow::new(
            "Workflow",
            ref_label_or_id(state.workflow.label.as_deref(), &state.workflow.workflow_id),
        ),
        KeyValueRow::new("Status", run_status_label(state.status)),
    ];

    if let Some(elapsed) = elapsed_between_timestamps(
        state.first_event_timestamp_utc.as_deref(),
        state.latest_event_timestamp_utc.as_deref(),
    ) {
        rows.push(KeyValueRow::new("Elapsed", format_elapsed_label(elapsed)));
    }

    if state.status != RunStatus::Running {
        rows.push(KeyValueRow::new(
            "Warnings",
            format_warning_count(state.warnings.len()),
        ));
    }

    StartSummary {
        title: "Run".to_string(),
        rows,
    }
    .render(renderer)
}

fn render_current_state(
    state: &RunStateSnapshot,
    run_directory: &Path,
    event_log_path: &Path,
    renderer: &TerminalRenderer,
) -> String {
    let mut rows = Vec::new();

    if let Some(stage) = &state.active_stage {
        rows.push(KeyValueRow::new("Stage", stage_label(stage)));
    }

    if let Some(phase) = &state.latest_phase {
        rows.push(KeyValueRow::new("Phase", phase_label(phase)));
    }

    if !state.latest_progress.is_empty() {
        rows.push(KeyValueRow::block(
            "Progress",
            progress_summary(state.latest_progress.as_slice()),
        ));
    }

    rows.push(KeyValueRow::new(
        "Events",
        display_path_relative_to(run_directory, event_log_path),
    ));

    if let Some(latest_path) = &state.checkpoints.latest_path {
        rows.push(KeyValueRow::new(
            "Checkpoint",
            display_path_relative_to(run_directory, Path::new(latest_path)),
        ));
    }

    StartSummary {
        title: "Current".to_string(),
        rows,
    }
    .render(renderer)
}

fn render_stage_state(
    state: &RunStateSnapshot,
    include_events: bool,
    renderer: &TerminalRenderer,
) -> String {
    if !include_events {
        return StageChecklist {
            title: "Stages".to_string(),
            stages: state
                .stage_states
                .iter()
                .map(|stage| StageChecklistItem {
                    name: stage_label(&stage.stage),
                    status: stage_status(stage.status),
                })
                .collect(),
        }
        .render(renderer);
    }

    let rows = state
        .stage_states
        .iter()
        .map(|stage| {
            let mut value = run_stage_status_label(stage.status).to_string();

            if let Some(elapsed) = elapsed_between_timestamps(
                stage.started_at_utc.as_deref(),
                stage.completed_at_utc.as_deref(),
            ) {
                value = format!("{value}, {}", format_elapsed_label(elapsed));
            }

            if let Some(started_at) = &stage.started_at_utc {
                value = format!("{value}, started {started_at}");
            }

            if let Some(completed_at) = &stage.completed_at_utc {
                value = format!("{value}, completed {completed_at}");
            }

            KeyValueRow::new(stage_label(&stage.stage), value)
        })
        .collect();

    StartSummary {
        title: "Stages".to_string(),
        rows,
    }
    .render(renderer)
}

fn render_artifacts(
    state: &RunStateSnapshot,
    run_directory: &Path,
    include_events: bool,
    event_log_path: &Path,
    renderer: &TerminalRenderer,
) -> String {
    let mut artifacts = state
        .artifacts
        .iter()
        .map(|artifact| {
            let mut path = display_path_relative_to(run_directory, Path::new(&artifact.path));

            if include_events && let Ok(metadata) = fs::metadata(&artifact.path) {
                path = format!("{path} ({})", format_file_size(metadata.len()));
            }

            ArtifactListItem::new(artifact.artifact_kind.as_str(), path)
        })
        .collect::<Vec<_>>();

    artifacts.push(ArtifactListItem::new(
        "events",
        display_path_relative_to(run_directory, event_log_path),
    ));

    render_artifact_list("Artifacts", artifacts, renderer)
        .unwrap_or_else(|| "Artifacts".to_string())
}

fn render_checkpoints(
    state: &RunStateSnapshot,
    run_directory: &Path,
    include_events: bool,
    renderer: &TerminalRenderer,
) -> String {
    let mut rows = vec![KeyValueRow::new(
        "Available",
        state.checkpoints.available_count.to_string(),
    )];

    if let Some(latest_path) = &state.checkpoints.latest_path {
        rows.push(KeyValueRow::new(
            "Latest",
            display_path_relative_to(run_directory, Path::new(latest_path)),
        ));

        if include_events && let Ok(metadata) = fs::metadata(latest_path) {
            rows.push(KeyValueRow::new(
                "Latest size",
                format_file_size(metadata.len()),
            ));
        }
    }

    StartSummary {
        title: "Checkpoints".to_string(),
        rows,
    }
    .render(renderer)
}

fn render_warnings(state: &RunStateSnapshot, renderer: &TerminalRenderer) -> String {
    WarningBlock {
        title: format!("Warnings: {}", state.warnings.len()),
        warnings: state
            .warnings
            .iter()
            .map(|warning| warning.reason.clone())
            .collect(),
    }
    .render(renderer)
}

fn render_event_log(
    state: &RunStateSnapshot,
    run_directory: &Path,
    event_log_path: &Path,
    state_source: RunStateSource,
    renderer: &TerminalRenderer,
) -> String {
    let mut rows = vec![
        KeyValueRow::new("Events", state.event_count.to_string()),
        KeyValueRow::new(
            "Log",
            display_path_relative_to(run_directory, event_log_path),
        ),
        KeyValueRow::new("State source", run_state_source_label(state_source)),
    ];

    if let Some(first_event_timestamp) = &state.first_event_timestamp_utc {
        rows.push(KeyValueRow::new("First", first_event_timestamp.clone()));
    }

    if let Some(latest_event_timestamp) = &state.latest_event_timestamp_utc {
        rows.push(KeyValueRow::new("Latest", latest_event_timestamp.clone()));
    }

    StartSummary {
        title: "Event log".to_string(),
        rows,
    }
    .render(renderer)
}

fn run_status_label(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Running => "running",
        RunStatus::Completed => "completed",
        RunStatus::Failed => "failed",
    }
}

fn run_stage_status_label(status: RunStageStatus) -> &'static str {
    match status {
        RunStageStatus::Pending => "pending",
        RunStageStatus::Running => "running",
        RunStageStatus::Completed => "completed",
        RunStageStatus::Failed => "failed",
    }
}

fn run_state_source_label(state_source: RunStateSource) -> &'static str {
    match state_source {
        RunStateSource::LatestStateSnapshot => "latest-state snapshot",
        RunStateSource::EventLogReplay => "event-log replay",
    }
}

fn stage_status(status: RunStageStatus) -> StageChecklistStatus {
    match status {
        RunStageStatus::Pending => StageChecklistStatus::Pending,
        RunStageStatus::Running => StageChecklistStatus::Active,
        RunStageStatus::Completed => StageChecklistStatus::Completed,
        RunStageStatus::Failed => StageChecklistStatus::Failed,
    }
}

fn stage_label(stage: &RunStageRef) -> String {
    ref_label_or_id(stage.label.as_deref(), &stage.stage_id)
}

fn phase_label(phase: &RunPhaseRef) -> String {
    ref_label_or_id(phase.label.as_deref(), &phase.phase_id)
}

fn ref_label_or_id(label: Option<&str>, identifier: &str) -> String {
    label.unwrap_or(identifier).to_string()
}

fn progress_summary(measures: &[ProgressMeasure]) -> String {
    measures
        .iter()
        .map(progress_measure_summary)
        .collect::<Vec<_>>()
        .join("\n")
}

fn progress_measure_summary(measure: &ProgressMeasure) -> String {
    match measure {
        ProgressMeasure::BoundedCount {
            label,
            completed_units,
            total_units,
            ..
        } => format!(
            "{} {completed_units}/{total_units}",
            progress_label(label.as_deref())
        ),
        ProgressMeasure::EvaluationCount {
            label,
            completed_evaluations,
            evaluation_budget,
            ..
        } => match evaluation_budget {
            Some(evaluation_budget) => format!(
                "{} {completed_evaluations}/{evaluation_budget}",
                progress_label(label.as_deref())
            ),
            None => format!(
                "{} {completed_evaluations} evaluations",
                progress_label(label.as_deref())
            ),
        },
        ProgressMeasure::Convergence { label, value, .. }
        | ProgressMeasure::ObjectiveValue { label, value, .. }
        | ProgressMeasure::Scalar { label, value, .. } => {
            format!("{} {value}", progress_label(label.as_deref()))
        }
        ProgressMeasure::CandidateId {
            label,
            candidate_id,
            ..
        } => format!("{} {candidate_id}", progress_label(label.as_deref())),
        ProgressMeasure::PricingGeometry {
            label, geometry, ..
        } => format!(
            "{} {}",
            progress_label(label.as_deref()),
            geometry_summary(geometry)
        ),
        ProgressMeasure::SimulationIntervalCount {
            label,
            completed_intervals,
            total_intervals,
            ..
        } => format!(
            "{} {completed_intervals}/{total_intervals}",
            progress_label(label.as_deref())
        ),
        ProgressMeasure::OutputChunkCount {
            label,
            completed_chunks,
            total_chunks,
            ..
        } => format!(
            "{} {completed_chunks}/{total_chunks}",
            progress_label(label.as_deref())
        ),
        ProgressMeasure::AnalyticsCheckCount {
            label,
            completed_checks,
            total_checks,
            ..
        } => format!(
            "{} {completed_checks}/{total_checks}",
            progress_label(label.as_deref())
        ),
        ProgressMeasure::Text { label, value, .. } => {
            format!("{} {value}", progress_label(label.as_deref()))
        }
    }
}

fn progress_label(label: Option<&str>) -> &str {
    label.unwrap_or("progress")
}

fn geometry_summary(geometry: &janiform_event_log::PricingProgressGeometry) -> String {
    let mut parts = Vec::new();

    if let Some(group) = &geometry.group {
        parts.push(format!(
            "group {}/{}",
            group.completed_units, group.total_units
        ));
    }

    if let Some(maturity) = &geometry.maturity {
        parts.push(format!(
            "maturity {}/{}",
            maturity.completed_units, maturity.total_units
        ));
    }

    if let Some(strike) = &geometry.strike {
        parts.push(format!(
            "strike {}/{}",
            strike.completed_units, strike.total_units
        ));
    }

    if let Some(interval) = &geometry.interval {
        parts.push(format!(
            "interval {}/{}",
            interval.completed_units, interval.total_units
        ));
    }

    parts.join(", ")
}
