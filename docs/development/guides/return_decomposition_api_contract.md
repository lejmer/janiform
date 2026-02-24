# Return Decomposition API Contract

This guide defines the canonical return-decomposition contract shared by pricing
and simulation in `janiform-core`.

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

## Edge modes for debug and test workflows

Explicit opt-in component-disable modes are exposed through
`return_decomposition::ReturnDecompositionEdgeMode`:

- `disabled`: canonical production behavior
- `buy_only`: disables sell and gap components
- `sell_only`: disables buy and gap components
- `gap_only`: disables buy and sell components

`ReturnDecomposition::with_edge_mode` applies deterministic zeroing so disabled
components contribute exactly zero.

## Engine reuse

Pricing and simulation use this contract in their public NIG adapter entrypoints:

- `pricing::compose_nig_trading_time_exponent_for_pricing`
- `pricing::compose_nig_trading_time_exponent_for_coarse_transform_integration`
- `pricing::compose_total_return_characteristic_function_for_pricing`
- `pricing::compose_total_return_characteristic_function_for_coarse_transform_integration`
- `simulation::sample_nig_trading_time_increment_for_simulation`

Edge-mode-aware debug and test entrypoints:

- `pricing::compose_nig_trading_time_exponent_for_pricing_with_edge_mode`
- `pricing::compose_nig_trading_time_exponent_for_coarse_transform_integration_with_edge_mode`
- `pricing::compose_total_return_characteristic_function_for_pricing_with_edge_mode`
- `pricing::compose_total_return_characteristic_function_for_coarse_transform_integration_with_edge_mode`
- `simulation::sample_nig_trading_time_increment_for_simulation_with_edge_mode`

This keeps component wiring explicit and prevents anonymous tuple-style passing.
