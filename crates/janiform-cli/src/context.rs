use std::path::PathBuf;

use clap::ArgAction;
use clap::Args;
use clap::ValueEnum;
use janiform_foundations::cancellation::CancellationToken;
use janiform_terminal_ui::TerminalRenderer;
use serde::Deserialize;
use serde::Serialize;

use crate::config::CliConfiguration;
use crate::error::CliError;
use crate::paths::PathResolver;

/// ANSI color selection for terminal CLI output.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum ColorChoice {
    #[value(help = "Use color when the output stream supports it. Respects NO_COLOR.")]
    Auto,
    #[value(help = "Use color for terminal output even when terminal detection is inconclusive.")]
    Always,
    #[value(help = "Disable color in terminal output.")]
    Never,
}

/// Final command output contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputMode {
    Terminal,
    Json,
    Quiet,
    Silent,
}

/// Machine event stream format.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum EventStreamFormat {
    #[value(help = "Write one JSON event object per line to stderr.")]
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
        help = "Load CLI defaults from a TOML document.",
        long_help = "Load CLI defaults from a TOML document.

Configuration can set output and diagnostic defaults such as color, verbosity, JSON mode, quiet mode, silent mode, and event streaming. Model choices, numerical settings, and command-specific inputs stay explicit on each command."
    )]
    pub(crate) config: Option<PathBuf>,

    #[arg(
        long,
        global = true,
        value_enum,
        value_name = "WHEN",
        help = "Control ANSI color for terminal output.",
        long_help = "Control ANSI color for terminal output.

Values:
  auto    use color when the stream supports it and NO_COLOR is not set
  always  force color for terminal output
  never   disable color

JSON and JSONL output never include ANSI color."
    )]
    pub(crate) color: Option<ColorChoice>,

    #[arg(
        long,
        global = true,
        action = ArgAction::SetTrue,
        conflicts_with_all = ["quiet", "verbose", "json"],
        help = "Suppress normal output while preserving warnings and errors.",
        long_help = "Suppress normal terminal output while preserving warnings and errors.

Use --silent when automation only needs the exit code and failure diagnostics. It conflicts with --quiet, --verbose, and --json because those options define different output modes."
    )]
    pub(crate) silent: bool,

    #[arg(
        long,
        global = true,
        action = ArgAction::SetTrue,
        conflicts_with_all = ["silent", "verbose", "json"],
        help = "Print final status and output paths only.",
        long_help = "Print final status and output paths only.

Use --quiet for logs and CI jobs that should keep a concise human-readable record without live progress tables. It conflicts with --silent, --verbose, and --json."
    )]
    pub(crate) quiet: bool,

    #[arg(
        long,
        short = 'v',
        global = true,
        action = ArgAction::Count,
        conflicts_with_all = ["silent", "quiet"],
        help = "Increase terminal diagnostic verbosity.",
        long_help = "Increase terminal diagnostic verbosity.

Use -v for INFO diagnostics and -vv for DEBUG diagnostics. Verbose output affects terminal diagnostics, not the final JSON result. It conflicts with --silent and --quiet."
    )]
    pub(crate) verbose: u8,

    #[arg(
        long,
        global = true,
        action = ArgAction::SetTrue,
        conflicts_with_all = ["silent", "quiet"],
        help = "Write one final JSON object to stdout.",
        long_help = "Write one final JSON object to stdout.

JSON mode is for machine consumption. Terminal progress and summary output are suppressed so stdout remains a single JSON document. Warnings and errors are written as JSON diagnostics when possible. Use --events jsonl for a streaming machine event log."
    )]
    pub(crate) json: bool,

    #[arg(
        long,
        global = true,
        value_enum,
        value_name = "FORMAT",
        help = "Write machine events to stderr.",
        long_help = "Write machine events to stderr in the selected format.

Use --events jsonl for one JSON event object per line. Event streams are separate from the final stdout output, so they can be used with terminal output, quiet output, or --json."
    )]
    pub(crate) events: Option<EventStreamFormat>,

    #[arg(
        long,
        global = true,
        action = ArgAction::SetTrue,
        help = "Include trace-level records in the machine event stream.",
        long_help = "Include trace-level records in the machine event stream.

This requires --events jsonl or an equivalent configured event stream. Trace records are intended for debugging command execution and are not part of the final stdout result."
    )]
    pub(crate) trace_events: bool,
}

/// Immutable context shared by every Rust CLI command.
#[derive(Debug)]
pub struct CommandContext {
    output_mode: OutputMode,
    color: ColorPolicy,
    verbosity: u8,
    cancellation_token: CancellationToken,
    paths: PathResolver,
    events: EventSettings,
    config_path: Option<PathBuf>,
}

impl CommandContext {
    pub(crate) fn from_global_options(
        options: &GlobalCliOptions,
        cancellation_token: CancellationToken,
    ) -> Result<Self, CliError> {
        let paths = PathResolver::from_process()?;

        let config_path = options
            .config
            .as_ref()
            .map(|path| paths.resolve_process_path(path));

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
            cancellation_token,
            paths,
            events,
            config_path,
        })
    }

    /// Final output mode after applying configuration defaults and CLI overrides.
    pub fn output_mode(&self) -> OutputMode {
        self.output_mode
    }

    /// Shared portable path resolver for process-relative CLI paths.
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

    /// Cooperative cancellation token shared by the active CLI command.
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation_token.shared_handle()
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

    OutputMode::Terminal
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

    OutputMode::Terminal
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
