## ISSUE-006: Gap Leakage Regression Harness And CI Gates

### Description
Create an automated regression harness that detects any gap-component leakage into open windows and enforces this behavior in CI checks.

### Acceptance Criteria
- Harness executes scenario suite and flags open-window gap leakage as failures.
- Regression results are deterministic for fixed scenario sets and seeds.
- CI gate integrates leakage checks as required validation for this project.
- Failure output includes scenario IDs and leakage magnitude details.

### Unit tests
- Harness tests for known passing and failing scenario fixtures.
- Determinism tests for repeated harness runs.
- CI integration smoke test for regression command invocation.

### Notes/Constraints
- Keep harness runtime bounded for frequent execution.
- Prioritize clear diagnostics over verbose logs.
