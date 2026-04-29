use std::path::PathBuf;

use clap::ArgAction;
use clap::Args;
use clap::Subcommand;
use clap::ValueEnum;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum PresetCategory {
    MarketProfile,
    Session,
    KnownNews,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum PresetRegistryScope {
    Auto,
    #[value(name = "built_in")]
    BuiltIn,
    #[value(name = "user_registered")]
    UserRegistered,
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
    #[arg(long, value_enum, help = "Restrict output to one preset category.")]
    pub(crate) category: Option<PresetCategory>,

    #[arg(long, value_enum, help = "Restrict output to one registry scope.")]
    pub(crate) scope: Option<PresetRegistryScope>,

    #[command(flatten)]
    pub(crate) registry: PresetRegistryArgs,
}

#[derive(Debug, Args)]
pub(crate) struct PresetsShowCommand {
    #[command(subcommand)]
    pub(crate) command: PresetsShowSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum PresetsShowSubcommand {
    #[command(about = "Show a market-profile preset.")]
    MarketProfile(PresetsShowPresetCommand),

    #[command(about = "Show a session preset.")]
    Session(PresetsShowExpandedPresetCommand),

    #[command(about = "Show a known-news preset.")]
    KnownNews(PresetsShowExpandedPresetCommand),
}

#[derive(Debug, Args)]
pub(crate) struct PresetsShowPresetCommand {
    #[arg(help = "Preset name.")]
    pub(crate) name: String,

    #[command(flatten)]
    pub(crate) registry: PresetsShowRegistryArgs,
}

#[derive(Debug, Args)]
pub(crate) struct PresetsShowExpandedPresetCommand {
    #[arg(help = "Preset name.")]
    pub(crate) name: String,

    #[command(flatten)]
    pub(crate) registry: PresetsShowRegistryArgs,

    #[arg(
        long,
        action = ArgAction::SetTrue,
        help = "Show expanded category-specific detail tables in human output."
    )]
    pub(crate) expanded: bool,
}

#[derive(Debug, Args)]
pub(crate) struct PresetsShowRegistryArgs {
    #[arg(
        long,
        value_enum,
        help = "Registry scope for the requested preset. Default: automatic."
    )]
    pub(crate) scope: Option<PresetRegistryScope>,

    #[command(flatten)]
    pub(crate) registry: PresetRegistryArgs,
}

#[derive(Debug, Args)]
pub(crate) struct PresetRegistryArgs {
    #[arg(
        long,
        value_name = "PATH",
        action = ArgAction::Append,
        help = "User preset registry manifest path."
    )]
    pub(crate) preset_registry: Vec<PathBuf>,

    #[arg(
        long,
        value_name = "PATH",
        action = ArgAction::Append,
        help = "User preset artifact path."
    )]
    pub(crate) user_preset: Vec<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct PresetsExportCommand {
    #[arg(value_enum, help = "Preset category.")]
    pub(crate) category: PresetCategory,

    #[arg(help = "Preset name.")]
    pub(crate) name: String,

    #[arg(
        long,
        value_enum,
        help = "Registry scope for the requested preset. Default: automatic."
    )]
    pub(crate) scope: Option<PresetRegistryScope>,

    #[arg(long, value_name = "PATH", help = "Export output path.")]
    pub(crate) output_path: Option<PathBuf>,

    #[command(flatten)]
    pub(crate) registry: PresetRegistryArgs,
}
