## Initiative 002: Affine Hawkes Event Core

Implement the core bivariate Hawkes event engine and safety constraints that define Janiform fine-scale activity dynamics.

### Desired outcome
- Janiform provides a stable, calibrated bivariate Hawkes event engine for buy and sell arrivals with exponential-kernel excitation and shared decay.
- The engine implements structured asymmetry for endogenous and exogenous excitation (`alpha`, `rho`) while enforcing positivity and stability constraints.
- Tick-event generation is usable as the canonical fine-scale model, with explicit state carry-over across intervals.
- Tick-event timestamps are simulated in event time without imposing a fixed intrainterval time grid.
- Fine-scale tick simulation and direct coarse-step simulation are explicitly aligned for time-scale consistency under shared model inputs.

### Success signals
- Core types encode the 4D excitation state (`Y++`, `Y+-`, `Y-+`, `Y--`) and update rules exactly once in a reusable module.
- Parameter validation enforces non-negativity, positive decay and baseline, and spectral-radius stability (`rho(Alpha / beta) < 1`).
- Simulation paths produce event clustering and buy-sell asymmetry under controlled seeded runs.
- State transition at interval boundaries is deterministic given seed and parameters and is covered by unit tests.
- Aggregating one-month tick paths to coarse horizons such as 15m, 1h, and 1D produces summary behavior consistent with direct simulation at the same coarse horizons.
