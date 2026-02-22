# NIG Cross-Engine Reference Fixture

This fixture stores the compact reference matrix for NIG buy and sell driver
integration across pricing and simulation paths.

## File

- `nig_cross_engine_reference_fixture.json`

## Contract

- `schema_version`: `nig.cross_engine.reference_fixture.v1`
- Fixture set id: `nig_cross_engine_reference_fixture_v1`
- Shared interface policy:
  `shared_buy_sell_driver_parameters_interface_for_pricing_and_simulation.v1`

## Coverage

- Deterministic weighted-exponent reference values at fixed complex argument
  grids.
- Seeded sampled-increment sanity checks with statistical tolerances.
- Cross-engine consistency checks comparing simulation empirical characteristic
  function estimates against pricing exponent composition.
