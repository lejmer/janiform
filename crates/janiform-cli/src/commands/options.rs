use std::path::PathBuf;

use clap::Args;
use clap::ValueEnum;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum ExecutionMode {
    Memory,
    Disk,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum WorkflowScope {
    Full,
    SeasonalityOnly,
    MarketDataOnly,
}

#[derive(Debug, Args)]
pub(crate) struct RunIdentityOptions {
    #[arg(long, value_name = "ID", help = "Stable run identifier.")]
    pub(crate) run_id: Option<String>,

    #[arg(long, value_name = "DIR", help = "Directory for command artifacts.")]
    pub(crate) output_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct ModelPathOptions {
    #[arg(long, value_name = "PATH", help = "Persisted Janiform model path.")]
    pub(crate) model_path: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct OutputPathOptions {
    #[arg(long, value_name = "PATH", help = "Primary command output path.")]
    pub(crate) output_path: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct TickDataOptions {
    #[arg(long, value_name = "PATH", help = "Tick data input path.")]
    pub(crate) tick_data_path: Option<PathBuf>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Observed option surface input path."
    )]
    pub(crate) option_surface_path: Option<PathBuf>,

    #[arg(long, value_name = "PATH", help = "Known-news input path.")]
    pub(crate) known_news_path: Option<PathBuf>,
}
