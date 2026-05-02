use clap::Command;
use clap::CommandFactory;

use crate::cli::Cli;

/// Build the canonical Clap command metadata for the Janiform CLI.
///
/// Generated documentation and other tooling should use this entrypoint rather
/// than re-creating the command tree or scraping terminal help output.
pub fn cli_command() -> Command {
    Cli::command()
}
