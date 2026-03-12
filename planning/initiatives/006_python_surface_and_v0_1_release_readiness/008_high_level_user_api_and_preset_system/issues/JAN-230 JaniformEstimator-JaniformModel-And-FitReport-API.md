## JaniformEstimator JaniformModel And FitReport API

### Description
Introduce the default high-level Python object model for fitting so users work with a fit-time estimator, a lean fitted model object, and an optional detailed fit report instead of staged execution payloads and manual runtime assembly.

### Implementation details
- Define the public high-level objects:
  - `JaniformEstimator`
  - `JaniformModel`
  - `JaniformFitReport`
- Give `JaniformEstimator` responsibility for fit-time configuration, preset resolution, session-inference policy selection, user-facing validation, and dispatch into the canonical calibration workflow.
- Define `fit(...) -> JaniformModel` and `fit(..., report=True) -> tuple[JaniformModel, JaniformFitReport]` or an equivalent explicit high-level contract if a small naming adjustment is needed during implementation.
- Ensure the estimator accepts market-data-first inputs, preset references, explicit overrides, and workflow hints without surfacing staged execution internals as normal user inputs.
- Keep `JaniformModel` lean:
  - expose fitted parameters and concise metadata
  - retain preset and fit provenance
  - materialize runtime controls internally
  - do not store raw market data by default
- Keep `JaniformFitReport` separate from the fitted model so detailed calibration diagnostics remain available without making normal fitted-model objects heavy and noisy.
- Define how this high-level object model composes with the existing market-data ingestion and staged-calibration translation work planned in Project 005.

### Acceptance Criteria
- Planning defines a high-level fit object model that matches the design note and provides one clear default fitting workflow.
- The estimator, model, and fit-report responsibilities are separated cleanly and documented.
- Raw market data is not retained inside the estimator or fitted model by default.
- The high-level fit path delegates to the canonical calibration workflow rather than introducing a second calibration engine.
- Project 005 boundaries for ingestion and staged-input derivation are reused rather than duplicated.

### Unit tests
- High-level contract tests for estimator construction, fit return shapes, and optional report behavior.
- Data-retention tests asserting raw input data is not stored by default on estimator or model objects.
- Integration tests showing preset resolution and market-data-first calibration feed the existing staged runner path.
- Negative tests for invalid high-level configuration and failure reporting behavior.

### Notes/Constraints
- Pre-v0.1 policy applies. If the current public API conflicts with this clearer object model, replace it rather than layering wrappers on top.
- Keep advanced staged-calibration inputs available through separate expert paths instead of overloading the default estimator surface.
