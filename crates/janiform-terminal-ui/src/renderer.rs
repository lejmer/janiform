use std::io;

use console::Term;

use crate::style::SemanticStyle;
use crate::style::style_contract;

/// Terminal renderer with a semantic stylesheet and layout policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalRenderer {
    color_enabled: bool,
    terminal_width: Option<usize>,
}

impl TerminalRenderer {
    /// Create a renderer for the resolved stream color capability.
    pub const fn new(color_enabled: bool, terminal_width: Option<usize>) -> Self {
        Self {
            color_enabled,
            terminal_width,
        }
    }

    /// Returns whether ANSI styling is enabled for this renderer.
    pub const fn color_enabled(&self) -> bool {
        self.color_enabled
    }

    /// Returns the stream width used by layout renderers when known.
    pub const fn terminal_width(&self) -> Option<usize> {
        self.terminal_width
    }

    pub(crate) fn style_text(&self, style: SemanticStyle, text: &str) -> String {
        style_contract(style)
            .force_styling(self.color_enabled)
            .apply_to(text)
            .to_string()
    }

    /// Write terminal text to stdout.
    pub fn write_stdout(&self, text: &str) -> io::Result<()> {
        Term::stdout().write_line(text)
    }

    /// Write terminal text to stderr.
    pub fn write_stderr(&self, text: &str) -> io::Result<()> {
        Term::stderr().write_line(text)
    }

    /// Return the color capability decision for stdout, respecting `NO_COLOR`.
    pub fn stdout_color_enabled() -> bool {
        console::colors_enabled() && std::env::var_os("NO_COLOR").is_none()
    }

    /// Return the color capability decision for stderr, respecting `NO_COLOR`.
    pub fn stderr_color_enabled() -> bool {
        console::colors_enabled_stderr() && std::env::var_os("NO_COLOR").is_none()
    }
}
