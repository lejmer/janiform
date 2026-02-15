## ISSUE-002: Oracle Pricer Risk Benchmark Harness Implementation

### Description
Implement benchmark harnesses for oracle solver, characteristic-function pricing, and sufficient-statistics risk sampling with unified execution controls.

### Acceptance Criteria
- Harness supports running oracle, pricing, and risk benchmarks from one entrypoint.
- Execution config includes warmup, iteration count, and reproducibility seed options.
- Benchmarks emit latency and throughput metrics per component.
- Harness output format is machine-readable for downstream reporting.

### Unit tests
- Tests for benchmark harness configuration validation.
- Tests for per-component benchmark invocation wiring.
- Tests for benchmark output schema stability.

### Notes/Constraints
- Keep harness internals decoupled from production execution paths.
- Avoid benchmark-specific code branching in core algorithms.
