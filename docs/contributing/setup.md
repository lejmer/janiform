# Set up a development environment

Janiform has a Python package, native Rust crates, and a Rust CLI. The bootstrap scripts create a local virtual environment, install the Python package with development dependencies, build the PyO3 extension with Maturin, and build the native CLI.

Run all commands from the repository root. The examples use `.venv` as the local virtual environment.

## Prerequisites

- Python 3.12 or newer
- Rust 1.89 or newer
- `cargo` on your `PATH`
- PowerShell (Windows)
- Optional `uv`

If `uv` is available, the scripts use it. Otherwise, they fall back to `venv` and `pip`.

## Bootstrap

Bootstrap the repo once after cloning it, or whenever you intentionally recreate the local environment.

### Linux

```bash
./scripts/bootstrap-linux.sh .venv
```

### Windows PowerShell

```powershell
./scripts/bootstrap-win.ps1 .venv
```

## Rebuild

Use the build scripts after pulling changes, switching branches, or editing Rust code that affects the Python extension or CLI. They reuse the existing virtual environment and refresh the editable install.

### Linux

```bash
./scripts/build-linux.sh .venv
```

### Windows PowerShell

```powershell
./scripts/build-win.ps1 .venv
```

## Release wheel mode

Pass `--release-wheel` to install Janiform from a locally built release wheel instead of using an editable install. This mode installs the wheel without development extras and builds the Rust CLI in release mode.

### Linux

```bash
./scripts/bootstrap-linux.sh --release-wheel .venv
./scripts/build-linux.sh --release-wheel .venv
```

### Windows PowerShell

```powershell
./scripts/bootstrap-win.ps1 --release-wheel .venv
./scripts/build-win.ps1 --release-wheel .venv
```

## Activate the environment

Activation is optional for the project scripts because they receive the environment path directly. It is still useful for editor integration, ad hoc Python commands, and local exploration.

### Linux

```bash
source .venv/bin/activate
```

### Windows PowerShell

```powershell
. .\.venv\Scripts\Activate.ps1
```

## Smoke check

After bootstrapping or rebuilding, check both sides of the boundary.

### Linux

```bash
./.venv/bin/python -c "import janiform; print(janiform.__name__)"
cargo run -p janiform-cli -- --help
```

### Windows PowerShell

```powershell
.\.venv\Scripts\python.exe -c "import janiform; print(janiform.__name__)"
cargo run -p janiform-cli -- --help
```

## Shared checkout

In some rare instances, like when doing agentic coding via a WSL sandbox on Windows, sometimes the same repository checkout is used from multiple platforms. In that case, give each platform its own virtual environment directory. For example, use `.venv-linux` with the Linux scripts and `.venv-win` with the Windows scripts.

## Run tests

For the full testing policy, see [Writing tests](writing-tests.md).

Use the project wrappers for normal test runs:

```bash
./scripts/test-python-linux.sh .venv -- [pytest arguments]
./scripts/test-rust-linux.sh .venv -- [nextest arguments]
```

```powershell
./scripts/test-python-win.ps1 .venv -- [pytest arguments]
./scripts/test-rust-win.ps1 .venv -- [nextest arguments]
```
