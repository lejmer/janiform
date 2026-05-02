//! Native Janiform command shell.
//!
//! The CLI owns command parsing, path resolution, output-mode selection, and
//! non-execution command screens. Workflow execution UX lives in
//! `janiform-workflows`.

mod application;
mod cli;
mod commands;
mod config;
mod context;
mod diagnostics;
mod dispatch;
mod error;
mod logging;
mod paths;
mod reference;
mod rewrite;

pub use application::run;
pub use context::ColorChoice;
pub use context::CommandContext;
pub use context::EventSettings;
pub use context::EventStreamFormat;
pub use context::OutputMode;
pub use error::CliError;
pub use error::CliExitCode;
pub use paths::PathResolver;
pub use reference::cli_command;
