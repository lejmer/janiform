use tracing::level_filters::LevelFilter;

use crate::context::CommandContext;
use crate::context::EventStreamFormat;

pub(crate) fn initialize_tracing(context: &CommandContext) {
    let level = if context.event_settings().trace_events {
        LevelFilter::TRACE
    } else {
        match context.verbosity() {
            0 => LevelFilter::WARN,
            1 => LevelFilter::INFO,
            _ => LevelFilter::DEBUG,
        }
    };

    match context.event_settings().stream_format {
        Some(EventStreamFormat::Jsonl) => {
            let _ = tracing_subscriber::fmt()
                .json()
                .with_writer(std::io::stderr)
                .with_ansi(false)
                .with_max_level(level)
                .try_init();
        }
        None => {
            let _ = tracing_subscriber::fmt()
                .compact()
                .with_writer(std::io::stderr)
                .with_ansi(context.color_policy().stderr_ansi)
                .with_max_level(level)
                .try_init();
        }
    }
}
