use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;

use crate::commands::options::ExecutionMode;
use crate::commands::options::RunIdentityOptions;
use crate::commands::options::TickDataOptions;
use crate::commands::options::WorkflowScope;

#[derive(Debug, Args)]
pub(crate) struct FitCommand {
    #[command(flatten)]
    pub(crate) inputs: TickDataOptions,

    #[command(flatten)]
    pub(crate) run: RunIdentityOptions,

    #[arg(long, value_enum, help = "High-level calibration workflow scope.")]
    pub(crate) workflow_scope: Option<WorkflowScope>,

    #[command(subcommand)]
    pub(crate) command: Option<FitSubcommand>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum FitSubcommand {
    #[command(about = "Inspect inferred fit configuration without running calibration.")]
    Inspect(FitInspectCommand),

    #[command(about = "Validate inputs and derive the high-level fit plan.")]
    Plan(FitPlanCommand),

    #[command(about = "Run or prepare explicit staged calibration contracts.")]
    Staged(FitStagedCommand),
}

#[derive(Debug, Args)]
pub(crate) struct FitInspectCommand {
    #[command(flatten)]
    pub(crate) inputs: TickDataOptions,

    #[command(flatten)]
    pub(crate) run: RunIdentityOptions,

    #[arg(long, value_enum, help = "High-level calibration workflow scope.")]
    pub(crate) workflow_scope: Option<WorkflowScope>,

    #[arg(
        long,
        value_name = "COUNT",
        help = "CPU count available to inspection preparation."
    )]
    pub(crate) cpu_count: Option<usize>,
}

#[derive(Debug, Args)]
pub(crate) struct FitPlanCommand {
    #[command(flatten)]
    pub(crate) inputs: TickDataOptions,

    #[command(flatten)]
    pub(crate) run: RunIdentityOptions,

    #[arg(
        long,
        value_enum,
        help = "Execution policy for generated plan artifacts."
    )]
    pub(crate) execution_mode: Option<ExecutionMode>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Write the prepared staged execution-input contract."
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
        help = "Joint refinement restart candidate budget."
    )]
    pub(crate) dynamics_search_restart_candidate_budget: Option<usize>,

    #[arg(long, value_enum, help = "High-level calibration workflow scope.")]
    pub(crate) workflow_scope: Option<WorkflowScope>,
}

#[derive(Debug, Args)]
pub(crate) struct FitStagedCommand {
    #[command(flatten)]
    pub(crate) staged: StagedExecutionOptions,

    #[command(subcommand)]
    pub(crate) command: Option<FitStagedSubcommand>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum FitStagedSubcommand {
    #[command(about = "Build a staged execution-input contract.")]
    Plan(FitStagedPlanCommand),

    #[command(about = "Run a staged execution-input contract.")]
    Run(FitStagedRunCommand),
}

#[derive(Debug, Args)]
pub(crate) struct FitStagedPlanCommand {
    #[command(flatten)]
    pub(crate) inputs: TickDataOptions,

    #[command(flatten)]
    pub(crate) run: RunIdentityOptions,

    #[arg(
        long,
        value_name = "PATH",
        help = "Generated staged execution-input path."
    )]
    pub(crate) execution_input_output_path: Option<PathBuf>,

    #[arg(
        long,
        value_name = "COUNT",
        help = "CPU count available to staged-input preparation."
    )]
    pub(crate) cpu_count: Option<usize>,

    #[arg(long, value_enum, help = "High-level calibration workflow scope.")]
    pub(crate) workflow_scope: Option<WorkflowScope>,
}

#[derive(Debug, Args)]
pub(crate) struct FitStagedRunCommand {
    #[command(flatten)]
    pub(crate) staged: StagedExecutionOptions,
}

#[derive(Debug, Args)]
pub(crate) struct StagedExecutionOptions {
    #[arg(
        long,
        value_name = "PATH",
        help = "Prepared staged execution-input contract."
    )]
    pub(crate) execution_input_path: Option<PathBuf>,

    #[arg(long, value_name = "DIR", help = "Directory for disk checkpoints.")]
    pub(crate) disk_checkpoint_directory: Option<PathBuf>,
}
