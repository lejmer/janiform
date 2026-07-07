# Contributing

## Start here

- [docs/contributing/index.md](docs/contributing/index.md)
- [RELEASE.md](RELEASE.md)

## Local setup

### Linux and WSL

```bash
./scripts/bootstrap-linux.sh .venv-linux
./scripts/build-linux.sh .venv-linux
```

### Windows PowerShell

```powershell
./scripts/bootstrap-win.ps1 .venv
./scripts/build-win.ps1 .venv
```

## Verification

Run the relevant suites before opening a pull request.

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

## Change expectations

- Keep code, tests, fixtures, generated artifacts, and docs aligned in the same change.
- Prefer shared implementations to local duplicates.
- Keep the public Python and CLI surfaces intentional.
- Move machine-readable inputs and outputs into fixtures or test data rather than `docs/`.
- Update the relevant page in `docs/` when you change setup steps, public workflows, repository layout, or contributor expectations.
- Follow [public API contract maintenance](docs/contributing/public-api-contracts.md) when a change touches the supported public surface.
- When adding a Rust workspace crate, follow the new-crate checklist in [docs/contributing/crates.md](docs/contributing/crates.md).
- Run the owned generation scripts when you change preset sources, checked-in crate `README.md` files, public API snapshots, or benchmark smoke artifacts.

## Licensing

Janiform is licensed under `AGPL-3.0-only`. Contributions are made to the repository under that same license unless a separate written agreement says otherwise. See [LICENSE](LICENSE) and [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt).
