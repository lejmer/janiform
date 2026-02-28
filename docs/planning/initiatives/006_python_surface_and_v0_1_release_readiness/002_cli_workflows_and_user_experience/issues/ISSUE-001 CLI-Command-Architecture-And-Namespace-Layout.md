## ISSUE-001: CLI Command Architecture And Namespace Layout

### Description
Define the top-level CLI command architecture and namespace layout for simulation, calibration, and pricing workflows.

### Acceptance Criteria
- CLI command tree is documented with explicit top-level and subcommand names
- Command naming aligns with public Python API terminology
- Namespace layout separates primary commands from internal developer-only utilities
- Help output reflects the finalized command hierarchy

### Unit tests
- Command discovery and expected subcommand registration and top-level and subcommand help output presence

### Notes/Constraints
- Keep names short, explicit, and automation-friendly
- Avoid command aliases that introduce ambiguity in scripts
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
