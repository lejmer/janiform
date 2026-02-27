## Project 001: Python API Contracts

Define the stable Python API contract for simulation, calibration, and pricing workflows.

### Context and requirements
- Define stable Python API entrypoints for simulation, calibration, and pricing workflows.
- Specify typed request and response contracts and error behavior.
- Keep API design aligned with core model concepts and v0.1 scope.
- Encode `execution_mode` behavior in calibration workflows with API default `memory` and explicit override support.

### Definition of Done
- Public Python modules expose documented and typed APIs for primary workflows.
- Calibration API contracts include explicit `execution_mode` semantics, with default `memory` and override to `disk`.
- API contract tests verify expected behavior and backward-stable signatures for v0.1.
- User-facing examples demonstrate end-to-end usage for each main workflow, including default and override mode behavior.

### Agent implementation notes
- Favor explicit domain objects over loosely typed dictionaries in public APIs.
- Keep experimental endpoints separate from stable v0.1 surface.

### Initiative alignment
- Satisfies Initiative 006 success signal for stable Python user-facing contracts.
- Provides the main adoption path for internal and external users.
