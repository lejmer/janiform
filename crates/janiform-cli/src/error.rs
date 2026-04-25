use std::fmt::Display;
use std::fmt::Formatter;
use std::process::ExitCode;

/// Stable CLI process exit codes.
///
/// `UnimplementedCommand` is reserved for recognized command surfaces that parse
/// successfully but have not yet been wired to their Rust runtime handler.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum CliExitCode {
    Success = 0,
    CommandLineUsage = 2,
    UnimplementedCommand = 78,
}

impl CliExitCode {
    pub(crate) fn from_clap_exit_code(exit_code: i32) -> Self {
        match exit_code {
            0 => Self::Success,
            _ => Self::CommandLineUsage,
        }
    }
}

impl From<CliExitCode> for ExitCode {
    fn from(value: CliExitCode) -> Self {
        Self::from(value as u8)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum CliError {
    UnimplementedCommand { command_path: &'static str },
}

impl CliError {
    pub(crate) fn unimplemented(command_path: &'static str) -> Self {
        Self::UnimplementedCommand { command_path }
    }

    pub(crate) fn exit_code(&self) -> CliExitCode {
        match self {
            Self::UnimplementedCommand { .. } => CliExitCode::UnimplementedCommand,
        }
    }
}

impl Display for CliError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnimplementedCommand { command_path } => write!(
                formatter,
                "error: command `janiform {command_path}` is recognized but is not implemented in the Rust CLI yet"
            ),
        }
    }
}
