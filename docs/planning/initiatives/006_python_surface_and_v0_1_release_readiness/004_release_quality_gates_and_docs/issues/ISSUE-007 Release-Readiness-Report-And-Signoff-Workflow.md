## ISSUE-007: Release Readiness Report And Signoff Workflow

### Description
Implement final release-readiness reporting and signoff workflow that summarizes gate outcomes and remaining blockers before v0.1 publication.

### Acceptance Criteria
- Report aggregates CI gate status, docs completeness, and checklist progress.
- Signoff workflow defines required approvers and blocking conditions.
- Report format supports machine-readable and human-readable views.
- Workflow includes explicit rollback or defer path when gates fail.

### Unit tests
- Tests for readiness-report schema and required sections.
- Tests for signoff-state transitions and blocking behavior.
- Regression tests for deterministic report generation from fixed inputs.

### Notes/Constraints
- Keep signoff workflow lightweight but enforceable.
- Ensure blockers are surfaced clearly and early.
