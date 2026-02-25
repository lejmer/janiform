# Affine Oracle Solver Fixtures

This directory stores deterministic one-interval fixtures for affine-oracle solver regression and
method-comparison checks.

## File

- `affine_oracle_solver_reference_fixture.json`
- `affine_oracle_solver_method_comparison_fixture.json`

## Contract

Reference fixture:

- `schema_version`: `affine_oracle.solver.reference_fixture.v1`
- Fixture set id: `affine_oracle_solver_reference_fixture_v1`
- Includes per-case expected `C(Delta)`, `D(Delta)`, `transform_value`, and absolute tolerances

Method-comparison fixture:

- `schema_version`: `affine_oracle.method_comparison.fixture.v1`
- Fixture set id: `affine_oracle_solver_method_comparison_fixture_v1`
- Includes a versioned tolerance policy with interval-dependent bands and expected pass or fail
  comparison behavior across integrator settings

## Coverage

- Reference fixture:
  - One analytic linear case for baseline drift detection
  - One nonlinear complex hourly case
  - One nonlinear complex sub-hour case for `Delta` parameterization coverage
- Method-comparison fixture:
  - One nonlinear complex hourly comparison case
  - One nonlinear complex sub-hour comparison case
  - Baseline and alternative method checks with deterministic output-difference snapshots

## Fixture Update Workflow

Reference fixture:

1. Make the intentional solver or equation change.
2. Run the ignored print helper to capture fresh outputs:
   `cargo test -p janiform-core --test affine_oracle_solver_reference_regression print_reference_outputs_for_fixture_update -- --ignored --nocapture`
3. Update `affine_oracle_solver_reference_fixture.json` expected outputs and tolerances as needed.
4. Run the deterministic suite:
   `cargo test -p janiform-core --test affine_oracle_solver_reference_regression`
5. If tolerances are changed, explain the reason in the PR description.

Method-comparison fixture:

1. Make the intentional solver, tolerance-policy, or method-comparison change.
2. Run the ignored print helper:
   `cargo test -p janiform-core --test affine_oracle_solver_method_comparison print_method_comparison_outputs_for_fixture_update -- --ignored --nocapture`
3. Update `affine_oracle_solver_method_comparison_fixture.json` expected status, failure codes,
   and output-difference snapshots.
4. Run:
   `cargo test -p janiform-core --test affine_oracle_solver_method_comparison`
5. If policy version or tolerance bands change, update change-log metadata in the fixture.
