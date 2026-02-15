## ISSUE-004: Structured Output Modes And Schema Contracts

### Description
Implement structured output modes for CLI commands so workflows can integrate reliably with automation pipelines.

### Acceptance Criteria
- CLI supports machine-readable output mode for primary workflow commands.
- Output schemas are documented and versioned for v0.1 stability.
- Structured output includes run status, key result fields, and error metadata when applicable.
- Output mode selection is consistent across commands.

### Unit tests
- Tests for structured output schema validation per command.
- Tests for deterministic field ordering or schema compliance.
- Tests for compatibility of output mode flags across workflows.

### Notes/Constraints
- Keep human-readable and machine-readable outputs clearly separated.
- Avoid including unstable internal fields in structured output.
