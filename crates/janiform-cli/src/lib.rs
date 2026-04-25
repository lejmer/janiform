mod application;
mod cli;
mod commands;
mod dispatch;
mod error;
mod rewrite;

pub use application::run;
pub use error::CliExitCode;
