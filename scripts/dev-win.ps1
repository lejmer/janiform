$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)

# Keep Rust build outputs OS-specific (prevents collisions with WSL builds)
$env:CARGO_TARGET_DIR = "target-win"

# Optional override for devs who want a nonstandard venv dir
# $env:VENV_DIR = "C:\path\to\.venv"
$venvDir = $env:VENV_DIR
if ([string]::IsNullOrWhiteSpace($venvDir)) {
    $venvDir = ".venv"
}

# If a venv exists at VENV_DIR, force uv to use it.
# Otherwise, just use uv's project environment resolution.
if (Test-Path "$venvDir\Scripts\python.exe") {
    uv tool run --python "$venvDir\Scripts\python.exe" maturin develop
} else {
    uv tool run maturin develop
}
