$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $PSCommandPath
. (Join-Path $ScriptDir "_build-helpers-win.ps1")

# This bootstrapping script assumes:
# - Current dir is repo root
# - Rust already installed
# - Either uv or Python is installed

function Show-Usage {
    Write-Host "Usage: $($MyInvocation.MyCommand.Name) [--release-wheel|--release-wheel-dev] [venv_dir]"
}

$ReleaseWheel = $false
$ReleaseWheelDev = $false
$SkipWheelDependencyInstall = $false
$UseUvPackageInstaller = $false
$VenvDir = ".venv-win"
$venvDirSet = $false

foreach ($argumentValue in $args) {
    if ($argumentValue -eq "--release-wheel-dev" -or $argumentValue -eq "-ReleaseWheelDev") {
        $ReleaseWheel = $true
        $ReleaseWheelDev = $true
    }
    elseif ($argumentValue -eq "--release-wheel" -or $argumentValue -eq "-ReleaseWheel") {
        $ReleaseWheel = $true
    }
    elseif ($argumentValue -eq "--help" -or $argumentValue -eq "-Help" -or $argumentValue -eq "-h") {
        Show-Usage
        exit 0
    }
    elseif ($argumentValue.StartsWith("-")) {
        Write-Host "Error: unknown option '$argumentValue'."
        Show-Usage
        exit 1
    }
    elseif ($venvDirSet) {
        Show-Usage
        exit 1
    }
    else {
        $VenvDir = $argumentValue
        $venvDirSet = $true
    }
}

Write-Host "Using virtual environment: $VenvDir"
if ($ReleaseWheelDev) {
    Write-Host "Using package build mode: release wheel with dev dependencies"
}
elseif ($ReleaseWheel) {
    Write-Host "Using package build mode: release wheel"
}
else {
    Write-Host "Using package build mode: editable"
}

if (Have-Cmd "uv") {
    Write-Host "Bootstrapping using: uv"
    $UseUvPackageInstaller = $true

    # Make uv treat the chosen venv as the project's environment
    $env:UV_PROJECT_ENVIRONMENT = $VenvDir

    uv venv $VenvDir
    Activate-Venv

    if ($ReleaseWheelDev) {
        $SkipWheelDependencyInstall = $true
        uv sync --extra dev --no-install-project
    }
    elseif ($ReleaseWheel) {
        $SkipWheelDependencyInstall = $true
        uv sync --no-install-project
        uv pip install "maturin>=1.9,<2.0"
    }
    else {
        # Install project and dev extra into the chosen local venv.
        uv sync --extra dev
    }
}
elseif (Have-Cmd "python") {
    Write-Host "Bootstrapping using: python"

    python -m venv $VenvDir
    Activate-Venv

    if ($ReleaseWheelDev) {
        $SkipWheelDependencyInstall = $true
        Install-EditableWithPip
    }
    elseif ($ReleaseWheel) {
        Install-ReleaseBuildToolWithPip
    }
    else {
        # Install project in editable mode with dev dependencies.
        Install-EditableWithPip
    }
}
elseif (Have-Cmd "py") {
    Write-Host "Bootstrapping using: py launcher"

    py -m venv $VenvDir
    Activate-Venv

    if ($ReleaseWheelDev) {
        $SkipWheelDependencyInstall = $true
        Install-EditableWithPip
    }
    elseif ($ReleaseWheel) {
        Install-ReleaseBuildToolWithPip
    }
    else {
        # Install project in editable mode with dev dependencies.
        Install-EditableWithPip
    }
}
else {
    Write-Host "Error: neither 'uv' nor a compatible Python interpreter was found."
    Write-Host "Install either uv or Python >= 3.12, then rerun."
    exit 1
}

if ($ReleaseWheel) {
    Build-ReleasePackageAndCli
}
else {
    Build-EditablePackageAndCli
}

# Smoke test
python -c "import janiform; print('janiform installed correctly.')"
