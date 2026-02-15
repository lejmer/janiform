## ISSUE-006: Automation Friendly Execution And Non Interactive Mode

### Description
Ensure CLI supports non-interactive execution patterns required for scripting, scheduled jobs, and CI automation.

### Acceptance Criteria
- Primary commands can run fully non-interactively with explicit arguments.
- Command behavior is deterministic when stdin is absent.
- Environment-variable and config-file inputs are documented with precedence rules.
- Automation mode avoids unnecessary prompts and formatting noise.

### Unit tests
- Tests for non-interactive command execution with explicit arguments.
- Tests for config and environment precedence behavior.
- Tests confirming no prompt output in automation mode.

### Notes/Constraints
- Keep automation behavior enabled by default for scriptable commands.
- Avoid hidden dependency on terminal-specific features.
