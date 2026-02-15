## ISSUE-004: Metric Delta Calculator For Returns Vol And Events

### Description
Implement metric-delta calculations for key consistency statistics including returns, volatility, and event-derived summaries.

### Acceptance Criteria
- Metric engine computes coarse-versus-fine deltas for configured statistics.
- Metrics include horizon and interval context in outputs.
- Delta outputs are machine-readable and consistently named.
- Metric computation supports batch execution across parameter regimes.

### Unit tests
- Tests for delta-calculation correctness on controlled fixtures.
- Tests for missing-metric handling and clear failure output.
- Regression tests for deterministic metric output ordering.

### Notes/Constraints
- Keep metric names aligned with stylized-fact diagnostics catalog.
- Separate metric computation from threshold decision logic.
