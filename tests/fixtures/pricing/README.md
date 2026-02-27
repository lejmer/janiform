# Vanilla Option Pricing Benchmark Fixture

This directory stores the benchmark and sanity-regression fixture for the characteristic-function
vanilla option pricer.

## File

- `vanilla_option_pricing_benchmark_fixture.json`

## Contract

Top-level fields:

- `schema_version`: `pricing.vanilla_option.benchmark_fixture.v1`
- `fixture_set_id`: `vanilla_option_pricing_benchmark_fixture_v1`
- `fixture_set_version`: semantic fixture version
- `determinism_metadata`: fixture ordering and content-hash policies
- `benchmark_result_schema_metadata`: machine-readable result-schema required fields
- `tolerance_policy`: absolute and relative price tolerances and put-call parity tolerance
- `sanity_guardrails`: no-arbitrage and monotonicity tolerances
- `canonical_regression_metadata`: required tags, stable fixture ids, monotonic sequences, and
  parity pairs
- `cases`: baseline reference and pathology scenarios

Each case includes:

- `fixture_id`: stable case identifier
- `payoff_type`: `call` or `put`
- `market_inputs`: baseline vanilla market inputs
- `volatility`: Black-Scholes benchmark volatility used for the characteristic function
- `engine_settings`: explicit Fourier contour, quadrature, and stability controls
- `expected_outcome`: either
  - `success` with expected price and numeric-quality expectations, or
  - `numeric_quality_failure` for integration pathology probes
- `tags`: coverage labels
- `description`: triage context

## Coverage

- Reference benchmark scenarios for call and put prices across a strike ladder.
- Tolerance-based regression checks against expected baseline prices.
- Sanity guardrails for no-arbitrage bounds, strike monotonicity, and put-call parity.
- Explicit integration-pathology case that must trigger a numeric-quality failure.

## Update Workflow

1. Make the intentional pricer or integration-policy change.
2. Run:
   `cargo test -p janiform-core --test pricing_benchmark_validation_regression -- --nocapture`
3. Update expected fixture outputs, tolerance policy, and changelog entry as needed.
4. Keep existing `fixture_id` values stable. Add new cases instead of renaming existing ones.
5. Re-run the test command and confirm all checks pass.
