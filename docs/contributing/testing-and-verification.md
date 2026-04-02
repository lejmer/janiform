# Testing And Verification

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

- `ci-fast-regression`
- `ci-rust-benchmark-governance-smoke`

Use the local scripts for routine verification. The GitHub workflows are the shared automation layer, not the only supported way to validate a change.
