## Seeded Simulation Boundary And Memory Performance Refactor

### Description
Refactor seeded-simulation execution and data movement so realistic runs do not pay for full fine-path duplication across Rust, JSON, and Python. The current implementation is too slow and too memory-hungry to be acceptable for real-data smoke tests.

The investigated run took approximately `298` seconds to generate a `96` hour seeded path and used roughly `48 GB` of RAM, while average CPU utilization stayed near `6` to `7` percent. The low CPU utilization is consistent with a mostly single-threaded path saturating roughly one core on a many-core machine, combined with heavy serialization and object-materialization overhead.

The current boundary costs are concrete:
- In `crates/core/src/simulation/runtime.rs`, seeded simulation allocates full `fine_path` and `event_path` vectors.
- The hot event loop in the same module is single-threaded and processes every endogenous and exogenous update sequentially.
- In `crates/pybind/src/bindings/simulation.rs`, `run_seeded_tick_event_simulation` serializes the full fine path to a single JSON string before returning to Python.
- In `python/janiform/simulation/_seeded.py`, Python deserializes the full payload into a `fine_path` tuple of objects and then reconstructs an `event_tape` from `fine_path`, creating another near-full copy when most fine-path records carry an event side.
- Before seeded simulation starts, the example and ingestion path already hold multiple full copies of the source ticks in memory by loading, normalizing, writing CSV, reloading CSV, and rebuilding normalized tuples.

The pathological Hawkes fit is the first source of pain because it generates too many events, but the boundary design is independently unacceptable. Even after Hawkes calibration is repaired, full fine-path JSON round-tripping should not remain the canonical implementation for long-horizon seeded simulation.

### Implementation details
- Replace the current full-fine-path JSON return path with a more appropriate boundary contract for large seeded runs. Options can include chunked native iteration, streaming artifact writing, typed binary transport, or a Rust-owned artifact materialization path with Python loading summaries on demand. The chosen design must avoid building one giant JSON string for the full fine path.
- Remove redundant public-path duplication between `fine_path` and `event_tape`. If both views are needed, define one canonical representation and derive the other lazily or on demand without eagerly materializing both for every run.
- Revisit whether Rust needs to retain both `fine_path` and `event_path` in memory at once for the canonical workflow. Prefer one canonical path plus optional debug or projection modes.
- Ensure the Python boundary can consume seeded outputs incrementally or from artifact-backed storage rather than requiring complete in-memory materialization of the path before any downstream work can begin.
- Audit the manual example and market-data preparation flow for unnecessary input copies. The current pattern of keeping all ticks in memory, writing normalized CSV, and reloading that CSV into new in-memory structures should be reduced where possible.
- Add benchmark fixtures and diagnostics for:
  - total transition count
  - wall-clock time
  - peak memory
  - serialization volume
  - Python materialization time
- Ensure performance diagnostics make it obvious whether a bad run is slow because of a bad model fit, because of boundary overhead, or both.
- Keep deterministic seeding and reproducibility intact. Performance improvements must not introduce nondeterministic path generation or ambiguous ordering semantics.

### Acceptance Criteria
- The canonical seeded-simulation boundary no longer serializes the entire fine path to one JSON string for normal execution.
- The canonical workflow no longer eagerly materializes both a full `fine_path` and an almost identical `event_tape` object graph in Python.
- Seeded-simulation diagnostics expose transition count, boundary transport size, wall-clock timing, and peak-memory-relevant metrics.
- A representative long-horizon seeded smoke test no longer exhibits the previous pathological memory amplification pattern from full-path duplication across Rust, PyO3, JSON, and Python.
- The manual EUR/USD example and related workflows can consume seeded outputs without requiring the current giant in-memory JSON round-trip.
- Benchmark and regression coverage exists for both runtime and memory behavior.

### Unit tests
- Boundary contract tests for the replacement seeded output transport or artifact format.
- Regression tests ensuring `event_tape` and fine-path projections remain semantically consistent when generated lazily or from a canonical source.
- Benchmark-oriented integration tests that capture transition counts, transport size, and timing metadata for representative runs.
- Memory-sensitive workflow tests that assert the seeded path does not create redundant full-path copies in the canonical Python workflow.
- Determinism tests showing the new boundary preserves seed-driven reproducibility.

### Notes/Constraints
- This issue does not remove the need to repair Hawkes calibration. A better boundary cannot compensate for a model that overproduces events by an order of magnitude.
- Pre-v0.1 rules apply. It is acceptable to replace the current seeded boundary contract directly rather than preserving a JSON compatibility shim.
- Keep debug-trace and checkpoint use cases explicit. Artifact persistence for diagnostics or resume is not a reason to round-trip every normal run through a giant JSON string.
