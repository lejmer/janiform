## Project 001: Python API Contracts

Define the Python API contract for simulation, calibration, and pricing workflows.

### Context and requirements
- Define Python API entrypoints for simulation, calibration, and pricing workflows.
- Specify typed request and response contracts and error behavior.
- Keep API design aligned with core model concepts and v0.1 scope.
- Encode `execution_mode` behavior in calibration workflows with API default `memory` and explicit override support.
- Ensure request and response contracts can represent known-surprise news controls and open-jump versus closed-gap return semantics where exposed.
- Add executable workflow-input contracts when request and response domain models alone are not sufficient to drive core services without hidden lookup or lossy semantic collapse.

### Definition of Done
- Public Python modules expose documented and typed APIs for primary workflows.
- Calibration API contracts include explicit `execution_mode` semantics, with default `memory` and override to `disk`.
- API contract tests verify expected behavior and deterministic schema semantics for the current pre-v0.1 surface.
- User-facing examples demonstrate end-to-end usage for each main workflow, including default and override mode behavior.
- Workflow orchestrator work depends only on execution contracts that preserve model semantics and can be dispatched without implicit data loading.

### Agent implementation notes
- Favor explicit domain objects over loosely typed dictionaries in public APIs.
- Pre-v0.1 contract evolution is expected. Breaking changes are allowed when they simplify and clarify the intended model behavior.

### Initiative alignment
- Satisfies Initiative 006 success signal for explicit Python user-facing contracts.
- Provides the main adoption path for internal and external users.
