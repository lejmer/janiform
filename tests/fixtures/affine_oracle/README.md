# Affine Oracle Solver Reference Fixture

This fixture stores deterministic one-interval reference cases for
`solve_exponential_affine_transform_with_settings(...)`.

## File

- `affine_oracle_solver_reference_fixture.json`

## Contract

- `schema_version`: `affine_oracle.solver.reference_fixture.v1`
- Fixture set id: `affine_oracle_solver_reference_fixture_v1`
- Includes per-case expected `C(Delta)`, `D(Delta)`, `transform_value`, and absolute tolerances

## Coverage

- One analytic linear case for baseline drift detection
- One nonlinear complex hourly case
- One nonlinear complex sub-hour case for `Delta` parameterization coverage

## Fixture Update Workflow

1. Make the intentional solver or equation change.
2. Run the ignored print helper to capture fresh outputs:
   `cargo test -p janiform-core --test affine_oracle_solver_reference_regression print_reference_outputs_for_fixture_update -- --ignored --nocapture`
3. Update `affine_oracle_solver_reference_fixture.json` expected outputs and tolerances as needed.
4. Run the deterministic suite:
   `cargo test -p janiform-core --test affine_oracle_solver_reference_regression`
5. If tolerances are changed, explain the reason in the PR description.
