## Project 003: Seeded Tick Event Simulator

Build deterministic seeded tick-event simulation for buy and sell arrivals with reproducible tapes and diagnostics.

### Context and requirements
- Build a deterministic seeded simulator for buy and sell event arrivals under calibrated Hawkes dynamics.
- Generate event tapes with timestamp, side, and state snapshots needed by diagnostics.
- Ensure behavior is reproducible across platforms and bindings.
- Simulate tick arrivals in event time without snapping arrivals to a fixed intrainterval grid.
- Preserve a clear compatibility path to coarse-step simulation so aggregated tick outputs can be validated against direct coarse simulation outputs.

### Definition of Done
- Simulator API accepts seed, parameters, calendar factors, and horizon and returns reproducible event paths.
- Simulation traces exhibit burst clustering and asymmetric response under expected parameter regimes.
- Tests verify seed stability, event ordering, and boundary state carry-over.
- Tick timestamps are represented as event-time arrivals and are not produced by a fixed subinterval lattice.
- Project test fixtures include cross-checks that aggregated tick outputs and direct coarse outputs remain aligned under shared parameters and calendar factors.

### Agent implementation notes
- Keep randomness abstraction explicit so Python and Rust tests can share reference sequences.
- Ensure simulator can run in debug mode with optional trace artifacts for calibration debugging.
- Keep the hourly factor grid scoped to deterministic factor materialization, not as an event timestamp grid.
- Expose enough diagnostics from simulation paths to support later scale-consistency harnesses.

### Initiative alignment
- Delivers Initiative 002 signal on deterministic seeded runs and realistic clustering behavior.
- Provides a reference fine-scale generator for later consistency validation.
- Establishes the fine-path side of the coarse-vs-aggregated consistency requirement.
