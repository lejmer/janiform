## ISSUE-005: Asymmetry Behavior Regression Pack

### Description
Create regression scenarios that lock down expected buy and sell asymmetry behavior implied by structured `alpha` and `rho` parameterizations.

### Acceptance Criteria
- Scenario set includes symmetric baseline and asymmetric stress parameter regimes
- Regression checks detect unintended side inversion or dampening of expected asymmetry
- Scenario metadata records parameter regime intent and expected qualitative behavior
- Outputs are deterministic at the seed and fixture level

### Unit tests
- Expected sell-side clustering under asymmetric parameters, Control tests for near-symmetric behavior under symmetric parameter fixtures, and Change-detection tests for side-specific event-rate drift

### Notes/Constraints
- Keep metrics simple and robust to reduce flaky failures
- Clearly separate expected qualitative signals from strict numeric equality checks
