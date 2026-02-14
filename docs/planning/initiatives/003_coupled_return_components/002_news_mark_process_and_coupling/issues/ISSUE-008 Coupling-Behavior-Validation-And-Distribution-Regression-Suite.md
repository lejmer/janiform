## ISSUE-008: Coupling Behavior Validation And Distribution Regression Suite

### Description
Build a validation and regression suite for coupled news behavior, covering mark-distribution sanity and open or closed coupling semantics.

### Acceptance Criteria
- Validation scenarios cover open-only, closed-only, and mixed session event streams.
- Distribution regression checks exist for severity and base-shock mark samplers.
- Coupling regressions verify shared-mark consistency across `J_t` and `G_t` paths.
- Failure diagnostics include seed, parameter set, and session-state context.

### Unit tests
- Regression tests for open and closed coupling behavior across fixed seeds.
- Distribution sanity tests for Gamma and double-exponential mark outputs.
- Diagnostics tests ensuring required metadata is emitted on failures.

### Notes/Constraints
- Keep stochastic checks tolerance-based to reduce flakiness.
- Separate strict deterministic coupling checks from statistical distribution checks.
