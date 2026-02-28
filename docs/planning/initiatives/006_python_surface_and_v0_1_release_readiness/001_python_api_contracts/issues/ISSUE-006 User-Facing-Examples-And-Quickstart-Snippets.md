## ISSUE-006: User Facing Examples And Quickstart Snippets

### Description
Create user-facing examples for simulation, calibration, and pricing workflows that use the current documented public API contracts.

### Acceptance Criteria
- Example scripts cover at least one end-to-end path per primary workflow.
- Examples use typed request and response models from public namespaces.
- Calibration examples document default `execution_mode=memory` behavior and explicit override to `disk`.
- Example outputs are documented and reproducible.
- Examples are referenced by package documentation.

### Unit tests
- Smoke tests that execute example paths or validate example snippets and tests ensuring examples import only public API modules.
- Deterministic example output structure.
- Coverage of calibration example default-mode and override-mode behavior.

### Notes/Constraints
- Keep examples concise and practical for first-time users.
- Avoid reliance on internal-only modules.
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
