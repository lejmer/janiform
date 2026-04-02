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

## CI Workflows

The shared GitHub workflows are:

- `ci-fast-regression`
- `ci-rust-benchmark-governance-smoke`

Use the local scripts for routine verification. The GitHub workflows are the shared automation layer, not the only supported way to validate a change.
