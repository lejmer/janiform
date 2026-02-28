## ISSUE-003: Consistent Argument Pattern And Flag Standards

### Description
Establish argument and flag standards so command usage is predictable across workflows and easy to script.

### Acceptance Criteria
- Common options use consistent flag names and semantics across commands.
- Option precedence and defaulting policy are documented.
- Shared argument parser utilities enforce consistency.
- Calibration commands use standardized `--execution-mode` flag semantics with default `disk`.
- Help text reflects standardized usage patterns.

### Unit tests
- Precedence and defaulting rules and tests ensuring help output includes standardized option descriptions.
- Shared-flag behavior across multiple commands.
- Calibration command tests ensuring `--execution-mode` help text and parser behavior are consistent.

### Notes/Constraints
- Prefer explicit long flags for script readability.
- Avoid workflow-specific naming for shared concepts.
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
