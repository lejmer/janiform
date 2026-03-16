# CI Tiered Execution Strategy

This guide defines the regression and benchmark CI tiers for Initiative 004 Project 004.

## Tier overview

| Tier | Workflow | Trigger policy | Expected runtime budget | Merge requirement |
| --- | --- | --- | --- | --- |
| Fast deterministic regression | `.github/workflows/ci-fast-regression.yml` | `pull_request`, `push` to `main`, manual dispatch | Target <= 30 minutes on `ubuntu-latest` | Required |
| Benchmark smoke tier | `.github/workflows/ci-benchmark-tier.yml` | Weekday schedule (`0 6 * * 1-5`) and manual dispatch | Target <= 60 minutes on `ubuntu-latest` | Not required on every commit |
| Microbenchmark smoke | `.github/workflows/ci-benchmark-tier.yml` | Manual dispatch only | Target <= 30 minutes on `ubuntu-latest` | Never required on every commit |

## Fast deterministic regression tier

- Runs deterministic and tolerance regression checks:
  - `cargo test -p janiform --test deterministic_regression_harness`
  - `cargo test -p janiform --test coarse_fine_consistency_regression_harness`
  - `cargo test -p janiform --test pricing_benchmark_validation_regression`
- Runs CI workflow lint and smoke coverage:
  - `pytest ci/tests/test_ci_tiered_execution_strategy.py`
- Emits machine-readable command results and parsed output:
  - `.artifacts/fast-tier/fast_tier_results.json`
  - `.artifacts/fast-tier/parsed_results.json`

## Benchmark smoke tier

- Runs benchmark smoke tests for schema and artifact-hook stability:
  - `cargo test -p janiform --test benchmark_scenario_catalog -- --nocapture`
  - `cargo test -p janiform --test oracle_pricer_risk_benchmark_harness -- --nocapture`
  - `cargo test -p janiform --test performance_baseline_capture_and_storage -- --nocapture`
  - `cargo test -p janiform --test regression_drift_attribution_reports -- --nocapture`
- Runs the scheduled real-data seeded runtime benchmark workload:
  - `PYTHONPATH=python:. .venv-linux/bin/python scripts/ci/run_real_data_seeded_runtime_benchmark.py`
  - fixture: `tests/fixtures/benchmarks/real_data_seeded_runtime_benchmark_fixture.json`
  - policy: `tests/fixtures/benchmarks/real_data_seeded_runtime_benchmark_policy.json`
  - baseline: `tests/fixtures/benchmarks/real_data_seeded_runtime_baseline_v1.json`
- Publishes benchmark logs and artifact index as uploaded workflow artifacts.
- Publishes real-data seeded runtime benchmark artifacts:
  - `real_data_seeded_runtime_benchmark_report.json`
  - `real_data_seeded_runtime_benchmark_comparison.json`
  - `real_data_seeded_runtime_benchmark_summary.md`

The benchmark smoke tier does not claim production-grade performance representativeness by itself.
Its purpose is to keep benchmark contracts wired and observable without making merge checks slow.

For the real-data seeded runtime path, `native_estimated_retained_bytes` is currently documented and
enforced as an estimated retained-memory proxy. It is not a portable peak-RSS measurement.

## Microbenchmark smoke

- Validates Criterion bench discovery without treating timings as a merge gate:
  - `cargo bench -p janiform --bench rust_microbenchmarks -- --list`
- Validates the Python `pytest-benchmark` suite wiring without recording noisy timing artifacts:
  - `PYTHONPATH=python:. .venv-linux/bin/pytest --benchmark-disable benches/python/test_microbenchmark_tooling.py benches/python/python_microbenchmarks.py -q`

This job is manual-only. It exists so contributors can verify the microbenchmark suites
stay runnable in CI-like environments while keeping timing noise out of required pull-request
checks.

## Failure diagnostics and links

Both tiers generate `.artifacts/*/benchmark_artifact_index.md` and `.json` using
`scripts/ci/write_benchmark_artifact_index.py`. The index includes:

- run artifacts page URL
- scenario catalog link
- performance baseline link

This index is written to the workflow summary and echoed on failure so triage has direct scenario
and baseline context.
