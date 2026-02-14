## Project 002: Characteristic Function Pricer

### Context and requirements
- Build a coarse-interval option pricer using transform outputs and Fourier inversion methods.
- Apply Esscher-tilted Levy dynamics under the risk-neutral pricing setup.
- Support baseline vanilla option pricing workflows with clear model input contracts.
- Use hourly boundaries as the v0.1 intraday default while keeping interfaces aligned with interval-parametric solver outputs.

### Definition of Done
- Pricer computes prices for vanilla options from hourly boundary state and model parameters.
- Numerical integration settings are configurable and tested for stability.
- Validation compares outputs against benchmark scenarios and sanity bounds.

### Agent implementation notes
- Keep pricing engine modular so payoff types can be extended later.
- Expose intermediate diagnostics for calibration against option surfaces.

### Initiative alignment
- Satisfies Initiative 004 success signal for a production characteristic-function pricing path.
- Enables direct coarse-time pricing without tick-level replay.
