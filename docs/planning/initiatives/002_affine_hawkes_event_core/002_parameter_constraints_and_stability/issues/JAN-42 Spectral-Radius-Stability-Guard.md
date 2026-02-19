## Spectral Radius Stability Guard

### Description
Implement the de-seasonalized Hawkes stability guard using spectral radius of `Alpha / beta` and expose deterministic pass or fail results.

### Acceptance Criteria
- Stability checker computes `rho(Alpha / beta)` for the 2x2 endogenous matrix
- Checker enforces strict stationarity threshold `rho(Alpha / beta) < 1`
- Numeric tolerance policy near threshold is documented and configurable
- Stability output includes computed radius for diagnostics

### Unit tests
- Stable and unstable parameter fixtures with expected decisions and near-threshold behavior with tolerance policy
- Deterministic numeric output across repeated runs

### Notes/Constraints
- Keep matrix construction consistent with `alpha` component ordering
- Use stable numeric routines suitable for small matrices
