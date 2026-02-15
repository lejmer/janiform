## ISSUE-006: CI Tiered Execution Strategy For Regression And Benchmarks

### Description
Define tiered CI strategy that separates fast deterministic regression checks from heavier performance benchmark jobs.

### Acceptance Criteria
- CI includes a fast tier for deterministic correctness and tolerance regressions.
- CI includes a scheduled or gated tier for heavier benchmark workloads.
- Failure outputs include links to scenario and baseline artifacts.
- CI documentation explains expected runtime and trigger policy per tier.

### Unit tests
- Tests for CI config linting and required workflow fields.
- Smoke tests for fast-tier command execution and output parsing.
- Tests for benchmark-tier artifact publishing hooks.

### Notes/Constraints
- Keep required merge checks fast and reliable.
- Avoid making heavy benchmark jobs mandatory on every commit.
