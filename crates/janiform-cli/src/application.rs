use std::ffi::OsString;
use std::process::ExitCode;

use clap::Parser;

use crate::cli::Cli;
use crate::dispatch::dispatch;
use crate::error::CliExitCode;
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

    let cli = match Cli::try_parse_from(rewritten_arguments) {
        Ok(cli) => cli,
        Err(error) => {
            let exit_code = CliExitCode::from_clap_exit_code(error.exit_code());
            if let Err(print_error) = error.print() {
                eprintln!("error: failed to write CLI diagnostic: {print_error}");
                return CliExitCode::CommandLineUsage.into();
            }

            return exit_code.into();
        }
    };

    match dispatch(cli) {
        Ok(()) => CliExitCode::Success.into(),
        Err(error) => {
            eprintln!("{error}");
            error.exit_code().into()
        }
    }
}
