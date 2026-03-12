## High-Level Model Simulation Pricing And Persistence Surface

### Description
Define the fitted-model surface for simulation, pricing, inspection, and persistence so the output of the new estimator is a practical user-facing object rather than a thin container around low-level artifacts.

### Implementation details
- Add one high-level simulation method on the fitted model with an output-mode selector instead of separate conceptual workflows for coarse and tick generation.
- Use the design-note shape as the default target:
  - `simulate(..., output="coarse" | "ticks" | "both")`
- Define how `JaniformModel` also exposes:
  - `price_option(...)`
  - `summary()`
  - `save(...)`
  - `load(...)`
  - fitted-parameter inspection through a property or explicit method
- Keep runtime factor wiring, session materialization, and preset-derived controls internal to the model surface.
- Define the high-level response shape for simulation and pricing outputs, including whether the public Python surface returns typed objects, pandas-first tables, or a hybrid contract with optional frame helpers.
- Require summaries and persisted model metadata to expose preset provenance, session-inference outcome, and known-news activation state.

### Acceptance Criteria
- Planning defines the default fitted-model methods for simulation, pricing, summary, and persistence.
- The simulation API uses one conceptual method with an explicit output-mode selector instead of multiple overlapping public workflows.
- Model summaries and persisted metadata expose fitted-parameter and preset-provenance information clearly.
- The design stays aligned with the underlying canonical runtime paths and does not leak low-level factor-wiring details into the normal user API.

### Unit tests
- Contract tests for simulation output-mode selection and mode-dependent response shapes.
- Summary and metadata tests verifying presence of fitted-parameter and preset-provenance fields.
- Persistence tests for save and load round-trips with deterministic metadata.
- Integration tests showing pricing and simulation methods dispatch through the canonical model execution path.

### Notes/Constraints
- Keep the public fitted-model surface small and coherent. Do not add separate method names that only encode output transport variants.
- If pandas or similar frame helpers are exposed, keep the canonical contract inspectable and deterministic for non-interactive use.
