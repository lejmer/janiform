## ISSUE-006: Calibration Facing Validation Report And Error Taxonomy

### Description
Create a unified validation report that aggregates constraint and stability checks into optimizer-friendly outputs and human-readable diagnostics.

### Acceptance Criteria
- Validation API returns structured pass or fail status with per-check details.
- Error taxonomy is consistent across positivity, structural, and stability checks.
- Report format supports optimizer loops, CLI display, and artifact persistence.
- Documentation includes examples of common failure patterns and remediation hints.

### Unit tests
- Tests for aggregated report content on fully valid parameter sets.
- Tests for multi-failure parameter sets with deterministic ordering of failures.
- Tests for compatibility of report payload with calibration pipeline consumers.

### Notes/Constraints
- Keep report schema stable for v0.1 planning and implementation.
- Avoid leaking module-internal error types across public boundaries.
