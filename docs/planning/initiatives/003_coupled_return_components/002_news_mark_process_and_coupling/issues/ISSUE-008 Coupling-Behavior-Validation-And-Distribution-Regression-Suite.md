## ISSUE-008: Coupling Behavior Validation And Distribution Regression Suite

### Description
Build a validation and regression suite for coupled news primitives, covering mark-distribution sanity and shared-mark consistency across `J_t` and `G_t` wiring.

### Acceptance Criteria
- Validation scenarios cover deterministic coupling primitives and mark-distribution behavior.
- Distribution regression checks exist for severity and base-shock mark samplers.
- Coupling regressions verify shared-mark consistency across `J_t` and `G_t` paths.
- Scope explicitly excludes end-to-end open and closed leakage suites, which are owned by Project 004 in this Initiative.
- Failure diagnostics include seed, parameter set, and session-state context.

### Unit tests
- Regression tests for shared-mark coupling behavior across fixed seeds.
- Distribution sanity tests for Gamma and double-exponential mark outputs.
- Diagnostics tests ensuring required metadata is emitted on failures.

### Notes/Constraints
- Keep stochastic checks tolerance-based to reduce flakiness.
- Separate strict deterministic coupling checks from statistical distribution checks.
