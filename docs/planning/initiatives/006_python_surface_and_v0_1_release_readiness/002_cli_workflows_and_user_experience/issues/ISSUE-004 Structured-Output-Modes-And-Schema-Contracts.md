## ISSUE-004: Structured Output Modes And Schema Contracts

### Description
Implement structured output modes for CLI commands so workflows can integrate reliably with automation pipelines.

### Acceptance Criteria
- Output mode selection is consistent across commands
- Output schema and metadata are machine-readable and documented

### Unit tests
- Structured output schema validation per command
- Deterministic field ordering or schema compliance
- Compatibility of output mode flags across workflows

### Notes/Constraints
- Keep human-readable and machine-readable outputs clearly separated
- Avoid including unstable internal fields in structured output
