## ISSUE-006: Release Checklist With Owners And Exit Criteria

### Description
Create the v0.1 release checklist with explicit owners, objective exit criteria, and evidence links for each gate.

### Acceptance Criteria
- Checklist includes all required technical, documentation, and packaging gates
- Each checklist item has owner, status, and objective completion criteria
- Evidence links are captured for completed checklist items
- Checklist template supports reuse for future releases

### Unit tests
- Validation checks for owner and exit-criteria completeness and status-transition rules in checklist artifacts
- Checklist-schema validation for required fields

### Notes/Constraints
- Keep checklist objective and audit-friendly
- Avoid subjective completion criteria
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
