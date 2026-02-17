## ISSUE-005: Constrained Optimizer Adapter And Objective Composition

### Description
Build optimizer adapter layer and objective-composition engine so stage modules can run constrained optimization with reproducible configuration and diagnostics.

### Acceptance Criteria
- Adapter supports constrained optimization invocation with explicit bounds and penalties
- Objective composition supports multiple weighted terms per stage
- Output schema and metadata are machine-readable and documented

### Unit tests
- Objective composition correctness on controlled inputs
- Optimizer-config validation and failure paths
- Deterministic adapter behavior under fixed random seeds

### Notes/Constraints
- Keep optimizer interface generic enough for backend substitution
- Separate objective logic from stage orchestration logic
