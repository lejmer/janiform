use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;
use janiform_event_log::RunPhaseRef;
use janiform_event_log::RunStageRef;
use janiform_event_log::RunStageStatus;
use janiform_event_log::RunStateSnapshot;
use janiform_event_log::RunStatus;
use janiform_event_log::default_run_event_paths;
use janiform_event_log::load_run_state_snapshot;
use janiform_event_log::replay_event_log;

use crate::commands::shared::output::write_json_payload;
use crate::context::CommandContext;
use crate::context::OutputMode;
use crate::error::CliError;

#[derive(Debug, Args)]
pub(crate) struct InspectCommand {
    #[command(subcommand)]
    pub(crate) command: InspectSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum InspectSubcommand {
    #[command(about = "Inspect run status, artifacts, checkpoints, and event-log health.")]
    Run(InspectRunCommand),

    #[command(about = "Inspect model identity, provenance, fitted components, and capabilities.")]
    Model(InspectModelCommand),
}

#[derive(Debug, Args)]
pub(crate) struct InspectRunCommand {
    #[arg(value_name = "RUN_DIR", help = "Run directory to inspect.")]
    pub(crate) run_directory: PathBuf,

    #[arg(long, help = "Include event-log and artifact metadata.")]
    pub(crate) include_events: bool,
}

#[derive(Debug, Args)]
pub(crate) struct InspectModelCommand {
    #[arg(value_name = "MODEL_PATH", help = "Persisted model path to inspect.")]
    pub(crate) model_path: PathBuf,

