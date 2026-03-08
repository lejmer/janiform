## Structured Output Modes And Schema Contracts

### Description
Implement structured output modes for CLI commands so workflows can integrate reliably with automation pipelines, with output contracts centered on user-facing workflow metadata rather than internal-only orchestration payloads.

### Acceptance Criteria
- Output mode selection is consistent across commands
- Output schema and metadata are machine-readable and documented
- Market-data calibration commands emit structured summaries that describe user-supplied files, normalization policy, selected workflow scope, and output artifact locations.
- Structured output for advanced staged-calibration mode is still supported, but it is clearly separated from the default user-facing calibration command family.

### Unit tests
- Structured output schema validation per command
- Deterministic field ordering or schema compliance
- Compatibility of output mode flags across workflows
- Coverage that user-facing calibration output does not leak raw internal staged execution payloads unless an explicit advanced mode requests them

### Notes/Constraints
- Keep human-readable and machine-readable outputs clearly separated
- Avoid including internal-only fields in default structured output
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
