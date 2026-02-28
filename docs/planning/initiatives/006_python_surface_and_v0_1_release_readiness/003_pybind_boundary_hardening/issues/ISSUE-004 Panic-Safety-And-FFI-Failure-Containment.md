## ISSUE-004: Panic Safety And FFI Failure Containment

### Description
Implement panic-safety protections so unexpected Rust failures are contained and converted into controlled Python exceptions.

### Acceptance Criteria
- Boundary calls guard against panics crossing the FFI boundary
- Panic paths convert to deterministic Python internal-error exceptions
- Failure containment behavior is consistent across all exposed entrypoints
- Panic-safety policy is documented with boundary invariants

### Unit tests
- Tests that simulate panic scenarios and verify controlled exception output
- Tests ensuring no process-aborting behavior in guarded boundary paths
- Consistent panic-conversion behavior across entrypoints

### Notes/Constraints
- Keep panic guards lightweight and centralized
- Preserve normal-path performance by avoiding unnecessary overhead
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
