# CI Tiered Execution Strategy

This guide defines the regression and benchmark CI tiers for Initiative 004 Project 004.

## Tier overview

| Tier | Workflow | Trigger policy | Expected runtime budget | Merge requirement |
| --- | --- | --- | --- | --- |
| Fast deterministic regression | `.github/workflows/ci-fast-regression.yml` | `pull_request`, `push` to `main`, manual dispatch | Target <= 30 minutes on `ubuntu-latest` | Required |
| Benchmark smoke tier | `.github/workflows/ci-benchmark-tier.yml` | Weekday schedule (`0 6 * * 1-5`) and manual dispatch | Target <= 60 minutes on `ubuntu-latest` | Not required on every commit |

## Fast deterministic regression tier

- Runs deterministic and tolerance regression checks:
  - `cargo test -p janiform-core --test deterministic_regression_harness`
  - `cargo test -p janiform-core --test coarse_fine_consistency_regression_harness`
- Runs CI workflow lint and smoke coverage:
  - `pytest ci/tests/test_ci_tiered_execution_strategy.py`
- Emits machine-readable command results and parsed output:
  - `.artifacts/fast-tier/fast_tier_results.json`
  - `.artifacts/fast-tier/parsed_results.json`

## Benchmark smoke tier

- Runs benchmark smoke tests for schema and artifact-hook stability:
  - `cargo test -p janiform-core --test oracle_pricer_risk_benchmark_harness -- --nocapture`
  - `cargo test -p janiform-core --test performance_baseline_capture_and_storage -- --nocapture`
- Publishes benchmark logs and artifact index as uploaded workflow artifacts.

The benchmark smoke tier does not claim production-grade performance representativeness by itself.
Its purpose is to keep benchmark contracts wired and observable without making merge checks slow.

## Failure diagnostics and links

Both tiers generate `.artifacts/*/benchmark_artifact_index.md` and `.json` using
`scripts/ci/write_benchmark_artifact_index.py`. The index includes:

- run artifacts page URL
- scenario catalog link
- performance baseline link

This index is written to the workflow summary and echoed on failure so triage has direct scenario
and baseline context.
