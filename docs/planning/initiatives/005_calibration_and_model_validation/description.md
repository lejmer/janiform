## Initiative 005: Calibration And Model Validation

Establish reproducible calibration workflows and validation tooling that prove fitted models meet target stylized facts and scale consistency.

### Desired outcome
- Janiform has an end-to-end calibration workflow that fits model parameters on market data with robust constraints and reproducible outputs in `memory` and `disk` execution modes.
- The fitted model is validated against the target stylized facts and scale-consistency requirements defined for v0.1.
- The team can diagnose fit quality, parameter identifiability, and failure modes before using outputs for pricing or risk.

### Success signals
- A reproducible calibration pipeline exists for seasonality, Hawkes, and return components with clear parameter bounds and penalties, and it runs in `memory` and `disk` modes.
- Validation reports include activity clustering, heavy tails, skew, volatility persistence, and gap behavior across multiple horizons.
- Fine-to-coarse consistency checks show direct coarse simulation agrees with aggregated fine simulation within predefined tolerances.
- Calibration artifacts (inputs, params, metrics, diagnostics) are versioned and reloadable for persisted runs where `execution_mode=disk`.
