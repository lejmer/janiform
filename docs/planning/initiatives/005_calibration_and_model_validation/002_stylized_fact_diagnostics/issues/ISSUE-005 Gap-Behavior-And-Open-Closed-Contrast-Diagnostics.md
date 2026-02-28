## ISSUE-005: Gap Behavior And Open Closed Contrast Diagnostics

### Description
Implement diagnostics for overnight, weekend, and open-versus-closed behavior to validate direct open-window jump dynamics and closed-window reopening-gap dynamics against observed data.

### Acceptance Criteria
- Module computes direct-jump metrics for open-window behavior and gap metrics for closed-window behavior
- Diagnostics include open-versus-closed contrast summaries
- Metric outputs map to shared activity-coupling, open-jump, and closed-gap definitions used in model design
- Outputs include scenario-level metadata for reproducibility

### Unit tests
- Gap metric correctness on controlled open and closed fixtures and open-versus-closed contrast calculations
- Deterministic reporting outputs

### Notes/Constraints
- Reuse calendar and market-state fixtures from Initiative 001
- Keep diagnostics aligned with Initiative 003 coupling semantics and explicit open-jump versus closed-gap component naming
- Pre-v0.1 policy applies: breaking schema changes are allowed when they improve model correctness and consistency.
