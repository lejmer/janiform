# Hawkes Parameter Validation Report

This guide defines the v0.1 report contract for Hawkes kernel parameter validation used by calibration and simulation setup paths.

## Report schema

- `schema_version`: `validation.hawkes.kernel_parameter_report.v1`
- `determinism_policy`: `stable_check_order_and_aggregated_failures.v1`
- `status`: `pass` or `fail`
- `summary`:
  - `total_check_count`
  - `passed_check_count`
  - `failed_check_count`
- `derived_alpha`: derived canonical `alpha` loadings when structured derivation is valid
- `derived_rho`: derived canonical `rho` loadings when structured derivation is valid
- `checks`: ordered per-check diagnostics for positivity, structural, and stability checks
- `violations`: ordered failed checks projected to `field`, `constraint`, and `value`

`checks` are deterministic in order and can be consumed directly by optimizer loops, CLI output formatting, and persisted run artifacts.

## Error taxonomy

The `constraint` field is stable across validation surfaces:

- `finite_and_strictly_positive`
- `finite_and_non_negative`
- `greater_than_or_equal_to_comparison_field`
- `greater_than_or_equal_to_threshold`
- `spectral_radius_strictly_below_one`

Each check also carries a `category`:

- `positivity`
- `structural`
- `stability`

## Common failure patterns and remediation hints

1. Invalid positivity domains
   - Pattern: `lambda_bar <= 0`, `beta <= 0`, negative `alpha` or `rho` components.
   - Typical remediation: clamp candidate domains in optimizer transforms and enforce positive reparameterization.
2. Structured asymmetry violations
   - Pattern: derived `alpha.minus_minus < alpha.plus_plus` or `rho.minus_plus < rho.plus_minus`.
   - Typical remediation: increase `delta_self` or `delta_cross` terms and keep them non-negative.
3. Self-dominance policy violations
   - Pattern: `rho.target_*_self_to_cross_ratio` below configured policy threshold.
   - Typical remediation: increase self loadings or decrease cross loadings, or relax policy threshold when justified.
4. Spectral-radius instability
   - Pattern: `alpha_over_beta.spectral_radius >= 1 - numeric_tolerance`.
   - Typical remediation: reduce endogenous excitation magnitudes or increase `beta`.

## Usage notes

- Use canonical precheck for already-derived `alpha` and `rho`.
- Use structured precheck when calibration parameter vectors are in base-plus-delta form.
- Prefer `checks` for rich diagnostics and `violations` for simple fail-fast branching.
