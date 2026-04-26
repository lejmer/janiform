/// Color policy requested by the CLI boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorMode {
    /// Resolve color from terminal capability and environment.
    Auto,
    /// Emit ANSI styling for terminal output.
    Always,
    /// Never emit ANSI styling.
    Never,
}
