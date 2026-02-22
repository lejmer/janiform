# NIG Cross-Engine Interface And Reference Fixtures

This guide defines the shared NIG trading-time driver contract used by pricing
and simulation and the fixture workflow that guards against implementation drift.

## Shared interface contract

The canonical driver payload is `NigBuySellDriverParameters`, derived from one
`NigSharedPlusShiftConfiguration`.

- Driver interface policy:
  `shared_buy_sell_driver_parameters_interface_for_pricing_and_simulation.v1`
- Shared-plus-shift mapping policy:
  `buy_beta_equals_beta_0_and_sell_beta_equals_beta_0_plus_delta_beta.v1`

Pricing and simulation consume this shared interface through thin adapters:

- `pricing::compose_nig_trading_time_exponent_for_pricing`
  - Uses `NigBuySellDriverParameters::compose_trading_time_weighted_characteristic_exponent`
  - Enforces sell-side sign convention through `psi_-(-u)` and publishes
    `buy_side_uses_psi_plus_of_u_and_sell_side_uses_psi_minus_of_negative_u.v1`
- `simulation::sample_nig_trading_time_increment_for_simulation`
  - Uses `NigBuySellDriverParameters::sample_trading_time_return_increment`
  - Enforces trading-time increment composition through
    `buy_side_increment_minus_sell_side_increment.v1`

## Reference fixture contract

Fixture location:

- `tests/fixtures/nig/nig_cross_engine_reference_fixture.json`

Schema and fixture ids:

- `schema_version`: `nig.cross_engine.reference_fixture.v1`
- `fixture_set_id`: `nig_cross_engine_reference_fixture_v1`

Coverage:

- Deterministic exponent reference values on fixed complex argument grids.
- Seeded sampled-increment sanity cases with statistical tolerances.
- Cross-engine consistency checks that compare simulation empirical
  characteristic-function estimates against pricing exponent composition.

## Fixture update workflow

When intentional model changes alter reference outputs:

1. Update implementation and keep shared interface policy unchanged unless the
   public contract changes.
2. Recompute deterministic exponent references with the canonical
   `NigSharedPlusShiftConfiguration` in the fixture.
3. Recompute seeded sampling expectations and tolerances, keeping deterministic
   and statistical checks separate.
4. Update `canonical_regression_metadata` stable fixture id lists when cases are
   added or removed.
5. Run:
   - `cargo test -p janiform-core nig_cross_engine_reference_regression`
   - `cargo test -p janiform-core`
6. Review fixture README and this guide for any contract wording drift.
