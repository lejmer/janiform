## ISSUE-006: Multi Regime And Multi Interval Batch Orchestrator

### Description
Implement batch orchestrator to run consistency checks across multiple parameter regimes and interval configurations in one execution workflow.

### Acceptance Criteria
- Orchestrator supports batched scenario execution with deterministic ordering
- Batch outputs include scenario-level status and summary metrics
- Orchestrator can resume or rerun failed scenarios without rerunning all cases
- Execution metadata is persisted for reproducibility

### Unit tests
- Batched execution flow and result collation and partial-rerun behavior and scenario filtering
- Deterministic batch-order behavior

### Notes/Constraints
- Keep orchestration logic decoupled from metric internals
- Bound batch defaults to maintain practical runtime
