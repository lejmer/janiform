# janiform-cli

Native Rust command-line shell for Janiform.

This crate owns argument parsing, command routing, and CLI boundary diagnostics. Core numerical behavior lives in the domain crates and is wired into this shell through explicit command handlers as the CLI migration progresses.
