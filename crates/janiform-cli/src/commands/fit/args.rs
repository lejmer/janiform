use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;

use crate::commands::options::CalibrationStageOption;
use crate::commands::options::ExecutionMode;
use crate::commands::options::RunIdentityOptions;
use crate::commands::options::TickDataOptions;
use crate::commands::options::WorkflowScope;

#[derive(Debug, Args)]
#[command(
    about = "Fit Janiform models from market data.",
    long_about = "Fit Janiform models from market data.

The default fit command validates the supplied market data, calibrates the Janiform model, and saves the fitted model and fit report.

Workflow split:
  janiform fit          run the fit workflow
  janiform fit inspect  preview inferred inputs and configuration
  janiform fit plan     validate inputs and preview the planned run
  janiform fit staged   run from a prepared calibration input file

Disk execution writes a run directory with the fitted model, fit report, progress log, latest status snapshot, and checkpoints. Use inspect and report to follow a run in progress or review a completed run."
)]
pub(crate) struct FitCommand {
    #[command(flatten)]
    pub(crate) inputs: TickDataOptions,

    #[command(flatten)]
    pub(crate) run: RunIdentityOptions,

    #[arg(
        long,
        value_enum,
        help = "Select which calibration workflow to prepare.",
        long_help = "Select which calibration workflow to prepare.

Use full for the standard path. Supplying an option surface allows the full workflow to include option-surface calibration. Explicit scope values are useful when automation must pin the intended workflow."
    )]
    pub(crate) workflow_scope: Option<WorkflowScope>,

    #[arg(
        long,
        value_enum,
        help = "Choose whether fit outputs are saved to disk.",
        long_help = "Choose whether fit outputs are saved to disk.

Disk execution is the durable mode for long-running calibration. It writes the run directory, fitted model, fit report, progress log, latest status snapshot, and checkpoints. Memory execution is available only for supported preparation and inspection tasks."
    )]
    pub(crate) execution_mode: Option<ExecutionMode>,

    #[arg(
        long = "no-persist",
        conflicts_with_all = ["output_dir", "execution_mode"],
        help = "Run fit without keeping the run directory or model artifact.",
        long_help = "Run fit without keeping the run directory or model artifact.

By default, janiform fit writes a durable run directory for downstream CLI commands. Use --no-persist for dry runs and one-off checks where the fitted model does not need to be reused."
    )]
    pub(crate) no_persist: bool,

    #[arg(
        long,
        value_name = "COUNT",
        help = "CPU count available to calibration preparation."
    )]
    pub(crate) cpu_count: Option<usize>,

    #[arg(
        long,
        value_name = "HOURS",
        help = "Mean-reversion calibration grid interval in hours."
    )]
    pub(crate) mean_reversion_grid_interval_hours: Option<f64>,

    #[arg(
        long,
        value_name = "COUNT",
        help = "Joint-refinement restart candidate budget."
    )]
    pub(crate) dynamics_search_restart_candidate_budget: Option<usize>,

    #[arg(
        long,
        value_name = "Z",
        help = "Stylized-fact Huber cutoff for the calibration objective."
    )]
    pub(crate) stylized_fact_huber_cutoff: Option<f64>,

    #[arg(
        long,
        value_name = "Z",
        help = "Stylized-fact admissibility z-threshold for the calibration objective."
    )]
    pub(crate) stylized_fact_admissibility_z_threshold: Option<f64>,

    #[arg(
        long,
        value_name = "REFERENCE",
        help = "Resume disk execution from a checkpoint reference.",
        long_help = "Resume disk execution from a checkpoint reference.

