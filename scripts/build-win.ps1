param(
    [Parameter(Position=0)]
    [string]$VenvDir = ".venv-win"
)

$ErrorActionPreference = "Stop"

# This build script assumes:
# - Current dir is repo root
# - Rust already installed
# - A venv already exists
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
    if (-not (Test-Path $activate)) {
        Write-Host "Error: virtual environment not found at '$VenvDir'."
        Write-Host "Run the bootstrap script instead."
        exit 1
    }

    . $activate
}

Activate-Venv

if (Have-Cmd "uv") {
    Write-Host "Building using: uv"
    $env:UV_PROJECT_ENVIRONMENT = $VenvDir
    uv sync --extra dev
}
else {
    Write-Host "Building using: python"
    python -m ensurepip --upgrade 2>$null
    python -m pip install --upgrade pip
    python -m pip install -e ".[dev]"
}

python -m maturin develop

cargo build -p janiform-cli

python -c "import janiform; print('janiform built correctly.')"
