## ISSUE-006: Automation Friendly Execution And Non Interactive Mode

### Description
Ensure CLI supports non-interactive execution patterns required for scripting, scheduled jobs, and CI automation.

### Acceptance Criteria
- Primary commands can run fully non-interactively with explicit arguments.
- Command behavior is deterministic when stdin is absent.
- Environment-variable and config-file inputs are documented with precedence rules.
- Calibration commands run non-interactively with explicit `--execution-mode` and default to `disk` when omitted.
- Automation mode avoids unnecessary prompts and formatting noise.

### Unit tests
- Non-interactive command execution with explicit arguments, config and environment precedence behavior, and no prompt output in automation mode.
- Calibration mode tests for omitted `--execution-mode` default behavior and explicit override behavior.

### Notes/Constraints
- Keep automation behavior enabled by default for scriptable commands.
- Avoid hidden dependency on terminal-specific features.
