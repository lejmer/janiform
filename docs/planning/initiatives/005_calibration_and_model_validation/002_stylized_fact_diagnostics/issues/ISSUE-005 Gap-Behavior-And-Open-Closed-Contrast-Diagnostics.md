## ISSUE-005: Gap Behavior And Open Closed Contrast Diagnostics

### Description
Implement diagnostics specifically for overnight, weekend, and open-versus-closed behavior to validate gap dynamics against observed data.

### Acceptance Criteria
- Module computes gap-specific metrics for closed-window behavior
- Diagnostics include open-versus-closed contrast summaries
- Metric outputs map to shared coupling and gap definitions used in model design
- Outputs include scenario-level metadata for reproducibility

### Unit tests
- Gap metric correctness on controlled open and closed fixtures and open-versus-closed contrast calculations
- Deterministic reporting outputs

### Notes/Constraints
- Reuse calendar and market-state fixtures from Initiative 001
- Keep gap diagnostics aligned with Initiative 003 coupling semantics