    #[arg(long, help = "Include detailed parameter summaries.")]
    pub(crate) parameters: bool,
}

pub(crate) fn run_inspect_run(
    context: &CommandContext,
    command: InspectRunCommand,
) -> Result<(), CliError> {
    let run_directory = context.paths().resolve_process_path(&command.run_directory);

    let event_paths = default_run_event_paths(&run_directory);

    let state = load_run_state_snapshot(&event_paths.latest_state_snapshot_path)
        .or_else(|_| replay_event_log(&event_paths.event_log_path))
        .map_err(|source| CliError::RunInspectionFailed {
            run_directory: run_directory.clone(),
            source,
        })?;

    match context.output_mode() {
        OutputMode::Json => write_json_payload(&state),
        OutputMode::Silent => Ok(()),
        OutputMode::Human | OutputMode::Quiet => {
            let rendered = render_run_state(
                &state,
                &run_directory,
                command.include_events || context.verbosity() > 0,
                &event_paths.event_log_path,
                &context.stdout_renderer(),
            );
            context
                .stdout_renderer()
                .write_stdout(&rendered)
                .map_err(|source| CliError::OutputWrite { source })
        }
    }
}

fn render_run_state(
    state: &RunStateSnapshot,
    run_directory: &std::path::Path,
    include_events: bool,
    event_log_path: &std::path::Path,
    renderer: &janiform_cli_ui::TerminalRenderer,
) -> String {
    let mut sections = Vec::new();

    let mut rows = vec![
        janiform_cli_ui::KeyValueRow::new("Run", state.run_id.clone()),
        janiform_cli_ui::KeyValueRow::new(
            "Workflow",
            ref_label_or_id(state.workflow.label.as_deref(), &state.workflow.workflow_id),
        ),
        janiform_cli_ui::KeyValueRow::new("Status", run_status_label(state.status)),
    ];

    if let Some(latest_event_timestamp) = &state.latest_event_timestamp_utc {
        rows.push(janiform_cli_ui::KeyValueRow::new(
            "Latest event",
            latest_event_timestamp.clone(),
        ));
    }

    sections.push(
        janiform_cli_ui::StartSummary {
            title: "Run".to_string(),
            rows,
        }
        .render(renderer),
    );

    if state.active_stage.is_some()
        || state.latest_phase.is_some()
        || !state.latest_progress.is_empty()
    {
        sections.push(render_current_state(state, renderer));
    }

    if !state.stage_states.is_empty() {
        sections.push(render_stage_state(state, renderer));
    }

    let artifact_paths = state
        .artifacts
        .iter()
        .map(|artifact| {
            format!(
                "{}: {}",
                artifact.artifact_kind,
                display_run_relative_path(run_directory, &artifact.path)
            )
        })
        .collect::<Vec<String>>();

    if !artifact_paths.is_empty() {
        sections.push(
            janiform_cli_ui::ArtifactPathGroup {
                title: "Artifacts".to_string(),
                paths: artifact_paths,
            }
            .render(renderer),
        );
    }

    if state.checkpoints.available_count > 0 {
        let mut checkpoint_rows = vec![janiform_cli_ui::KeyValueRow::new(
            "Available",
            state.checkpoints.available_count.to_string(),
        )];

        if let Some(latest_path) = &state.checkpoints.latest_path {
            checkpoint_rows.push(janiform_cli_ui::KeyValueRow::new(
                "Latest",
                display_run_relative_path(run_directory, latest_path),
            ));
        }

        sections.push(
            janiform_cli_ui::StartSummary {
                title: "Checkpoints".to_string(),
                rows: checkpoint_rows,
            }
            .render(renderer),
        );
    }

    if !state.warnings.is_empty() {
        sections.push(
            janiform_cli_ui::WarningBlock {
                title: format!("Warnings: {}", state.warnings.len()),
                warnings: state
                    .warnings
                    .iter()
                    .map(|warning| warning.reason.clone())
                    .collect(),
            }
            .render(renderer),
        );
    }

    if include_events {
        let mut event_rows = vec![
            janiform_cli_ui::KeyValueRow::new("Events", state.event_count.to_string()),
            janiform_cli_ui::KeyValueRow::new(
                "Log",
                display_path_relative_to(run_directory, event_log_path),
            ),
        ];

        if let Some(first_event_timestamp) = &state.first_event_timestamp_utc {
            event_rows.push(janiform_cli_ui::KeyValueRow::new(
                "First",
                first_event_timestamp.clone(),
            ));
        }

        if let Some(latest_event_timestamp) = &state.latest_event_timestamp_utc {
            event_rows.push(janiform_cli_ui::KeyValueRow::new(
                "Latest",
                latest_event_timestamp.clone(),
            ));
        }

        sections.push(
            janiform_cli_ui::StartSummary {
                title: "Event log".to_string(),
                rows: event_rows,
            }
            .render(renderer),
        );
    }

    sections.join("\n\n")
}

fn render_current_state(
    state: &RunStateSnapshot,
    renderer: &janiform_cli_ui::TerminalRenderer,
) -> String {
    let mut rows = Vec::new();

    if let Some(stage) = &state.active_stage {
        rows.push(janiform_cli_ui::KeyValueRow::new(
            "Stage",
            stage_label(stage),
        ));
    }

    if let Some(phase) = &state.latest_phase {
        rows.push(janiform_cli_ui::KeyValueRow::new(
            "Phase",
            phase_label(phase),
        ));
    }

    if !state.latest_progress.is_empty() {
        rows.push(janiform_cli_ui::KeyValueRow::new(
            "Progress",
            format!("{} metric updates", state.latest_progress.len()),
        ));
    }

    janiform_cli_ui::StartSummary {
        title: "Current".to_string(),
        rows,
    }
    .render(renderer)
}

fn render_stage_state(
    state: &RunStateSnapshot,
    renderer: &janiform_cli_ui::TerminalRenderer,
) -> String {
    janiform_cli_ui::StageChecklist {
        title: "Stages".to_string(),
        stages: state
            .stage_states
            .iter()
            .map(|stage| janiform_cli_ui::StageChecklistItem {
                name: stage_label(&stage.stage),
                status: stage_status(stage.status),
            })
            .collect(),
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

fn stage_status(status: RunStageStatus) -> janiform_cli_ui::StageChecklistStatus {
    match status {
        RunStageStatus::Pending => janiform_cli_ui::StageChecklistStatus::Pending,
        RunStageStatus::Running => janiform_cli_ui::StageChecklistStatus::Active,
        RunStageStatus::Completed => janiform_cli_ui::StageChecklistStatus::Completed,
        RunStageStatus::Failed => janiform_cli_ui::StageChecklistStatus::Failed,
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

fn display_run_relative_path(run_directory: &std::path::Path, path: &str) -> String {
    display_path_relative_to(run_directory, std::path::Path::new(path))
}

fn display_path_relative_to(base_directory: &std::path::Path, path: &std::path::Path) -> String {
    if let Ok(relative_path) = path.strip_prefix(base_directory) {
        return relative_path.display().to_string();
    }

    path.display().to_string()
}