Checkpoint references identify a saved checkpoint, for example checkpoint://RUN_ID/last-success or checkpoint://RUN_ID/STAGE_TOKEN. Resume is only valid for disk execution."
    )]
    pub(crate) resume_checkpoint_reference: Option<String>,

    #[arg(
        long,
        requires = "resume_checkpoint_reference",
        value_enum,
        help = "Stage to start from when resuming a compatible checkpoint."
    )]
    pub(crate) resume_from_stage: Option<CalibrationStageOption>,

    #[command(subcommand)]
    pub(crate) command: Option<FitSubcommand>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum FitSubcommand {
    #[command(
        about = "Inspect inferred fit configuration without running calibration.",
        long_about = "Inspect inferred fit configuration without running calibration.

This command resolves the supplied market-data inputs and calibration scope, then renders the inferred calibration configuration. It does not run calibration."
    )]
    Inspect(FitInspectCommand),

    #[command(
        about = "Validate inputs and derive the fit plan.",
        long_about = "Validate inputs and derive the fit plan.

This command prepares the same calibration plan used by the default fit command, but stops before execution. Use it to review resolved inputs, output paths, and execution policy."
    )]
    Plan(FitPlanCommand),

    #[command(
        about = "Run from prepared calibration inputs.",
        long_about = "Run from prepared calibration inputs.

This advanced command is useful when a separate process prepares calibration inputs before the fit runs. Use staged plan to create the input file and staged run to execute it."
    )]
    Staged(FitStagedCommand),
}

#[derive(Debug, Args)]
#[command(
    about = "Inspect inferred fit configuration without running calibration.",
    long_about = "Inspect inferred fit configuration without running calibration.

The output summarizes resolved market data, known-news inputs, session inputs, calibration scope, and preparation resources. This inspection command does not run calibration and does not write a fitted model, fit report, progress log, or checkpoint."
)]
pub(crate) struct FitInspectCommand {
    #[command(flatten)]
    pub(crate) inputs: TickDataOptions,

    #[command(flatten)]
    pub(crate) run: RunIdentityOptions,

    #[arg(
        long,
        value_enum,
        help = "Select which calibration workflow to inspect."
    )]
    pub(crate) workflow_scope: Option<WorkflowScope>,

    #[arg(
        long,
        value_name = "COUNT",
        help = "CPU count available to inspection preparation."
    )]
    pub(crate) cpu_count: Option<usize>,
}

#[derive(Debug, Args)]
#[command(
    about = "Validate inputs and derive the fit plan.",
    long_about = "Validate inputs and derive the fit plan.

The plan command validates user inputs, resolves output paths, chooses the calibration scope, and can write a prepared calibration input file with --staged-execution-input-output-path. It does not run calibration."
)]
pub(crate) struct FitPlanCommand {
    #[command(flatten)]
    pub(crate) inputs: TickDataOptions,

    #[command(flatten)]
    pub(crate) run: RunIdentityOptions,

    #[arg(
        long,
        value_enum,
        help = "Choose whether planned fit outputs are disk-backed."
    )]
    pub(crate) execution_mode: Option<ExecutionMode>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Write the prepared calibration input file."
    )]
    pub(crate) staged_execution_input_output_path: Option<PathBuf>,

    #[arg(
        long,
        value_name = "COUNT",
        help = "CPU count available to calibration planning."
    )]
    pub(crate) cpu_count: Option<usize>,

    #[arg(
        long,
        value_name = "HOURS",
        help = "Mean-reversion calibration grid interval in hours."
    )]
    pub(crate) mean_reversion_grid_interval_hours: Option<f64>,

    #[arg(
        long,
        value_name = "COUNT",
        help = "Joint-refinement restart candidate budget."
    )]
    pub(crate) dynamics_search_restart_candidate_budget: Option<usize>,

    #[arg(
        long,
        value_name = "Z",
        help = "Stylized-fact Huber cutoff for the calibration objective."
    )]
    pub(crate) stylized_fact_huber_cutoff: Option<f64>,

    #[arg(
        long,
        value_name = "Z",
        help = "Stylized-fact admissibility z-threshold for the calibration objective."
    )]
    pub(crate) stylized_fact_admissibility_z_threshold: Option<f64>,

    #[arg(long, value_enum, help = "Select which calibration workflow to plan.")]
    pub(crate) workflow_scope: Option<WorkflowScope>,
}

