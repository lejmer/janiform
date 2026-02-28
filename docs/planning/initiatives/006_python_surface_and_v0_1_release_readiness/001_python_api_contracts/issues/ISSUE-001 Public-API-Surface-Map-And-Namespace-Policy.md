## ISSUE-001: Public API Surface Map And Namespace Policy

### Description
Define the Python API surface map for simulation, calibration, and pricing workflows, including namespace boundaries between public and internal modules.

### Acceptance Criteria
- Public API entrypoints are explicitly listed and versioned for the current pre-v0.1 milestone
- Namespace policy distinguishes public modules from private internals
- API surface map includes workflow ownership and contract-evolution rules
- Policy document is linked from package documentation

### Unit tests
- Tests that public symbols are exported from expected modules and tests that private modules are not accidentally exposed

### Notes/Constraints
- Keep public surface minimal and task-oriented
- Avoid leaking internal implementation types through public imports
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
