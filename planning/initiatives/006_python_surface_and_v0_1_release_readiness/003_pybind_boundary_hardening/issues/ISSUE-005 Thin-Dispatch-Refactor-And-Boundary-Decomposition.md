## ISSUE-005: Thin Dispatch Refactor And Boundary Decomposition

### Description
Refactor pybind entrypoints to keep boundary code thin, with explicit separation between conversion, dispatch, and error translation.

### Acceptance Criteria
- Entry points use a consistent boundary pipeline: parse, validate, dispatch, map errors
- Business logic is removed from pybind modules and delegated to core services
- Boundary helpers are reusable across exposed functions
- Refactor preserves documented behavior for the current public Python APIs

### Unit tests
- Tests that verify shared dispatch helpers are used by multiple entrypoints
- Boundary pipeline ordering and failure handling
- Integration tests for unchanged public behavior after refactor

### Notes/Constraints
- Keep refactor focused on boundary responsibilities
- Avoid drive-by changes outside pybind and related adapters
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
