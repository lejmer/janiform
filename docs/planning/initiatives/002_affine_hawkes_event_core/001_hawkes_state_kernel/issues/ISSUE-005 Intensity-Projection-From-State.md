## ISSUE-005: Intensity Projection From State

### Description
Implement deterministic mapping from Hawkes state to de-seasonalized and seasonality-scaled buy and sell intensities.

### Acceptance Criteria
- Projection computes `tilde_lambda_plus = lambda_bar + Y++ + Y+-` and `tilde_lambda_minus = lambda_bar + Y-+ + Y--`.
- Seasonality scaling produces `lambda_plus = s(t) * tilde_lambda_plus` and `lambda_minus = s(t) * tilde_lambda_minus`.
- Baseline symmetry uses one shared `lambda_bar` parameter for both sides.
- Projection API is shared by fine-scale simulation and coarse-time sufficient-statistics code.

### Unit tests
- Tests for correct intensity values under controlled state and seasonality inputs.
- Tests for symmetry with identical state components across sides.
- Edge tests for `s(t)` values used in hourly piecewise approximation.

### Notes/Constraints
- Keep projection logic separate from transition operators.
- Reuse calendar-seasonality interfaces rather than duplicating factor retrieval.
