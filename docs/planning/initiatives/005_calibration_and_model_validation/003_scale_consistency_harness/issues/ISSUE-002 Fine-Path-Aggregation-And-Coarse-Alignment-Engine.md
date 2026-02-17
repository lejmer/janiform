## ISSUE-002: Fine Path Aggregation And Coarse Alignment Engine

### Description
Implement aggregation engine that maps fine-scale simulation outputs onto coarse intervals for direct consistency comparison.

### Acceptance Criteria
- Aggregator aligns fine-path outputs to coarse interval boundaries deterministically
- Aggregation policy handles boundary and partial-window behavior explicitly
- Engine supports multiple interval pairs across configured `Delta` values
- Output includes alignment metadata for auditability

### Unit tests
- Correct aggregation across exact and non-exact boundary cases and invalid interval-pair handling
- Deterministic alignment behavior on repeated runs

### Notes/Constraints
- Reuse shared interval-grid utilities
- Keep aggregation logic separate from metric comparison code
