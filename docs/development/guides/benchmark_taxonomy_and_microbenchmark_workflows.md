# Benchmark Taxonomy And Microbenchmark Workflows

This repository uses two benchmark layers on purpose:

- microbenchmark tooling for local, narrow timing work
- fixture-driven benchmark governance for workflow-scale regression policy

They solve different problems and should not be treated as interchangeable.

## Why Two Layers

Criterion and `pytest-benchmark` are the standard local tools that Rust and Python contributors
expect for tight feedback loops on narrow code paths. The existing fixture-driven harnesses remain
the source of truth for deterministic workflow acceptance, baseline comparison, provenance, and
failure classification.

Use the microbenchmark suites when the question is "did this kernel or wrapper get
faster". Use the fixture-driven benchmark system when the question is "did the accepted workflow
behavior or release budget regress".

## Benchmark Taxonomy

- Microbenchmark: one hot function, kernel, accessor, or serialization path measured with minimal
  surrounding workflow setup.
- Component benchmark: a bounded engine-level path such as oracle solving, pricing, or
  sufficient-statistics sampling exercised with deterministic checked-in inputs.
- Workflow acceptance benchmark: a representative end-to-end path with realistic fixture inputs and
  machine-readable benchmark artifacts.
- CI governance benchmark: a scheduled or manually triggered benchmark workload whose output is used
  for drift classification, acceptance gating, or artifact publication.

## Criterion

Use Criterion for pure Rust kernels and component-level hot paths under
`crates/janiform-governance-bench/benches/rust_microbenchmarks.rs`.

The initial Rust microbenchmark suite covers:

- `oracle_solver_inner_path`
- `pricing_kernel_hot_path`
- `risk_sampling_kernel`
- `simulation_transition_primitive`

These benches do not use ad hoc inline constants. They load the checked-in benchmark scenario
catalog and benchmark policy fixtures from `tests/fixtures/benchmarks/` through the shared
`janiform::benchmarking` microbenchmark support layer.

## Pytest Benchmark

Use `pytest-benchmark` for narrow Python-facing paths under
`benches/python/python_microbenchmarks.py`, with shared benchmark-only support under
`benches/python/support/`.

The initial Python microbenchmark suite covers:

- seeded native-wrapper overhead through `run_seeded_tick_event_simulation(...)`
- seeded event-tape accessor overhead
- seeded response serialization round-trip overhead
- seeded result `to_pandas()` materialization overhead

These benchmarks reuse the checked-in real-data seeded runtime fixture rather than defining a second
unrelated Python benchmark dataset.

## Fixture-Driven Governance

Keep using the existing fixture-driven benchmark system for workflow acceptance and CI governance.

That layer includes:

- the Rust oracle, pricing, and risk benchmark harness and accepted baselines
- the real-data seeded runtime benchmark for fit plus simulation plus Python boundary behavior
- local benchmark runs that publish machine-readable artifacts and compare them against checked-in
  baselines

Those workflows remain the source of truth for end-to-end acceptance checks and regression
classification.

## Local Commands

Rust microbenchmark workflow:

```bash
cargo bench -p janiform-governance-bench --bench rust_microbenchmarks -- --list
TMPDIR=/tmp cargo bench -p janiform-governance-bench --bench rust_microbenchmarks
```

Python microbenchmark workflow:

```bash
PYTHONPATH=python:. pytest benches/python/python_microbenchmarks.py --benchmark-only
PYTHONPATH=python:. pytest benches/python/test_microbenchmark_tooling.py benches/python/python_microbenchmarks.py --benchmark-disable
```

Workflow acceptance and governance benchmarks:

```bash
TMPDIR=/tmp cargo test -p janiform-governance-bench --test oracle_pricer_risk_benchmark_harness -- --nocapture
python3 scripts/python_benchmark_workloads.py
```

## CI Policy

Microbenchmarks are informative local tools. Their timings are intentionally not required
merge gates on every commit.

The repository keeps the Rust governance benchmark smoke tier in
`.github/workflows/ci-rust-benchmark-governance-smoke.yml`.

The Python benchmark workloads are local-only because their representative fixture setup and
resource usage are too machine-dependent for reliable GitHub-hosted CI execution. Run them with:

```bash
python3 scripts/python_benchmark_workloads.py
```

## Input Provenance

Both microbenchmark suites reuse checked-in deterministic inputs:

- Rust Criterion benches reuse `benchmark_scenario_catalog.json` and
  `oracle_pricer_risk_benchmark_policy.json`
- Python `pytest-benchmark` benches reuse
  `real_data_seeded_runtime_smoke_fixture.json`

This keeps the microbenchmark layer representative enough to be useful while avoiding a second set
of unrelated benchmark fixtures.
