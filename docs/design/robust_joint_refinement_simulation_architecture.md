# Robust Joint-Refinement Simulation Architecture

## Summary
Redesign joint refinement so simulation, artifact materialization, and objective evaluation are separate concerns. Keep restart-level parallelism and exact objective semantics, but stop materializing full seeded `fine_path` artifacts for joint refinement. Add a streaming summary path that produces the exact same `JointRefinementSeedSummary` inputs from a single deterministic simulation pass, then run heavy summary simulations through a memory-aware core-runtime scheduler with process-isolated workers so allocator failure never crashes the host process.

## Key Changes
- Split the seeded simulator into a shared transition engine plus pluggable consumers.
    - Introduce a streaming transition-consumer API in `janiform-engine-simulation` that emits each deterministic transition as it is simulated.
    - Reimplement the current full-path result builder as one consumer so `run_seeded_tick_event_simulation(...)` keeps its current behavior and outputs.
    - Add a second consumer path for calibration that does not retain `SeededFinePathRecord` rows unless explicitly requested.

- Add an exact summary-only seeded simulation path for joint refinement.
    - Implement a joint-refinement summary collector in `janiform-calibration-runtime` that consumes streamed transitions and builds the exact data currently derived from `fine_path`.
    - Maintain online 5m, 15m, and hourly interval aggregates, hourly absolute trade-return sums and counts, event counts, realized variance, and other interval-derived metrics directly during simulation.
    - Retain only the exact per-trade return scalar vector needed for the existing equal-stride tick-shape sampling rule. Do not retain full trade records, boundary records, path snapshots, or projection indices.
    - Keep `JointRefinementSeedSummary` unchanged and require bit-for-bit parity against the current full-path-derived summary for fixed seeds and inputs.
    - Update `build_seed_summary_for_candidate_seed(...)` to use the summary-only path and remove dependence on `simulation_result.fine_path`.

- Restore and preserve coarse-grained parallelism, but remove multiplicative memory pressure.
    - Keep restart-level parallel optimization parallel under the stage thread pool.
    - Keep inner simulation-seed fanout serial within one candidate evaluation so one candidate cannot multiply its own retained memory footprint by the number of final seeds.
    - Remove the temporary “serialize the whole joint-refinement stage” direction. The correct concurrency model is parallel candidates with bounded per-candidate memory, not globally serial evaluation.

- Add memory-aware task scheduling in core runtime.
    - Estimate summary-simulation memory from exact retained structures: interval templates, hourly statistics, retained trade-return vector capacity, and worker baseline resident bytes reported at runtime.
    - Use current available memory and active worker baseline RSS to decide how many heavy summary simulations may run concurrently. This policy only affects scheduling order, never candidate validity, objective math, or numeric outputs.
    - Use existing `cpu_count` as the upper bound on concurrent workers. Do not add machine-specific candidate rejection or numeric shortcuts.

- Add process-isolated heavy simulation workers in core runtime.
    - Add a dedicated Rust worker executable crate for seeded summary simulation requests.
    - Define a compact request/response contract for one summary simulation job: run config, fitted parameters, summary templates, deterministic seed, and exact summary/failure response.
    - Parent runtime submits heavy summary jobs to workers over stdio using serialized request/response payloads. Each worker executes one job at a time and returns only the summary object, not a full path.
    - If a worker exits abnormally or the OS kills it during allocation, the parent surfaces a structured simulation failure for that candidate/seed instead of crashing the host process.
    - Keep worker isolation inside core runtime, not the Python/CLI boundary.

- Improve failure modeling and diagnostics.
    - Keep the current fallible reserve/growth work in the simulator.
    - Add a dedicated joint-refinement simulation-infrastructure failure path instead of overloading `InvalidCandidateGenerationInput` for worker or summary-simulation runtime failures.
    - Preserve per-candidate `invalid_reason` diagnostics when a candidate fails simulation, and use a stage-level infrastructure failure only when the worker/scheduler layer itself is unavailable or the baseline candidate cannot be evaluated.
    - Default logging stays at `error`. No blanket debug logging by default.

## Public Interfaces
- Keep existing Python and CLI calibration request/response shapes unchanged.
- Keep existing full-path simulation APIs unchanged for export, diagnostics, and direct simulation use.
- Add a new Rust-side streaming simulation interface in `janiform-engine-simulation` for transition consumers.
- Add a new internal worker request/response contract plus worker executable in the core runtime stack.
- Extend joint-refinement failure reporting with a dedicated simulation-infrastructure/resource failure variant. Update in-repo parsers/tests to the new contract in the same change.

## Test Plan
- Exactness parity:
    - Compare old full-path-derived and new summary-only `JointRefinementSeedSummary` outputs bit-for-bit for fixed seeds across representative low-, medium-, and high-activity configurations.
    - Assert identical joint-refinement objective terms, candidate rankings, selected candidate, and final fitted parameters for `cpu_count=1` and `cpu_count>1`.
    - Assert identical results between in-process summary execution and worker-process summary execution.

- Robustness:
    - Reproduce the previous high-activity `fine_path` blowup with a synthetic fixture and verify the host process no longer aborts.
    - Kill or force-fail a worker during one summary simulation and verify the parent returns a structured joint-refinement failure/invalid-candidate diagnostic without crashing.
    - Verify fallback/error behavior when the worker executable cannot be resolved or returns malformed output.

- Performance and scheduling:
    - Assert restart-level parallelism is still active.
    - Add scheduler tests showing concurrent heavy simulations are deferred by memory packing while lighter jobs still use available workers.
    - Add regression coverage that summary-only simulation retained bytes are materially below full-path retained bytes for the same deterministic run.

- Cross-platform:
    - Run the worker-process integration tests on both Windows and Linux.
    - Validate worker executable discovery and invocation from the calibration runtime test harness in both environments.

## Assumptions
- Exactness target is bit-for-bit parity with the current calibrator for fixed seeds and inputs.
- Worker isolation belongs in core runtime, implemented with a dedicated Rust worker executable, not only in Python/CLI wrappers.
- Existing full-path simulation outputs remain supported and unchanged.
- Memory-aware scheduling is allowed to depend on runtime-observed system memory and worker baseline RSS because it changes only concurrency, not calibration math or candidate admissibility.
- The current temporary diagnostic patches are superseded by this architecture. Keep the graceful allocation/error propagation work, but do not keep any whole-stage serialization as the long-term solution.
