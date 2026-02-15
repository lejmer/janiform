## ISSUE-004: Volatility Persistence And Autocorrelation Diagnostics

### Description
Implement diagnostics for volatility persistence and related autocorrelation structure to evaluate fitted-model temporal behavior.

### Acceptance Criteria
- Module computes volatility-persistence metrics on empirical and simulated series.
- Diagnostics support multiple horizons and lag settings.
- Outputs include model-versus-data deltas and threshold context.
- Diagnostic interface integrates with shared metric catalog.

### Unit tests
- Tests for autocorrelation and persistence metric correctness on reference fixtures.
- Tests for lag and horizon parameter validation.
- Regression tests for deterministic outputs under fixed inputs.

### Notes/Constraints
- Keep lag configuration explicit and documented.
- Separate raw-timeseries preprocessing from metric computation.
