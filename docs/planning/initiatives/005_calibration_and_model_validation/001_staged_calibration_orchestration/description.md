## Project 001: Staged Calibration Orchestration

Orchestrate multi-stage calibration with deterministic execution, constrained optimization, and mode-aware checkpoint and resume behavior.

### Context and requirements
- Build an end-to-end calibration workflow with explicit stages for seasonality, Hawkes, and return components.
- Support constrained optimization with deterministic seeds and explicit `execution_mode` control: `memory` or `disk`.
- Ensure stage handoffs work with in-memory stage outputs and persisted artifact references.
- Ensure calibration resume semantics are mode-aware: same-process resume in `memory` mode and persisted resume in `disk` mode.

### Definition of Done
- Calibration runner executes staged fitting with clear inputs, outputs, and stage dependencies in both `memory` and `disk` modes.
- Constraint handling and objective configuration are explicit and reproducible.
- Integration tests cover successful runs, override behavior, and expected mode-specific failure conditions.

### Agent implementation notes
- Keep stage interfaces composable so experiments can swap objective terms.
- Record solver settings in run results for both modes and in persisted artifacts for `disk` mode.
- Treat JAN-122 and JAN-130 together as the complete run-contract source for mode-aware orchestration.

### Initiative alignment
- Delivers Initiative 005 success signal for a reproducible constrained calibration pipeline.
- Creates a stable backbone for model fitting in v0.1.
