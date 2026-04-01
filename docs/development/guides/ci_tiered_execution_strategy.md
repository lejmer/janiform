# CI Tiered Execution Strategy

This guide defines the regression and benchmark CI tiers for Initiative 004 Project 004.

## Tier overview

| Tier | Workflow | Trigger policy | Expected runtime budget | Merge requirement |
| --- | --- | --- | --- | --- |
| Fast deterministic regression | `.github/workflows/ci-fast-regression.yml` | `pull_request`, `push` to `main`, manual dispatch | Target <= 30 minutes on `ubuntu-latest` | Required |
| Rust benchmark governance smoke | `.github/workflows/ci-rust-benchmark-governance-smoke.yml` | Manual dispatch | Target <= 60 minutes on `ubuntu-latest` | Not required on every commit |
| Local Python benchmark workloads | `scripts/python_benchmark_workloads.py` | Local only | Runner-dependent | Never required on every commit |

## Fast deterministic regression tier

- Runs deterministic and tolerance regression checks:
  - `cargo test -p janiform-governance --test deterministic_regression_harness`
  - `cargo test -p janiform-validation --test coarse_fine_consistency_regression_harness`
  - `cargo test -p janiform-governance-bench --test pricing_benchmark_validation_regression`
- Runs CI workflow lint and smoke coverage:
  - `pytest ci/tests/test_ci_tiered_execution_strategy.py`
- Emits machine-readable command results and parsed output:
  - `.artifacts/fast-tier/fast_tier_results.json`
  - `.artifacts/fast-tier/parsed_results.json`

## Rust benchmark governance smoke

- Runs benchmark smoke tests for schema and artifact-hook stability:
  - `cargo test -p janiform-governance-bench --test benchmark_scenario_catalog -- --nocapture`
  - `cargo test -p janiform-governance-bench --test oracle_pricer_risk_benchmark_harness -- --nocapture`
  - `cargo test -p janiform-governance --test performance_baseline_capture_and_storage -- --nocapture`
  - `cargo test -p janiform-governance --test regression_drift_attribution_reports -- --nocapture`
- Publishes benchmark logs and artifact index as uploaded workflow artifacts.

The Rust benchmark governance smoke tier does not claim production-grade performance representativeness by itself.
Its purpose is to keep benchmark contracts wired and observable without making merge checks slow.

## Local Python benchmark workloads

Run the Python benchmark workloads locally with:

```bash
python3 scripts/python_benchmark_workloads.py
```

That script drives:

- the representative real-data seeded runtime benchmark comparison against the checked-in baseline
- the Rust Criterion bench discovery smoke command
- the Python `pytest-benchmark` smoke run with benchmark timing disabled

It writes artifacts under `.artifacts/local-python-benchmarks/` by default.

For the real-data seeded runtime path, `native_estimated_retained_bytes` is currently documented and
enforced as an estimated retained-memory proxy. It is not a portable peak-RSS measurement.

## Failure diagnostics and links

Both tiers generate `.artifacts/*/benchmark_artifact_index.md` and `.json` using
`scripts/ci/write_benchmark_artifact_index.py`. The index includes:

- run artifacts page URL
- scenario catalog link
- performance baseline link

This index is written to the workflow summary and echoed on failure so triage has direct scenario
and baseline context.
