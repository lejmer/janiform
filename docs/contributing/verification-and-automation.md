# Verification And Automation

Run the narrowest tests that prove your change, then widen coverage when the change touches shared contracts or runtime boundaries.

## Standard Test Scripts

### Linux and WSL

```bash
./scripts/test-python-linux.sh .venv-linux
./scripts/test-rust-linux.sh .venv-linux
```

### Windows PowerShell

```powershell
./scripts/test-python-win.ps1 .venv
./scripts/test-rust-win.ps1 .venv
```

## Targeted Runs

### Python

```bash
./scripts/test-python-linux.sh .venv-linux -- tests/test_cli_workflows.py
```

### Rust

```bash
./scripts/test-rust-linux.sh .venv-linux -- -p janiform-calibration-runtime staged_runner
```

## Generated Artifacts

Generated crate `README.md` files are checked in. Verify them with:

```bash
./.venv-linux/bin/python scripts/generate_crate_readmes.py --check
```

## Generation Scripts

With the repo environment active, run these after changing the source inputs they own, then review the checked-in outputs in the same change.

### Bundled presets

Regenerate the built-in preset bundle after editing files under `presets/sources/`:

```bash
python scripts/generate_bundled_presets.py
```

This rewrites the bundled preset artifacts under `python/janiform/calibration/_bundled_presets/generated/`.

### Crate `README.md` files

Regenerate or verify crate-local `README.md` files after changing crate descriptions, publish settings, or workspace membership:

```bash
python scripts/generate_crate_readmes.py
python scripts/generate_crate_readmes.py --check
```

### Public API contract snapshot

Update the checked-in Python public API snapshot after an intentional public-surface change:

```bash
python scripts/update_public_api_contract_snapshot.py
```

This rewrites `tests/fixtures/python_api_contracts/public_api_contract_snapshot.json`.

### Third-party notices

Regenerate the checked-in Rust third-party notices file after changing the bundled native dependency set:

```bash
python scripts/generate_third_party_notices.py
python scripts/generate_third_party_notices.py --check
```

This rewrites `THIRD_PARTY_NOTICES.txt`.

### Seeded runtime smoke report

Regenerate the checked-in seeded-runtime smoke report after changing the smoke fixture or the report contract:

```bash
python scripts/generate_real_data_seeded_runtime_smoke_report.py
```

This rewrites `tests/fixtures/benchmarks/real_data_seeded_runtime_smoke_report.json`.

## CI Workflows

The shared GitHub workflows are:

- `ci-pr-fast`
- `ci-platform`
- `ci-fast-regression`
- `ci-rust-benchmark-governance-smoke`
- `release-candidate`

Use the local scripts for routine verification. The GitHub workflows are the shared automation layer, not the only supported way to validate a change.

### Workflow Roles

- `ci-pr-fast`: fast repository-contract checks for generated artifacts, public API guardrails,
  seeded-runtime smoke artifacts, and CI workflow structure.
- `ci-platform`: Linux full verification plus Windows build and smoke coverage.
- `ci-fast-regression`: fast deterministic benchmark regression coverage on pull requests and
  `develop`.
- `ci-rust-benchmark-governance-smoke`: manual and reusable benchmark-governance validation for
  the Rust benchmark catalog, baseline capture wiring, and drift attribution reporting.
- `release-candidate`: manual release-candidate automation. Detailed release flow lives in
  `RELEASE.md`.

### Branch Protection

Protect `develop` with these required checks:

- `ci-pr-fast`
- `ci-platform`
- `ci-fast-regression`

Keep `ci-rust-benchmark-governance-smoke` non-blocking for pull requests because it is manual
and reusable by design. When you cut a `release/*` branch, apply the same gates there before
running the release-candidate workflow.

## Benchmark Workflows

Janiform keeps benchmark automation split by purpose so deterministic contract coverage and heavier
runtime measurement do not compete for the same CI budget.

### Benchmark Layers

- Fast deterministic regression:
  `ci-fast-regression` runs the governed fast tier on pull requests and `develop`.
- Governance smoke:
  `ci-rust-benchmark-governance-smoke` validates the Rust benchmark catalog, baseline capture
  wiring, and drift attribution reporting.
- Local benchmark development:
  `scripts/python_benchmark_workloads.py` runs the representative Python-facing benchmark paths
  that stay local-only to preserve GitHub Actions minutes.

### Local Benchmark Entry Points

- Run both local benchmark families:
  `python3 scripts/python_benchmark_workloads.py`
- Skip the representative runtime workload and inspect only the microbenchmark smoke path:
  `python3 scripts/python_benchmark_workloads.py --skip-real-data-seeded-runtime`
- Skip the microbenchmark smoke path and run only the representative runtime comparison:
  `python3 scripts/python_benchmark_workloads.py --skip-microbenchmark-smoke`
- Run the benchmark comparison entry point directly:
  `python3 scripts/ci/run_real_data_seeded_runtime_benchmark.py --fixture-path tests/fixtures/benchmarks/real_data_seeded_runtime_benchmark_fixture.json --policy-path tests/fixtures/benchmarks/real_data_seeded_runtime_benchmark_policy.json --baseline-path tests/fixtures/benchmarks/real_data_seeded_runtime_baseline_v1.json --output-dir .artifacts/local-real-data-runtime`

The representative real-data seeded runtime benchmark and the seeded-runtime smoke artifact refresh
stay local-only. They are intentionally excluded from GitHub Actions because the repository uses the
free GitHub Actions tier and must stay within a limited monthly minute budget.
