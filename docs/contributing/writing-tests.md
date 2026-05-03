# Writing tests

Tests in Janiform should describe the behavior the project promises today. They are not a place to preserve accidental behavior, duplicate implementation details, or compensate for weak contracts.

Use this page together with the language-specific conventions:

- [Python conventions](python-conventions.md)
- [Rust conventions](rust-conventions.md)

## What to test

Write tests for behavior that can fail in a meaningful way:

- error cases, including conversion and validation failures
- boundary conversions between Python, Rust, files, and command-line payloads
- numerical stability when behavior depends on numerical properties
- contracts that define public API shape, serialized artifacts, command output, or workflow results
- invariants that protect model behavior, pricing, simulation, calibration, validation, or automation

Prefer exact positive contract assertions. A good test states what the behavior is, not merely what it is not.

Avoid tests whose only purpose is to prove that a removed behavior, removed field, old output, or obsolete path is absent. If a state should never be possible, prefer making it impossible through typed contracts, constructors, validators, or narrower public methods. Add or keep a regression test only when the invalid behavior remains easy to reintroduce after the contract shape has been fixed.

## Test shape

Prefer deep equality comparisons whenever practical. Assert against the complete object, response, artifact, or diagnostic payload when that gives a clearer contract than checking isolated fields.

Keep test setup explicit and small. Test data should make the scenario obvious without requiring the reader to reverse-engineer a production workflow.

Avoid mutating process environment in tests. Prefer passing environment-derived flags, paths, or dependencies from above so the test does not leak state into other tests.

Do not hardcode stale outputs just to keep a regression fixture passing. If expected output changes because the contract changed, update the fixture to describe the current contract.

Property tests are useful for tricky invariants, especially where a small set of examples would miss edge cases. Keep the invariant clear, and avoid broad generators that make failures hard to understand.

## Python tests

Python tests live under `tests/` and use pytest. They should focus on the Python boundary: public API contracts, serialization, error mapping, CLI entrypoints, workflow orchestration, and Python-to-Rust conversions.

Keep Python tests thin around model behavior. If a test needs to prove numerical behavior, state evolution, calibration math, pricing logic, or simulation evolution, the primary implementation and the closest tests should usually live in Rust.

Use shared fixtures and support helpers deliberately. Before adding a local helper, check `tests/conftest.py` and `tests/support/`. If an existing fixture can be adapted without weakening other tests, update it. If a new helper is likely to be reused, place it in `tests/conftest.py` or `tests/support/` instead of hiding it in one test module.

For Python-specific style and fixture rules, see [Python conventions](python-conventions.md#tests).

## Rust tests

Rust tests should sit as close as practical to the behavior they validate:

- use unit tests in the owning module for focused internal behavior
- use crate-level `tests/` integration tests for public crate behavior and cross-module contracts
- use cross-crate tests when the behavior is specifically about crate boundaries or integration

Rust core tests should cover computational contracts directly. Prefer testing the validated Rust type or domain API before adding Python boundary coverage for the same behavior.

Use `assert_eq!()` on entire values when possible. This keeps contract drift visible and avoids tests that pass after only one field remains correct.

For Rust-specific rules on PyO3 boundary tests, `assert_eq!()`, and Criterion, see [Rust conventions](rust-conventions.md#tests).

## Integration tests

Integration tests should validate behavior across a real boundary: Python-to-Rust, CLI-to-workflow, serialized artifact round trips, crate-to-crate behavior, or saved workflow outputs.

Keep integration tests focused on the boundary they own. Avoid using an end-to-end workflow when a smaller integration test proves the same contract. When an end-to-end workflow is necessary, make the setup minimal and deterministic.

Integration tests are also the right place for public error contracts. Assert the stable error category, field path, code, or message contract that callers rely on, rather than incidental internal exception text.

## Regression tests

Regression tests are useful when a bug is easy to reintroduce, but they are not a substitute for better API or contract design.

When a bug exposes an invalid state, first make that state hard or impossible to express. Use typed contracts, shared constructors, validators, or narrower public methods before adding a regression test.

Keep a regression test when the mistake could still reasonably return through another path. Name the scenario after the behavior being protected, not after the bug report or implementation detail.

## Benchmarks

Benchmarks should measure behavior that is performance-critical or operationally expensive enough to justify a baseline.

Use Criterion for Rust benchmarks. Python benchmark support belongs under `benches/python/` when the benchmark is specifically about Python boundary cost, workflow overhead, or user-facing Python behavior.

Use `scripts/python_benchmark_workloads.py` for local Python benchmark workloads. It runs
the representative real-data seeded runtime benchmark through
`scripts/ci/run_real_data_seeded_runtime_benchmark.py` and the Python microbenchmark smoke
checks in `benches/python/python_microbenchmarks.py`, writing artifacts under
`.artifacts/local-python-benchmarks` by default. Use `--skip-real-data-seeded-runtime` or
`--skip-microbenchmark-smoke` when a narrower local run is enough.

Benchmark automation is tiered. `ci-pr-fast` keeps fast repository contract checks close to
pull-request feedback, `ci-platform` owns full platform verification, and
`ci-rust-benchmark-governance-smoke` is the manual and reusable Rust benchmark governance smoke
workflow for scenario, baseline, and drift artifact checks.

Optimize based on measurement. Do not change numerical policy, model behavior, solver choices, or tolerances merely to improve a benchmark unless that change is explicitly justified as part of the task.

Keep benchmark fixtures and policies explicit. If a benchmark uses a threshold, tolerance, baseline, or scenario catalog, the value should be named and documented as part of the benchmark contract.

## Running tests

Use the project scripts when possible:

```bash
./scripts/test-python-linux.sh .venv-linux -- [pytest arguments]
./scripts/test-rust-linux.sh .venv-linux -- [nextest arguments]
```

On Windows, use:

```powershell
./scripts/test-python-win.ps1 .venv -- [pytest arguments]
./scripts/test-rust-win.ps1 .venv -- [nextest arguments]
```
