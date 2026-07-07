# Installation

Install Janiform from the artifacts attached to the
[latest GitHub Release](https://github.com/lejmer/janiform/releases/latest).

The Python wheel installs the importable package and the `janiform` command-line tool.
Standalone CLI archives are also published for shell workflows that do not need Python.

## Requirements

- Windows or Linux
- Python 3.12 or newer for Python package workflows
- A virtual environment for Python package workflows

## Python package and CLI

Download the wheel that matches your operating system and Python version from the
GitHub Release, then create and activate a virtual environment. This keeps
Janiform and its numerical dependencies separate from the rest of your Python
installation.

Linux or WSL:

```bash
python -m venv .venv
source .venv/bin/activate
python -m pip install ./path/to/janiform-<version>-<platform>.whl
```

Windows PowerShell:

```powershell
py -m venv .venv
.\.venv\Scripts\Activate.ps1
python -m pip install .\path\to\janiform-<version>-<platform>.whl
```

The package installation adds `janiform` to the active environment. Run it from the same shell where the environment is activated:

```bash
janiform --help
```

The CLI and Python package are two ways to run the same Janiform workflows. The CLI fits repeatable shell runs, automation, and saved artifacts. The Python library fits notebooks, research scripts, and larger applications.

## Standalone CLI archive

Download the standalone archive for your operating system when you only need the
`janiform` executable.

Linux:

```bash
tar -xzf janiform-cli-<version>-x86_64-unknown-linux-gnu.tar.gz
./janiform-cli-<version>-x86_64-unknown-linux-gnu/janiform --help
```

Windows PowerShell:

```powershell
Expand-Archive .\janiform-cli-<version>-x86_64-pc-windows-msvc.zip -DestinationPath .\janiform-cli
.\janiform-cli\janiform-cli-<version>-x86_64-pc-windows-msvc\janiform.exe --help
```

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

Reinstall the downloaded wheel in the active environment:

```bash
python -m pip install --force-reinstall ./path/to/janiform-<version>-<platform>.whl
```

This usually means the package was installed with one Python interpreter while the shell is using another environment.

### `pip` cannot find Janiform by package name

Janiform release artifacts are attached to GitHub Releases. Download the wheel
from the release page and pass the wheel path to `python -m pip install`.

## Next steps

- [Introduction](introduction.md)
- [First model](first-model.md)
