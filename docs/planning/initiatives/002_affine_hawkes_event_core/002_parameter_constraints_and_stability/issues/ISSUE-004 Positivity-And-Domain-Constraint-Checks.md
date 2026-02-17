## ISSUE-004: Positivity And Domain Constraint Checks

### Description
Implement domain checks for baseline and decay positivity and element-wise non-negativity for excitation and loading parameters.

### Acceptance Criteria
- Validator enforces `lambda_bar > 0` and `beta > 0`
- Validator enforces `alpha_ij >= 0` and `rho_ij >= 0` across all components
- Error responses identify failing field names and failing constraints
- Validation can run as a standalone pre-check before expensive calibration steps

### Unit tests
- Passing configurations at interior and boundary-adjacent values, failing configurations with negative and zero-invalid fields, and Tests verifying exact error payload structure for failed checks

### Notes/Constraints
- Keep validation pure and side-effect free
- Do not rely on runtime panics for constraint enforcement
