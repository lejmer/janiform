use std::path::PathBuf;

use clap::ArgAction;
use clap::Args;
use clap::Subcommand;
use clap::ValueEnum;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum PresetCategory {
    #[value(help = "Market profile presets.")]
    MarketProfile,
    #[value(help = "Market session presets.")]
    Session,
    #[value(help = "Known-news presets.")]
    KnownNews,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum PresetRegistryScope {
    #[value(help = "Resolve from built-in and user-registered presets.")]
    Auto,
    #[value(name = "built_in", help = "Resolve only from built-in presets.")]
    BuiltIn,
    #[value(
        name = "user_registered",
        help = "Resolve only from user-registered presets."
    )]
    UserRegistered,
}

#[derive(Debug, Args)]
#[command(
    about = "Inspect bundled and user-registered presets.",
    long_about = "Inspect bundled and user-registered presets.

Presets provide reusable market-profile, session, and known-news inputs. Use list to discover available names, show to inspect a resolved preset, and export to write a resolved preset plus dependencies."
)]
pub(crate) struct PresetsCommand {
    #[command(subcommand)]
    pub(crate) command: PresetsSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum PresetsSubcommand {
    #[command(
        about = "List bundled and user-registered presets.",
        long_about = "List bundled and user-registered presets.

Use --category or --scope to narrow discovery. Add --preset-registry or --user-preset to include user-defined presets."
    )]
    List(PresetsListCommand),

    #[command(
        about = "Show a resolved preset document.",
        long_about = "Show a resolved preset document.

The command resolves the selected preset through the requested registry scope. Session and known-news presets support --expanded for category-specific detail tables in human output."
    )]
    Show(PresetsShowCommand),

    #[command(
        about = "Export resolved presets and dependencies.",
        long_about = "Export resolved presets and dependencies.

Export writes the selected resolved preset and the nested preset dependencies needed to reuse it outside the current registry context."
    )]
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
        help = "Registry scope for the requested preset. Defaults to automatic."
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
        help = "User preset file path."
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
        help = "Registry scope for the requested preset. Defaults to automatic."
    )]
    pub(crate) scope: Option<PresetRegistryScope>,

    #[arg(long, value_name = "PATH", help = "Export output path.")]
    pub(crate) output_path: Option<PathBuf>,

    #[command(flatten)]
    pub(crate) registry: PresetRegistryArgs,
}
