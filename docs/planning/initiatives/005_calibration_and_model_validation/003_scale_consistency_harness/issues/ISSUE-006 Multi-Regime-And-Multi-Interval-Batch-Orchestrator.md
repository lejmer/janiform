## ISSUE-006: Multi Regime And Multi Interval Batch Orchestrator

### Description
Implement batch orchestrator to run consistency checks across multiple parameter regimes and interval configurations in one execution workflow.

### Acceptance Criteria
- Orchestrator supports batched scenario execution with deterministic ordering.
- Batch outputs include scenario-level status and summary metrics.
- Orchestrator can resume or rerun failed scenarios without rerunning all cases.
- Resume behavior is mode-aware: same-process resume in `memory` mode and persisted resume in `disk` mode.
- Execution metadata is always captured in run results and optionally persisted for reproducibility in `disk` mode.

### Unit tests
- Batched execution flow and result collation and partial-rerun behavior and scenario filtering.
- Deterministic batch-order behavior.
- Mode-aware resume behavior and mode-incompatible option failures.

### Notes/Constraints
- Keep orchestration logic decoupled from metric internals.
- Bound batch defaults to maintain practical runtime.
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
