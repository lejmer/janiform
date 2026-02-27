# Return Decomposition API Contract

This guide defines the canonical return-decomposition contract shared by
pricing, simulation, and sufficient-statistics risk generation in
`janiform-core`.

## Domain model

The shared model is `return_decomposition::ReturnDecomposition`.

It uses explicit fields that map directly to the model equation
\f$X = L^+(A^+) - L^-(A^-) + G\f$:

- `a_plus`: integrated buy intensity \f$A^+\f$
- `a_minus`: integrated sell intensity \f$A^-\f$
- `gap`: closed-market gap return contribution \f$G\f$

## Units and validation

- `a_plus` and `a_minus` are expected event counts in business time for the
  target interval and must be finite and non-negative.
- `gap` is a log-return increment and must be finite.

Validation is enforced by `ReturnDecomposition::try_new` and on deserialization.
Unknown fields and incomplete payloads are rejected.

Policy constants:

- `RETURN_DECOMPOSITION_INTERFACE_POLICY`
- `RETURN_DECOMPOSITION_UNITS_POLICY`
- `RETURN_DECOMPOSITION_EDGE_MODE_POLICY`
- `RETURN_DECOMPOSITION_INTERFACE_EVOLUTION_POLICY`

## Edge modes for debug and test workflows

Explicit opt-in component-disable modes are exposed through
`return_decomposition::ReturnDecompositionEdgeMode`:

- `disabled`: canonical production behavior
- `buy_only`: disables sell and gap components
- `sell_only`: disables buy and gap components
- `gap_only`: disables buy and sell components

`ReturnDecomposition::with_edge_mode` applies deterministic zeroing so disabled
components contribute exactly zero.

## Shared composition interface

The decomposition-to-driver mapping lives in `return_decomposition` and is the
single source for both engines:

- `ReturnDecomposition::compose_nig_trading_time_exponent`
- `ReturnDecomposition::compose_nig_trading_time_exponent_with_edge_mode`
- `ReturnDecomposition::sample_nig_trading_time_increment`
- `ReturnDecomposition::sample_nig_trading_time_increment_with_edge_mode`

These methods map `a_plus` and `a_minus` to the NIG buy and sell drivers after
optional edge-mode zeroing.

## Engine reuse

Engine-facing shared entrypoints are consumer-agnostic:

- `pricing::compose_nig_trading_time_exponent`
- `pricing::compose_nig_trading_time_exponent_with_edge_mode`
- `simulation::sample_nig_trading_time_increment`
- `simulation::sample_nig_trading_time_increment_with_edge_mode`
- `risk::generate_interval_return_from_sampled_statistics`
- `return_decomposition::compose_nig_trading_time_exponent_with_edge_mode`
- `return_decomposition::sample_nig_trading_time_increment_with_edge_mode`
- `pricing::compose_total_return_characteristic_function`
- `pricing::compose_total_return_characteristic_function_with_edge_mode`
- `pricing::compose_total_return_characteristic_function_from_oracle_output`

Public API impact and migration notes:

- JAN-105 intentionally removes pre-release `*_for_*` compatibility aliases in
  `pricing` and `simulation`.
- In-repo call sites now use the canonical names listed above.

## Integration call flow map

Pricing call flow:

1. Shared entrypoint receives `ReturnDecomposition`.
2. Shared decomposition mapping runs through
   `return_decomposition::compose_nig_trading_time_exponent_with_edge_mode`.
3. Full CF assembly multiplies `exp(weighted_trading_exponent)` and gap factor.

Oracle-output pricing call flow:

1. Shared entrypoint receives validated `ExponentialAffineTransformOutput`.
2. Shared entrypoint uses `output.transform_value` as the trading-time characteristic factor.
3. Factor composition multiplies trading-time and explicit gap factors through:
   - `pricing::compose_total_return_characteristic_function_from_oracle_output`

Simulation call flow:

1. Shared entrypoint receives `ReturnDecomposition`.
2. Shared decomposition mapping runs through
   `return_decomposition::sample_nig_trading_time_increment_with_edge_mode`.
3. Simulation uses returned buy, sell, and combined contributions as-is.

Risk interval-return call flow:

1. Shared entrypoint receives sampled `A_plus` and `A_minus` plus gap input.
2. Shared entrypoint builds `ReturnDecomposition` and samples trading-time
   increments through `ReturnDecomposition::sample_nig_trading_time_increment`.
3. Shared entrypoint applies market-state gap gating and emits total return with
   optional component breakdown.

## Interface evolution policy for v0.1

`RETURN_DECOMPOSITION_INTERFACE_EVOLUTION_POLICY` defines v0.1 stability:

- additive fields or additive APIs may be introduced without changing policy id
- existing field semantics and existing method behavior remain stable
- breaking changes must ship behind a new policy identifier

This keeps component wiring explicit and prevents anonymous tuple-style passing.

## Analytic sign fixture

Deterministic sign-validation references live in
`tests/fixtures/return_decomposition/return_decomposition_sign_analytic_fixture.json`.

Schema contract:

- `schema_version`: `return.decomposition.sign_analytic_fixture.v1`
- Fixture set id: `return_decomposition_sign_analytic_fixture_v1`
- Scenario intents: `buy_only`, `sell_only`, `combined`

Each case stores machine-readable expected outputs for:

- weighted buy and sell exponents
- combined trading-time exponent
- trading-time and full characteristic factors
- positive-`u` sell-side regression reference and minimum distance threshold

## Regression diagnostics report

Decomposition regression diagnostics in the Rust test suite emit a structured
failure report with this schema id:

- `return_decomposition.failure_diagnostic_report.v1`

Each violation entry includes:

- `scenario_id`
- `seed`
- `edge_mode`
- `check_id`
- categorized failure type: `mapping`, `sign`, or `component_disable_mode`
- `expected` and `observed` component contribution payloads

This format is deterministic and machine-readable so CI failures can be triaged
without ad-hoc log parsing.
