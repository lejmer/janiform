use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

use clap::Parser;

use crate::cli::Cli;
use crate::context::CommandContext;
use crate::diagnostics::write_error;
use crate::diagnostics::write_error_for_parse_failure;
use crate::diagnostics::write_error_without_context;
use crate::dispatch::dispatch;
use crate::error::CliError;
use crate::error::CliExitCode;
use crate::logging::initialize_tracing;
use crate::rewrite::rewrite_beginner_fit_arguments;

/// Run the Janiform CLI from a complete argument vector.
///
/// The first argument is the executable name, matching `std::env::args_os`.
pub fn run<I, T>(arguments: I) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let rewritten_arguments = rewrite_beginner_fit_arguments(
        arguments
            .into_iter()
            .map(Into::into)
            .collect::<Vec<OsString>>(),
    );
    let requested_json = rewritten_arguments
        .iter()
        .any(|argument| argument == "--json");

    let cli = match Cli::try_parse_from(rewritten_arguments) {
        Ok(cli) => cli,
        Err(error) => {
            let exit_code = CliExitCode::from_clap_exit_code(error.exit_code());
            if exit_code != CliExitCode::Success {
                let cli_error = CliError::ArgumentParse {
                    message: error.to_string(),
                };

                if let Err(write_error) = write_error_for_parse_failure(cli_error, requested_json) {
                    let mut stderr = std::io::stderr().lock();

                    let _ = writeln!(
                        stderr,
                        "error: failed to write CLI diagnostic: {write_error}"
                    );

                    return CliExitCode::CommandLineUsage.into();
                }

                return exit_code.into();
            }

            if let Err(print_error) = error.print() {
                let mut stderr = std::io::stderr().lock();

                let _ = writeln!(
                    stderr,
                    "error: failed to write CLI diagnostic: {print_error}"
                );

                return CliExitCode::CommandLineUsage.into();
            }

            return exit_code.into();
        }
    };

    let context = match CommandContext::from_global_options(&cli.global) {
        Ok(context) => context,
        Err(error) => {
            let exit_code = error.exit_code();

            if let Err(write_error) = write_error_without_context(error, &cli.global) {
                let mut stderr = std::io::stderr().lock();

                let _ = writeln!(
                    stderr,
                    "error: failed to write CLI diagnostic: {write_error}"
                );

                return CliExitCode::CommandLineUsage.into();
            }

            return exit_code.into();
        }
    };

    initialize_tracing(&context);

    match dispatch(&context, cli.command) {
        Ok(()) => CliExitCode::Success.into(),
        Err(error) => {
            let exit_code = error.exit_code();

            if let Err(write_error) = write_error(error, &context) {
                let mut stderr = std::io::stderr().lock();

                let _ = writeln!(
                    stderr,
                    "error: failed to write CLI diagnostic: {write_error}"
                );

                return CliExitCode::CommandLineUsage.into();
            }

            exit_code.into()
        }
    }
}
