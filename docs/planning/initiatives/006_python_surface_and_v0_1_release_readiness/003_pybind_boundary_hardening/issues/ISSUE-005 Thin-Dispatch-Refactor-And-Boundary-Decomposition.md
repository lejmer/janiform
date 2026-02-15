## ISSUE-005: Thin Dispatch Refactor And Boundary Decomposition

### Description
Refactor pybind entrypoints to keep boundary code thin, with explicit separation between conversion, dispatch, and error translation.

### Acceptance Criteria
- Entry points use a consistent boundary pipeline: parse, validate, dispatch, map errors.
- Business logic is removed from pybind modules and delegated to core services.
- Boundary helpers are reusable across exposed functions.
- Refactor preserves public Python behavior for stable APIs.

### Unit tests
- Integration tests for unchanged public behavior after refactor.
- Tests that verify shared dispatch helpers are used by multiple entrypoints.
- Regression tests for boundary pipeline ordering and failure handling.

### Notes/Constraints
- Keep refactor focused on boundary responsibilities.
- Avoid drive-by changes outside pybind and related adapters.
