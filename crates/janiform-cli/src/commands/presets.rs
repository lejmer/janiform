use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;
use clap::ValueEnum;

use crate::commands::options::CliBoundaryOptions;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum PresetKind {
    MarketProfile,
    Session,
    KnownNews,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum PresetExportScope {
    Selected,
    Dependencies,
    Bundle,
}

#[derive(Debug, Args)]
pub(crate) struct PresetsCommand {
    #[command(subcommand)]
    pub(crate) command: PresetsSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum PresetsSubcommand {
    #[command(about = "List bundled and user-registered presets.")]
    List(PresetsListCommand),

    #[command(about = "Show a resolved preset document.")]
    Show(PresetsShowCommand),

    #[command(about = "Export resolved presets and dependencies.")]
    Export(PresetsExportCommand),
}

#[derive(Debug, Args)]
pub(crate) struct PresetsListCommand {
    #[command(flatten)]
    pub(crate) boundary: CliBoundaryOptions,

    #[arg(long, value_enum, help = "Restrict output to one preset kind.")]
    pub(crate) kind: Option<PresetKind>,

    #[arg(long, value_name = "PATH", help = "User preset registry path.")]
    pub(crate) user_preset_json: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct PresetsShowCommand {
    #[command(flatten)]
    pub(crate) boundary: CliBoundaryOptions,

    #[arg(value_enum, help = "Preset kind.")]
    pub(crate) kind: PresetKind,

    #[arg(help = "Preset name.")]
    pub(crate) name: String,

    #[arg(long, value_name = "PATH", help = "User preset registry path.")]
    pub(crate) user_preset_json: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct PresetsExportCommand {
    #[command(flatten)]
    pub(crate) boundary: CliBoundaryOptions,

    #[arg(value_enum, help = "Preset kind.")]
    pub(crate) kind: PresetKind,

    #[arg(help = "Preset name.")]
    pub(crate) name: String,

    #[arg(long, value_enum, help = "Export scope.")]
    pub(crate) scope: Option<PresetExportScope>,

    #[arg(long, value_name = "PATH", help = "Export output path.")]
    pub(crate) output_path: Option<PathBuf>,

    #[arg(long, value_name = "PATH", help = "User preset registry path.")]
    pub(crate) user_preset_json: Option<PathBuf>,
}
