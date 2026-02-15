## ISSUE-005: Numerical Integration Stability And Tuning Policy

### Description
Define and implement stability controls for numerical integration, including tolerance, truncation, and convergence policies for pricing accuracy and runtime balance.

### Acceptance Criteria
- Integration policy supports configurable tolerance and truncation controls.
- Convergence and failure conditions are explicit and documented.
- Stability controls emit diagnostics when numeric quality is poor.
- Policy can be tuned without code changes through structured configuration.

### Unit tests
- Tests for convergence behavior on representative fixtures.
- Tests for failure-path behavior under intentionally poor settings.
- Tests for configuration validation and defaults.

### Notes/Constraints
- Keep policy aligned with oracle solver tolerance guidance.
- Separate numeric quality diagnostics from business-level errors.
