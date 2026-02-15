## Initiative 004: Coarse-Time Pricing And Risk Engines

Deliver tractable coarse-time pricing and risk engines powered by the exponential-affine sufficient-statistics oracle.

### Desired outcome
- Janiform supports direct coarse-interval simulation and pricing without requiring tick-by-tick replay.
- The exponential-affine sufficient-statistics oracle is implemented as the canonical bridge between fine and coarse scales.
- v0.1 includes two production paths: a characteristic-function pricing engine and a sufficient-statistics risk simulation engine.
- Coarse-interval execution is interval-parametric in `Delta`, with hourly as the v0.1 default for intraday option-pricing workflows.

### Success signals
- The ODE system for `C(t)` and `D(t)` is implemented with explicit domain guards for transform validity and numerical stability.
- The pricing engine computes coarse-interval characteristic functions from hourly boundary state and prices vanilla options through Fourier methods.
- The risk engine samples interval sufficient statistics (`A+`, `A-`, `N+`, `N-`) and updates state without cluster simulation.
- Benchmarks show coarse-time simulation and pricing can run at practical portfolio scales.
- Non-pricing generation supports configurable interval grids when compatible factors are provided, including sub-hour intervals.

### Example projects
- Build the ODE-based transform solver and reusable oracle API in `crates/core`.
- Implement CF-based option pricer with Esscher-tilted Levy component under risk-neutral measure.
- Implement sufficient-statistics sampler and path simulator for risk scenarios.
- Add numerical regression tests against controlled Monte Carlo baselines.
