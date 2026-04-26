use std::io;
use std::io::Write;

use serde::Serialize;

use crate::context::CommandContext;
use crate::context::GlobalCliOptions;
use crate::context::OutputMode;
use crate::context::output_mode_from_options;
use crate::error::CliError;

pub(crate) fn write_error(error: CliError, context: &CommandContext) -> io::Result<()> {
    match context.output_mode() {
        OutputMode::Json => write_json_error(error),
        OutputMode::Human | OutputMode::Quiet | OutputMode::Silent => {
            write_human_error(error, context.color_policy().stderr_ansi)
        }
    }
}

pub(crate) fn write_error_without_context(
    error: CliError,
    options: &GlobalCliOptions,
) -> io::Result<()> {
    match output_mode_from_options(options) {
        OutputMode::Json => write_json_error(error),
        OutputMode::Human | OutputMode::Quiet | OutputMode::Silent => {
            write_human_error(error, false)
        }
    }
}

pub(crate) fn write_error_for_parse_failure(
    error: CliError,
    requested_json: bool,
) -> io::Result<()> {
    if requested_json {
        write_json_error(error)
    } else {
        write_human_error(error, false)
    }
}

fn write_human_error(error: CliError, ansi: bool) -> io::Result<()> {
    let report = miette::Report::new(error);

    let mut stderr = io::stderr().lock();

    if ansi {
        writeln!(stderr, "\u{1b}[31merror\u{1b}[0m: {report}")?;
    } else {
        writeln!(stderr, "error: {report}")?;
    }

    Ok(())
}

fn write_json_error(error: CliError) -> io::Result<()> {
    let message = error.to_string();

    let payload = JsonError {
        status: "error",
        code: error.code(),
        message: &message,
        exit_code: error.exit_code() as u8,
    };

    let mut stdout = io::stdout().lock();

    serde_json::to_writer(&mut stdout, &payload)?;

    writeln!(stdout)?;

    Ok(())
}

#[derive(Serialize)]
struct JsonError<'a> {
    status: &'static str,
    code: &'static str,
    message: &'a str,
    exit_code: u8,
}
