use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;

use crate::commands::options::CliBoundaryOptions;
use crate::commands::options::OutputPathOptions;

#[derive(Debug, Args)]
pub(crate) struct SeasonalityCommand {
    #[command(subcommand)]
    pub(crate) command: SeasonalitySubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum SeasonalitySubcommand {
    #[command(
        name = "preprocess-timestamps",
        about = "Preprocess timestamp columns for seasonality workflows."
    )]
    PreprocessTimestamps(PreprocessTimestampsCommand),

    #[command(about = "Build seasonality diagnostics from timestamp data.")]
    Diagnostics(SeasonalityDiagnosticsCommand),
}

#[derive(Debug, Args)]
pub(crate) struct PreprocessTimestampsCommand {
    #[command(flatten)]
    pub(crate) boundary: CliBoundaryOptions,

    #[arg(long, value_name = "PATH", help = "Input data path.")]
    pub(crate) input_path: Option<PathBuf>,

    #[arg(long, value_name = "NAME", help = "Timestamp column name.")]
    pub(crate) timestamp_column: Option<String>,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,
}

#[derive(Debug, Args)]
pub(crate) struct SeasonalityDiagnosticsCommand {
    #[command(flatten)]
    pub(crate) boundary: CliBoundaryOptions,

    #[arg(long, value_name = "PATH", help = "Input data path.")]
    pub(crate) input_path: Option<PathBuf>,

    #[arg(long, value_name = "NAME", help = "Timestamp column name.")]
    pub(crate) timestamp_column: Option<String>,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,
}
