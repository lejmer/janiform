## End To End Coarse Interval Simulation Runner

### Description
Implement the public Rust core runner that executes coarse-time simulation across intervals by orchestrating transform-slice solving, sufficient-statistics sampling, direct-jump generation, return generation, and boundary-state updates.

Today the core contains the main building blocks, but there is no single simulation-facing entrypoint that consumes a coarse simulation request and emits a complete path artifact. This issue closes that orchestration gap.

### Acceptance Criteria
- Rust core exposes one public coarse simulation runner for deterministic seeded interval-path generation.
- The runner composes transform-slice solving, interval sufficient-statistics sampling, return generation, and boundary-state carryover across the full horizon.
- The runner supports interval-parametric execution for compatible factor inputs instead of hardcoding hourly-only operation.
- The runner is reproducible under fixed seeds and shares random-stream policies with existing engine conventions.
- The runner emits the canonical coarse simulation path artifact defined for this project.

### Unit tests
- Single-interval and multi-interval success-path tests for coarse simulation execution.
- Deterministic repeated-run tests under fixed seeds.
- Failure-path tests for invalid contracts, solver failures, and state-update failures surfaced through the runner boundary.

### Notes/Constraints
- Keep the orchestration in a simulation-facing module and avoid forcing callers to manually sequence lower-level risk-sampler APIs.
- Reuse existing sufficient-statistics and state-update internals rather than wrapping them in thin pass-through layers that add no boundary value.
