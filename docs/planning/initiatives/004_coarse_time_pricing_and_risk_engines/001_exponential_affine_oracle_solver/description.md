## Project 001: Exponential Affine Oracle Solver

### Context and requirements
- Implement the ODE solver for `C(t)` and `D(t)` that defines the sufficient-statistics transform over an interval.
- Support hourly piecewise-constant factors and configurable numeric integrators.
- Enforce domain guards for transform validity and fail safely on blow-up conditions.

### Definition of Done
- Solver API computes transform outputs for pricing and risk sampling use cases.
- Domain constraints are validated during integration with clear error reporting.
- Regression tests verify solver outputs on deterministic parameter fixtures.

### Agent implementation notes
- Separate equation specification from integration strategy to allow numerical-method upgrades.
- Emit diagnostics useful for debugging integration instability.

### Initiative alignment
- Delivers Initiative 004 success signal for an implemented and guarded ODE transform system.
- Provides the core bridge from fine-scale dynamics to coarse-time execution.
