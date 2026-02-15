## ISSUE-005: Constrained Optimizer Adapter And Objective Composition

### Description
Build optimizer adapter layer and objective-composition engine so stage modules can run constrained optimization with reproducible configuration and diagnostics.

### Acceptance Criteria
- Adapter supports constrained optimization invocation with explicit bounds and penalties.
- Objective composition supports multiple weighted terms per stage.
- Adapter outputs include optimizer status, iterations, and convergence metadata.
- Configuration schema captures all optimizer knobs used in runs.

### Unit tests
- Tests for objective composition correctness on controlled inputs.
- Tests for optimizer-config validation and failure paths.
- Tests for deterministic adapter behavior under fixed random seeds.

### Notes/Constraints
- Keep optimizer interface generic enough for backend substitution.
- Separate objective logic from stage orchestration logic.
