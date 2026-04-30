# Installation

Install Janiform as a Python package. The `janiform` command-line tool is installed with it.

## Requirements

- Python 3.12 or newer
- A virtual environment for project work
- Windows or Linux

## Python package

Create and activate a virtual environment first. This keeps Janiform and its numerical dependencies separate from the rest of your Python installation.

Linux or WSL:

```bash
python -m venv .venv
source .venv/bin/activate
python -m pip install janiform
```

Windows PowerShell:

```powershell
py -m venv .venv
.\.venv\Scripts\Activate.ps1
python -m pip install janiform
```

## Command line interface

The package installation adds `janiform` to the active environment. Use it from the same shell where the environment is activated:

```bash
janiform --help
```

The CLI and Python package are two ways to run the same Janiform workflows. Use the CLI for repeatable shell runs, automation, and saved artifacts. Use Python when Janiform is part of a notebook, research script, or larger application.

## Verify installation

Check the CLI:

```bash
janiform --help
```

Check the Python package:

```bash
python -c "import janiform; print(janiform.__version__)"
```

If both commands succeed, the environment is ready for the first model walkthrough.

## Troubleshooting

### `janiform` is not found

Activate the environment that contains Janiform, then run `janiform --help` again. If the command is still missing, check that `python -m pip show janiform` reports the package from the same environment.

### Python imports Janiform, but the CLI is missing

Reinstall Janiform in the active environment:

```bash
python -m pip install --force-reinstall janiform
```

This usually means the package was installed with one Python interpreter while the shell is using another environment.

## Next steps

- [What Janiform does](what-janiform-does.md)
- [First model](first-model.md)
