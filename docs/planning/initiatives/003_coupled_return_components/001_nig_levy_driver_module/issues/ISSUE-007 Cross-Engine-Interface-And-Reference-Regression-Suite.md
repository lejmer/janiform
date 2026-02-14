## ISSUE-007: Cross Engine Interface And Reference Regression Suite

### Description
Finalize shared NIG driver interfaces used by pricing and simulation engines and add a reference regression suite to prevent implementation drift.

### Acceptance Criteria
- One shared interface is used by both characteristic-function pricing and increment simulation paths.
- Reference fixtures cover exponent evaluation and sampled-increment sanity checks.
- Regression suite detects divergence between pricing and simulation parameter interpretation.
- Documentation includes interface contract and fixture update workflow.

### Unit tests
- Integration tests confirming both engines consume the same driver interface.
- Regression tests for reference exponent values at fixed argument grids.
- Regression tests for seeded sampling sanity metrics.

### Notes/Constraints
- Keep fixture set compact and versioned for long-term maintainability.
- Separate deterministic numeric checks from statistical tolerance checks.
