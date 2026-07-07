use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

use clap::Parser;
use janiform_foundations::cancellation::CancellationError;
use janiform_foundations::cancellation::CancellationRequest;
use janiform_foundations::cancellation::CancellationSource;
use janiform_foundations::cancellation::CancellationToken;

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

/// Run the Janiform CLI from a complete argument vector and caller-supplied
/// cancellation token, returning its stable process exit code.
///
/// The first argument is the executable name, matching `std::env::args_os`.
pub fn run_exit_code<I, T>(arguments: I, cancellation_token: CancellationToken) -> CliExitCode
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

                    return CliExitCode::CommandLineUsage;
                }

                return exit_code;
            }

            if let Err(print_error) = error.print() {
                let mut stderr = std::io::stderr().lock();

                let _ = writeln!(
                    stderr,
                    "error: failed to write CLI diagnostic: {print_error}"
                );

                return CliExitCode::CommandLineUsage;
            }

            return exit_code;
        }
    };

    let context = match CommandContext::from_global_options(&cli.global, cancellation_token) {
        Ok(context) => context,
        Err(error) => {
            let exit_code = error.exit_code();

            if let Err(write_error) = write_error_without_context(error, &cli.global) {
                let mut stderr = std::io::stderr().lock();

                let _ = writeln!(
                    stderr,
                    "error: failed to write CLI diagnostic: {write_error}"
                );

                return CliExitCode::CommandLineUsage;
            }

            return exit_code;
        }
    };

    initialize_tracing(&context);

    if context.cancellation_token().is_cancelled() {
        return write_context_error(
            CliError::Interrupted {
                source: CancellationError,
            },
            &context,
        );
    }

    match dispatch(&context, cli.command) {
        Ok(exit_code) => exit_code,
        Err(error) => write_context_error(error, &context),
    }
}

/// Install the terminal Ctrl+C policy for the native Rust binary.
///
/// The first interrupt requests cooperative cancellation. A repeated interrupt
/// applies the CLI-only force policy and exits with the conventional terminal
/// interruption code.
pub fn install_terminal_interrupt_handler(
    cancellation_source: CancellationSource,
) -> Result<(), ctrlc::Error> {
    ctrlc::set_handler(move || {
        if cancellation_source.request_cancellation() == CancellationRequest::ForceRequested {
            std::process::exit(i32::from(CliExitCode::Interrupted as u8));
        }
    })
}

/// Run the Janiform CLI from a complete argument vector.
///
/// The first argument is the executable name, matching `std::env::args_os`.
pub fn run<I, T>(arguments: I) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let arguments = arguments
        .into_iter()
        .map(Into::into)
        .collect::<Vec<OsString>>();

    let requested_json = arguments.iter().any(|argument| argument == "--json");
    let cancellation_source = CancellationSource::new();

    if let Err(error) = install_terminal_interrupt_handler(cancellation_source.shared_handle()) {
        let cli_error = CliError::InterruptHandlerInstall { source: error };

        if let Err(write_error) = write_error_for_parse_failure(cli_error, requested_json) {
            let mut stderr = std::io::stderr().lock();

            let _ = writeln!(
                stderr,
                "error: failed to write CLI diagnostic: {write_error}"
            );
        }

        return CliExitCode::CommandLineUsage.into();
    }

    run_exit_code(arguments, cancellation_source.token()).into()
}

fn write_context_error(error: CliError, context: &CommandContext) -> CliExitCode {
    let exit_code = error.exit_code();

    if let Err(write_error) = write_error(error, context) {
        let mut stderr = std::io::stderr().lock();

        let _ = writeln!(
            stderr,
            "error: failed to write CLI diagnostic: {write_error}"
        );

        return CliExitCode::CommandLineUsage;
    }

    exit_code
}

#[cfg(test)]
mod tests {
    use janiform_foundations::cancellation::CancellationSource;

    use super::run_exit_code;
    use crate::error::CliExitCode;

    #[test]
    fn pre_cancelled_token_returns_interrupted_exit_code() {
        let cancellation_source = CancellationSource::new();
        cancellation_source.cancel();

        let exit_code = run_exit_code(["janiform", "presets", "list"], cancellation_source.token());

        assert_eq!(CliExitCode::Interrupted, exit_code);
    }
}
