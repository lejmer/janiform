## Project 001: Staged Calibration Orchestration

### Context and requirements
- Build an end-to-end calibration workflow with explicit stages for seasonality, Hawkes, and return components.
- Support constrained optimization with deterministic seeds and checkpointing.
- Ensure calibration can resume from intermediate artifacts.

### Definition of Done
- Calibration runner executes staged fitting with clear inputs, outputs, and stage dependencies.
- Constraint handling and objective configuration are explicit and reproducible.
- Integration tests cover successful runs and expected failure conditions.

### Agent implementation notes
- Keep stage interfaces composable so experiments can swap objective terms.
- Record all solver settings in artifacts for auditability.

### Initiative alignment
- Delivers Initiative 005 success signal for a reproducible constrained calibration pipeline.
- Creates a stable backbone for model fitting in v0.1.
