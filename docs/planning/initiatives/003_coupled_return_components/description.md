## Initiative 003: Coupled Return Components

Implement the coupled return layer that combines business-time NIG moves with shared-news gap behavior and strict sign semantics.

### Desired outcome
- Janiform implements the full return decomposition `X = trading_time_moves + closed_market_gap_moves`.
- Trading-time moves are driven by buy and sell NIG Levy components in business time with the agreed one-parameter skew asymmetry.
- Exogenous news shocks are modeled as a shared marked Poisson process that couples Hawkes-state stress and closed-market gap returns.

### Success signals
- NIG exponent and parameter constraints are implemented once and reused by simulation and pricing components.
- The model applies sell-side contribution with the correct sign convention in the characteristic function path.
- News marks (`U`, `Z`) follow the specified Gamma plus double-exponential construction and feed both Hawkes shock accumulation and gap returns.
- Gap returns are emitted only during `closed` windows and open-window news affects returns only through activity dynamics.
