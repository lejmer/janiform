## Real-Data Seeded Runtime Benchmark Fixtures And CI Baselines

### Description
Add representative real-data benchmark fixtures, acceptance baselines, and CI coverage for the calibration plus seeded-simulation runtime path. The current code exposes seeded-runtime diagnostics, but those diagnostics are not yet turned into canonical benchmark artifacts with documented thresholds or scheduled CI gates.

This gap is now concrete:
- `python/janiform/simulation/seeded/contracts.py` defines diagnostics such as `transition_count`, `event_count`, `native_runtime_seconds`, `python_response_construction_seconds`, `python_materialization_seconds`, `boundary_transport_bytes`, and `native_estimated_retained_bytes`.
- `crates/pybind/src/types/simulation.rs` exposes native retained-byte estimates and chunked result accessors.
- `crates/core/src/simulation/artifacts.rs` computes native retained-memory estimates for the canonical seeded result.
- Current CI tiers validate engine benchmark harnesses and benchmark contracts for oracle, pricing, and risk paths, but they do not run a representative real-data seeded workflow or compare runtime and memory diagnostics against a stored acceptance baseline.

As a result, the repository can regress in seeded runtime behavior without a canonical signal that answers the questions we actually care about:
- Did event counts return to a realistic regime for representative unsigned market data
- Did seeded runtime or Python materialization cost regress materially
- Did boundary transport size regress materially
- Did end-to-end retained-memory behavior regress materially
- Is a slow run caused by a bad model fit, by boundary overhead, or by both

This issue is specifically about making those questions measurable and enforceable in CI. It is not the issue that redesigns the seeded boundary itself.

### Implementation Details
- Define one or more representative benchmark fixtures for the repaired real-data workflow, starting with the existing synthetic EUR/USD-style unsigned tick dataset used in the runtime-recovery investigation.
- Add a benchmark report contract for the real-data seeded workflow that records at minimum:
  - observed source tick count
  - calibrated expected event-count regime where available
  - coarse generated event count
  - seeded transition count
  - seeded event count
  - native runtime seconds
  - Python response-construction seconds
  - Python materialization seconds for representative access patterns
  - boundary transport bytes
  - native estimated retained bytes
  - explicit benchmark provenance for dataset slice, calibration configuration, and execution mode
- Define a baseline artifact and comparison report for those real-data seeded benchmark outputs, following the same general pattern used by the oracle-pricer-risk benchmark baseline machinery.
- Introduce explicit threshold policy or acceptance-band rules for:
  - event-count realism relative to observed source data
  - maximum tolerated runtime regression
  - maximum tolerated Python materialization regression
  - maximum tolerated boundary transport growth
  - maximum tolerated retained-memory growth
- Ensure the benchmark report can separate model-regime failures from boundary failures. A run that explodes to unrealistic event counts should be distinguishable from a run with acceptable model counts but unacceptable transport or materialization overhead.
- Add a CI tier or scheduled workload dedicated to this benchmark path. The workload should be scheduled or manually gated rather than required on every commit unless the fixture is proven to be cheap and stable enough for normal regression runs.
- Publish machine-readable benchmark artifacts and human-readable summaries so failures include:
  - benchmark fixture identity
  - baseline version
  - observed metric deltas
  - failure classification
- Reuse the existing seeded diagnostics fields where they already capture the needed metrics. Do not create duplicate runtime diagnostics with different meanings unless there is a clear gap.
- Where true peak RSS cannot be measured portably in the first version, document that limitation explicitly and treat `native_estimated_retained_bytes` as an interim retained-memory proxy rather than calling it peak memory.

### Acceptance Criteria
- The repository includes a canonical real-data seeded benchmark fixture for at least one representative unsigned market-data workflow.
- Benchmark outputs for that workflow are serializable, versioned, and stored as a machine-readable artifact with explicit provenance.
- The benchmark artifact includes event-count, runtime, transport-size, and retained-memory metrics sufficient to explain the major runtime-recovery failure modes.
- The repository includes a baseline artifact and comparison path that can classify candidate runs against a stored accepted baseline.
- CI runs the real-data seeded benchmark workload on a scheduled or manually gated basis and fails when configured event-count or performance thresholds are exceeded.
- CI failure output makes it clear whether the failure is primarily:
  - model-regime drift
  - runtime drift
  - boundary transport drift
  - retained-memory drift
- The benchmark documentation states clearly whether the memory metric is true peak memory or an estimated retained-memory proxy.

### Unit tests
- Fixture-loading and schema-validation tests for the real-data seeded benchmark fixture and baseline artifacts.
- Benchmark report tests asserting that seeded diagnostics are captured into the new benchmark artifact with stable required-field metadata.
- Baseline comparison tests asserting that runtime, transport, event-count, and retained-memory deltas are computed deterministically and classified against explicit thresholds.
- CI smoke tests asserting that the new scheduled or manually gated workload is wired correctly and publishes its artifacts.
- Regression tests covering at least one failure mode where event-count blow-up causes benchmark failure and one failure mode where boundary or materialization overhead causes benchmark failure.

### Notes/Constraints
- Keep the first benchmark fixture representative but bounded. The goal is not to run an unbounded production-scale dataset in normal CI.
- Avoid inventing a second performance-reporting vocabulary when the seeded runtime path already exposes useful diagnostics.
- Be explicit about metric semantics. `native_estimated_retained_bytes` is not the same thing as true process peak memory and must not be labeled as such unless measurement semantics change.
- Keep the benchmark artifact and threshold policy focused on seeded runtime and memory recovery for real-data workflows. Do not fold unrelated calibration-quality governance into this issue.
