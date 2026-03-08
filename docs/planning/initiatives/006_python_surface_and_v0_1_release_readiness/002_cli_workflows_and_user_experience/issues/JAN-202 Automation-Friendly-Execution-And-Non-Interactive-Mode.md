## Automation Friendly Execution And Non Interactive Mode

### Description
Ensure CLI supports non-interactive execution patterns required for scripting, scheduled jobs, and CI automation, including realistic market-data calibration runs from file paths and configuration documents.

### Acceptance Criteria
- Primary commands can run fully non-interactively with explicit arguments.
- Command behavior is deterministic when stdin is absent.
- Environment-variable and config-file inputs are documented with precedence rules.
- Calibration commands run non-interactively with explicit `--execution-mode` and default to `disk` when omitted.
- Automation mode avoids unnecessary prompts and formatting noise.
- Market-data calibration commands can be driven end to end from explicit file paths and configuration flags without requiring intermediate hand-authored staged execution JSON.

### Unit tests
- Non-interactive command execution with explicit arguments, config and environment precedence behavior, and no prompt output in automation mode.
- Calibration mode tests for omitted `--execution-mode` default behavior and explicit override behavior.
- Automation-path tests for file-based calibration invocations with explicit tick-data, session, and known-news inputs.

### Notes/Constraints
- Keep automation behavior enabled by default for scriptable commands.
- Avoid hidden dependency on terminal-specific features.
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
