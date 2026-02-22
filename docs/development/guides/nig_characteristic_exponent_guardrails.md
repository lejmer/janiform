# NIG Characteristic Exponent Guardrails

This guide defines the machine-readable numerical stability report emitted by
`NigParameters::characteristic_exponent_with_guardrails`.

## Report schema

- `schema_version`: `nig.characteristic_exponent.guardrail_report.v1`
- `determinism_policy`: `stable_check_order_with_explicit_threshold_metadata.v1`
- `stability_policy`: `gamma_minus_sqrt_term_uses_rationalized_difference_when_cancellation_risk_is_detected.v1`
- `status`: `pass`, `warning`, or `fail`
- `argument`:
  - `real`
  - `imaginary`
- `alpha_beta_margin`: observed `alpha - abs(beta)`
- `alpha_beta_margin_warning_threshold`: deterministic warning threshold used for boundary proximity checks
- `radicand_norm`: observed norm of `alpha^2 - (beta + i u)^2`
- `radicand_warning_threshold`: deterministic warning threshold used for transform singularity proximity checks
- `cancellation_ratio`: observed ratio used to detect subtraction cancellation in
  `gamma - sqrt(alpha^2 - (beta + i u)^2)`
- `cancellation_warning_threshold`: deterministic cancellation warning threshold
- `used_rationalized_difference`: whether the rationalized difference formula was used
- `events[]`, each event includes:
  - `code`
  - `severity`
  - `metric`
  - `observed_value`
  - `threshold_value`
  - `detail`
- `value`: evaluated exponent value when `status` is not `fail`, else `null`

## Event codes

- `non_finite_input`
- `alpha_beta_boundary_proximity`
- `alpha_beta_domain_violation`
- `transform_radicand_proximity`
- `cancellation_sensitive_difference`
- `rationalized_difference_fallback`
- `non_finite_evaluation_output`

## Stability handling

- The implementation detects near-boundary `alpha` and `beta` configurations.
- The implementation detects near-singular transform radicands.
- When cancellation sensitivity is detected in `gamma - sqrt_term`, the evaluator
  switches to a rationalized difference formula.
- If rationalized division is unstable, the evaluator falls back to direct
  subtraction and emits a warning event.
- Domain violations and non-finite inputs are explicit failures and return
  `status = fail` with `value = null`.
