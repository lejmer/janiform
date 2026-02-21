# Seeded Tick Simulation Parity Fixture

This fixture stores deterministic seeded tick-event parity scenarios used by both Rust and Python tests.

## File

- `seeded_tick_simulation_parity_fixture.json`

## Contract

- `schema_version`: `simulation.seeded_tick.parity_fixture.v1`
- Fixture set id: `seeded_tick_simulation_parity_fixture_v1`

## Coverage

- One short horizon case and one medium horizon case
- Event-time arrivals are non-grid timestamps
- Near-tied adjacent timestamp ordering behavior is explicitly covered
- Interval-level coarse summaries are included for scale-consistency workflows
