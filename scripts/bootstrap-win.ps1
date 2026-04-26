param(
    [Parameter(Position=0)]
    [string]$VenvDir = ".venv-win"
)

$ErrorActionPreference = "Stop"

# This bootstrapping script assumes:
# - Current dir is repo root
# - Rust already installed
# - Either uv or Python is installed

if ($args.Count -gt 0) {
    Write-Host "Usage: $($MyInvocation.MyCommand.Name) [venv_dir]"
    exit 1
}

Write-Host "Using virtual environment: $VenvDir"

function Have-Cmd {
    param([string]$Cmd)
    return [bool](Get-Command $Cmd -ErrorAction SilentlyContinue)
}

function Activate-Venv {
    $activate = Join-Path $VenvDir "Scripts\Activate.ps1"
    . $activate
}

if (Have-Cmd "uv") {
    Write-Host "Bootstrapping using: uv"

    # Make uv treat the chosen venv as the project's environment
    $env:UV_PROJECT_ENVIRONMENT = $VenvDir

    uv venv $VenvDir
    Activate-Venv

    # Install project and dev extra into the chosen local venv
    uv sync --extra dev
}
elseif (Have-Cmd "python") {
    Write-Host "Bootstrapping using: python"

    python -m venv $VenvDir
    Activate-Venv

    # Just in case pip is missing in the venv for some reason
    python -m ensurepip --upgrade 2>$null
    python -m pip install --upgrade pip

    # Install project in editable mode with dev dependencies
    python -m pip install -e ".[dev]"
}
elseif (Have-Cmd "py") {
    Write-Host "Bootstrapping using: py launcher"

    py -m venv $VenvDir
    Activate-Venv

    # Just in case pip is missing in the venv for some reason
    python -m ensurepip --upgrade 2>$null
    python -m pip install --upgrade pip

    # Install project in editable mode with dev dependencies
    python -m pip install -e ".[dev]"
}
else {
    Write-Host "Error: neither 'uv' nor a compatible Python interpreter was found."
    Write-Host "Install either uv or Python >= 3.12, then rerun."
    exit 1
}

# Build/install the Rust extension into the venv
python -m maturin develop

# Build the native Rust CLI executable
cargo build -p janiform-cli

# Smoke test
python -c "import janiform; print('janiform installed correctly.')"
