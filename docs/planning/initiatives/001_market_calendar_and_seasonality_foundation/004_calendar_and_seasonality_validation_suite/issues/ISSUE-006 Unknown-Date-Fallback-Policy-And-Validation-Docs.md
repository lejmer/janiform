## ISSUE-006: Unknown Date Fallback Policy And Validation Docs

### Description
Document and encode fallback behavior for unknown dates and incomplete fixture coverage so validation outcomes remain explicit and auditable.

### Acceptance Criteria
- A fallback policy exists for unknown dates and unsupported exchange calendar segments.
- Validation docs describe expected behavior, failure modes, and triage steps.
- Policy is referenced by both Rust and Python validation entrypoints.
- Policy examples are included for at least one unknown-date and one incomplete-fixture scenario.

### Unit tests
- Tests covering unknown-date behavior under the defined fallback policy.
- Tests for incomplete fixture sets with expected failure or fallback outcomes.
- Documentation lint checks verifying required policy sections are present.

### Notes/Constraints
- Keep fallback policy deterministic and minimal for v0.1.
- Ensure policy does not silently mask missing critical fixture data.
