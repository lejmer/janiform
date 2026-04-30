# Installation

Install Janiform from a source checkout with the bootstrap scripts below. They create a virtual environment, install Python dependencies, and build the PyO3 extension.

## Requirements

| Tool | Required version |
| --- | --- |
| Python | 3.12 or newer |
| Rust | 1.89 or newer |
| Optional | `uv` for environment management |

## Linux

```bash
./scripts/bootstrap-linux.sh .venv-linux
source .venv-linux/bin/activate
janiform --help
```

## Windows

```powershell
./scripts/bootstrap-win.ps1 .venv-win
. .\.venv-win\Scripts\Activate.ps1
janiform --help
```

## Rebuild After Changes

Use the build scripts when you change Rust or Python code and want to refresh the installed extension.

### Linux

```bash
./scripts/build-linux.sh .venv-linux
```

### Windows

```powershell
./scripts/build-win.ps1 .venv-win
```
