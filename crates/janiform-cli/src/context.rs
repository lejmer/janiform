use std::path::PathBuf;

use clap::ArgAction;
use clap::Args;
use clap::ValueEnum;
use janiform_cli_ui::TerminalRenderer;
use serde::Deserialize;
use serde::Serialize;

use crate::config::CliConfiguration;
use crate::error::CliError;
use crate::paths::PathResolver;

/// ANSI color selection for terminal CLI output.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

/// Final command output contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputMode {
    Human,
    Json,
    Quiet,
    Silent,
}

/// Machine event stream format.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum EventStreamFormat {
    Jsonl,
}

/// Event-stream configuration shared by every command.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct EventSettings {
    pub stream_format: Option<EventStreamFormat>,
    pub trace_events: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ColorPolicy {
    pub(crate) choice: ColorChoice,
    pub(crate) stdout_ansi: bool,
    pub(crate) stderr_ansi: bool,
}

#[derive(Debug, Args)]
#[command(next_help_heading = "Global Options")]
pub(crate) struct GlobalCliOptions {
    #[arg(
        long,
        global = true,
        value_name = "PATH",
        help = "TOML CLI configuration document."
    )]
    pub(crate) config: Option<PathBuf>,

    #[arg(
        long,
        global = true,
        value_enum,
        value_name = "WHEN",
        help = "ANSI color policy for terminal diagnostics."
    )]
    pub(crate) color: Option<ColorChoice>,

    #[arg(
        long,
        global = true,
        action = ArgAction::SetTrue,
        conflicts_with_all = ["quiet", "verbose", "json"],
        help = "Suppress normal output while still printing warnings and errors."
    )]
    pub(crate) silent: bool,

    #[arg(
        long,
        global = true,
        action = ArgAction::SetTrue,
        conflicts_with_all = ["silent", "verbose", "json"],
        help = "Print final status and artifact paths only."
    )]
    pub(crate) quiet: bool,

    #[arg(
        long,
        short = 'v',
        global = true,
        action = ArgAction::Count,
        conflicts_with_all = ["silent", "quiet"],
        help = "Increase terminal diagnostic verbosity."
    )]
    pub(crate) verbose: u8,

    #[arg(
        long,
        global = true,
        action = ArgAction::SetTrue,
        conflicts_with_all = ["silent", "quiet"],
        help = "Emit one final JSON object on stdout."
    )]
    pub(crate) json: bool,

    #[arg(
        long,
        global = true,
        value_enum,
        value_name = "FORMAT",
        help = "Emit machine events to stderr in the selected format."
    )]
    pub(crate) events: Option<EventStreamFormat>,

    #[arg(
        long,
        global = true,
        action = ArgAction::SetTrue,
        help = "Include trace-level diagnostics in the machine event stream."
    )]
    pub(crate) trace_events: bool,
}

/// Immutable context shared by every Rust CLI command.
#[derive(Debug)]
pub struct CommandContext {
    output_mode: OutputMode,
    color: ColorPolicy,
    verbosity: u8,
    paths: PathResolver,
    events: EventSettings,
    config_path: Option<PathBuf>,
}

impl CommandContext {
    pub(crate) fn from_global_options(options: &GlobalCliOptions) -> Result<Self, CliError> {
        let paths = PathResolver::from_process()?;

        let config_path = match &options.config {
            Some(path) => Some(paths.resolve_process_path(path)),
            None => None,
        };

        let configuration = match &config_path {
            Some(path) => CliConfiguration::load(path)?,
            None => CliConfiguration::default(),
        };

        if let Some(path) = &config_path {
            configuration.validate(path)?;
        }

        let output_mode = resolve_output_mode(options, &configuration);

        let color_choice = options
            .color
            .or(configuration.output.color)
            .unwrap_or(ColorChoice::Auto);

        let color = resolve_color_policy(color_choice, output_mode);

        let verbosity = match options.verbose {
            0 => configuration.output.verbose.unwrap_or(0),
            value => value,
        };

        let events = EventSettings {
            stream_format: options.events.or(configuration.output.events),
            trace_events: options.trace_events
                || configuration.output.trace_events.unwrap_or(false),
        };
        if events.trace_events && events.stream_format.is_none() {
            return Err(CliError::TraceEventsWithoutEventStream);
        }

        Ok(Self {
            output_mode,
            color,
            verbosity,
            paths,
            events,
            config_path,
        })
    }

    /// Final output mode after applying configuration defaults and CLI overrides.
    pub fn output_mode(&self) -> OutputMode {
        self.output_mode
    }

    /// Shared portable path resolver for process-relative and repository-relative paths.
    pub fn paths(&self) -> &PathResolver {
        &self.paths
    }

    /// Machine event-stream settings requested for this command.
    pub fn event_settings(&self) -> EventSettings {
        self.events
    }

    /// CLI diagnostic verbosity level.
    pub fn verbosity(&self) -> u8 {
        self.verbosity
    }

    /// Resolved configuration path, if a TOML CLI configuration document was loaded.
    pub fn config_path(&self) -> Option<&std::path::Path> {
        self.config_path.as_deref()
    }

    pub(crate) fn color_policy(&self) -> ColorPolicy {
        self.color
    }

    /// Semantic renderer for final terminal summaries written to stdout.
    pub const fn stdout_renderer(&self) -> TerminalRenderer {
        TerminalRenderer::new(self.color.stdout_ansi, None)
    }

    /// Semantic renderer for terminal diagnostics and live progress written to stderr.
    pub const fn stderr_renderer(&self) -> TerminalRenderer {
        TerminalRenderer::new(self.color.stderr_ansi, None)
    }
}

pub(crate) fn output_mode_from_options(options: &GlobalCliOptions) -> OutputMode {
    if options.json {
        return OutputMode::Json;
    }

    if options.silent {
        return OutputMode::Silent;
    }

    if options.quiet {
        return OutputMode::Quiet;
    }

    OutputMode::Human
}

fn resolve_output_mode(options: &GlobalCliOptions, configuration: &CliConfiguration) -> OutputMode {
    if options.json || options.silent || options.quiet {
        return output_mode_from_options(options);
    }

    if configuration.output.json.unwrap_or(false) {
        return OutputMode::Json;
    }

    if configuration.output.silent.unwrap_or(false) {
        return OutputMode::Silent;
    }

    if configuration.output.quiet.unwrap_or(false) {
        return OutputMode::Quiet;
    }

    OutputMode::Human
}

fn resolve_color_policy(choice: ColorChoice, output_mode: OutputMode) -> ColorPolicy {
    let machine_output = matches!(output_mode, OutputMode::Json);

    let ansi_allowed = match choice {
        ColorChoice::Always => !machine_output,
        ColorChoice::Never => false,
        ColorChoice::Auto => !machine_output,
    };

    let stdout_ansi = match choice {
        ColorChoice::Always => ansi_allowed,
        ColorChoice::Auto => ansi_allowed && TerminalRenderer::stdout_color_enabled(),
        ColorChoice::Never => false,
    };

    let stderr_ansi = match choice {
        ColorChoice::Always => ansi_allowed,
        ColorChoice::Auto => ansi_allowed && TerminalRenderer::stderr_color_enabled(),
        ColorChoice::Never => false,
    };

    ColorPolicy {
        choice,
        stdout_ansi,
        stderr_ansi,
    }
}
