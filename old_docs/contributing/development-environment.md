# Development Environment

## Prerequisites

- Python 3.12 or newer
- Rust 1.89 or newer
- Optional `uv`

## Bootstrap

### Linux and WSL

```bash
./scripts/bootstrap-linux.sh .venv-linux
```

### Windows PowerShell

```powershell
./scripts/bootstrap-win.ps1 .venv
```

## Build

### Linux and WSL

```bash
./scripts/build-linux.sh .venv-linux
```

### Windows PowerShell

```powershell
./scripts/build-win.ps1 .venv
```

## Smoke Check

After a successful bootstrap or rebuild, confirm the package imports and the CLI starts:

```bash
./.venv-linux/bin/python -m janiform --help
```
