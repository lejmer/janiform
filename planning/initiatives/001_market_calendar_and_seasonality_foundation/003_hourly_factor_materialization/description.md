## Project 003: Hourly Factor Materialization

Convert seasonality and news-intensity signals into interval-aligned factors for simulation and pricing.

### Context and requirements
- Convert continuous or high-resolution seasonal signals into hourly piecewise-constant factors.
- Generate and expose `s_h` and `nu_h` values for each modeled interval.
- Guarantee deterministic interpolation and interval assignment rules.

### Definition of Done
- Core APIs provide hourly factor materialization for any date range and supported exchange.
- Pricing and simulation modules consume `s_h` and `nu_h` through shared interfaces.
- Tests cover interval boundaries, daylight saving transitions, and missing-data guards.

### Agent implementation notes
- Keep factor materialization independent of pricing code so it can be reused by calibration and simulation.
- Record metadata on interpolation choices to aid auditability.

### Initiative alignment
- Satisfies Initiative 001 signal that hourly piecewise-constant factors are generated and consumed by engines.
- Provides the deterministic bridge from calendar-time estimation to coarse-time model execution.
