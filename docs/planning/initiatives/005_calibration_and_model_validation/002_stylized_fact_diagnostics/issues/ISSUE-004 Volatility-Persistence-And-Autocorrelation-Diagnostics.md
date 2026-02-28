## ISSUE-004: Volatility Persistence And Autocorrelation Diagnostics

### Description
Implement diagnostics for volatility persistence and related autocorrelation structure to evaluate fitted-model temporal behavior.

### Acceptance Criteria
- Module computes volatility-persistence metrics on empirical and simulated series
- Diagnostics support multiple horizons and lag settings
- Outputs include model-versus-data deltas and threshold context
- Diagnostic interface integrates with shared metric catalog

### Unit tests
- Autocorrelation and persistence metric correctness on reference fixtures and lag and horizon parameter validation
- Deterministic outputs under fixed inputs

### Notes/Constraints
- Keep lag configuration explicit and documented
- Separate raw-timeseries preprocessing from metric computation
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
