## ISSUE-004: Metric Delta Calculator For Returns Vol And Events

### Description
Implement metric-delta calculations for key consistency statistics including returns, volatility, and event-derived summaries.

### Acceptance Criteria
- Metric engine computes coarse-versus-fine deltas for configured statistics
- Metrics include horizon and interval context in outputs
- Delta outputs are machine-readable and consistently named
- Metric computation supports batch execution across parameter regimes

### Unit tests
- Delta-calculation correctness on controlled fixtures
- Missing-metric handling and clear failure output
- Deterministic metric output ordering

### Notes/Constraints
- Keep metric names aligned with stylized-fact diagnostics catalog
- Separate metric computation from threshold decision logic
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