#[derive(Debug, Args)]
#[command(
    about = "Run from prepared calibration inputs.",
    long_about = "Run from prepared calibration inputs.

With no nested subcommand, this command runs a prepared calibration input file. Use janiform fit staged plan to create that file first."
)]
pub(crate) struct FitStagedCommand {
    #[command(flatten)]
    pub(crate) staged: StagedExecutionOptions,

    #[command(subcommand)]
    pub(crate) command: Option<FitStagedSubcommand>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum FitStagedSubcommand {
    #[command(
        about = "Create a prepared calibration input file.",
        long_about = "Create a prepared calibration input file.

This command validates fit inputs and writes the prepared input file consumed by janiform fit staged run. It does not run calibration."
    )]
    Plan(Box<FitStagedPlanCommand>),

    #[command(
        about = "Run a prepared calibration input file.",
        long_about = "Run a prepared calibration input file.

This command runs a prepared calibration input file. Disk runs write the fitted model, fit report, progress log, latest status snapshot, and checkpoints."
    )]
    Run(Box<FitStagedRunCommand>),
}

#[derive(Debug, Args)]
#[command(
    about = "Create a prepared calibration input file.",
    long_about = "Create a prepared calibration input file.

The generated file captures validated inputs, calibration scope, run identity, output paths, and runtime resources for a later fit run."
)]
pub(crate) struct FitStagedPlanCommand {
    #[command(flatten)]
    pub(crate) inputs: TickDataOptions,

    #[command(flatten)]
    pub(crate) run: RunIdentityOptions,

    #[arg(
        long,
        value_name = "PATH",
        help = "Path for the prepared calibration input file."
    )]
    pub(crate) execution_input_output_path: Option<PathBuf>,

    #[arg(
        long,
        value_name = "COUNT",
        help = "CPU count available to staged-input preparation."
    )]
    pub(crate) cpu_count: Option<usize>,

    #[arg(
        long,
        value_enum,
        help = "Select which calibration workflow to encode."
    )]
    pub(crate) workflow_scope: Option<WorkflowScope>,
}

#[derive(Debug, Args)]
#[command(
    about = "Run a prepared calibration input file.",
    long_about = "Run a prepared calibration input file.

The input file contains the prepared calibration run. Command-line checkpoint options can select the disk persistence root or resume from a compatible checkpoint reference."
)]
pub(crate) struct FitStagedRunCommand {
    #[command(flatten)]
    pub(crate) staged: StagedExecutionOptions,
}

#[derive(Debug, Args)]
pub(crate) struct StagedExecutionOptions {
    #[arg(long, value_name = "PATH", help = "Prepared calibration input file.")]
    pub(crate) execution_input_path: Option<PathBuf>,

    #[arg(
        long,
        value_name = "DIR",
        help = "Disk persistence root for checkpoints and staged artifacts.",
        long_help = "Disk persistence root for checkpoints and staged artifacts.

When omitted, disk execution uses the persistence root from the prepared calibration input file or the default run directory."
    )]
    pub(crate) disk_checkpoint_directory: Option<PathBuf>,

    #[arg(
        long,
        value_name = "REFERENCE",
        help = "Resume disk execution from a checkpoint reference.",
        long_help = "Resume disk execution from a checkpoint reference.

Checkpoint references identify a saved checkpoint such as checkpoint://RUN_ID/last-success or checkpoint://RUN_ID/STAGE_TOKEN."
    )]
    pub(crate) resume_checkpoint_reference: Option<String>,

    #[arg(
        long,
        requires = "resume_checkpoint_reference",
        value_enum,
        help = "Stage to start from when resuming a compatible checkpoint."
    )]
    pub(crate) resume_from_stage: Option<CalibrationStageOption>,
}
