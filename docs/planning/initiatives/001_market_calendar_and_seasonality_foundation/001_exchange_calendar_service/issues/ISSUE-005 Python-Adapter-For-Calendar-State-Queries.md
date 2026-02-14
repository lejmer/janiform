## ISSUE-005: Python Adapter For Calendar State Queries

### Description
Expose the calendar resolver through the Python package so higher-level workflows can query `open` and `closed` state without duplicating logic outside Rust core.

### Acceptance Criteria
- Python API exposes a typed function for calendar state query by exchange and timestamp.
- Adapter maps Rust domain and error types into stable Python-facing types and exceptions.
- Public Python docs include minimal usage examples for the calendar query path.
- Adapter behavior is consistent with Rust resolver outputs for the same inputs.

### Unit tests
- Python tests for successful queries across normal and special session dates.
- Python tests for missing exchange IDs and invalid timestamp inputs.
- Cross-layer parity tests comparing Python and core resolver behavior.

### Notes/Constraints
- Keep PyO3 layer thin and conversion-focused.
- Do not duplicate resolver logic in Python.
