# Shared helper functions for bootstrap/build scripts.
# This file is dot-sourced by script entrypoints and is not intended to run directly.

function Have-Cmd {
    param([string]$Cmd)
    return [bool](Get-Command $Cmd -ErrorAction SilentlyContinue)
}

function Activate-Venv {
    $activate = Join-Path $VenvDir "Scripts\Activate.ps1"
    . $activate
}

function Activate-ExistingVenv {
    $activate = Join-Path $VenvDir "Scripts\Activate.ps1"
    if (-not (Test-Path $activate)) {
        Write-Host "Error: virtual environment not found at '$VenvDir'."
        Write-Host "Run the bootstrap script instead."
        exit 1
    }

    Activate-Venv
}

function Install-PipTools {
    # Just in case pip is missing in the venv for some reason
    python -m ensurepip --upgrade 2>$null
    python -m pip install --upgrade pip
}

function Install-EditableWithPip {
    Install-PipTools
    python -m pip install -e ".[dev]"
}

function Install-ReleaseBuildToolWithPip {
    Install-PipTools
    python -m pip install "maturin>=1.9,<2.0"
}

function Build-ReleaseWheel {
    $wheelDir = Join-Path ([System.IO.Path]::GetTempPath()) "janiform-wheel-$([System.Guid]::NewGuid())"
    New-Item -ItemType Directory -Path $wheelDir | Out-Null

    python -m maturin build --release --out $wheelDir

    $wheelPaths = @(Get-ChildItem -Path $wheelDir -Filter "*.whl")
    if ($wheelPaths.Count -ne 1) {
        Write-Host "Error: expected exactly one wheel in '$wheelDir'."
        exit 1
    }

    if ($UseUvPackageInstaller) {
        uv pip install --reinstall --no-deps ($wheelPaths[0]).FullName
    }
    elseif ($SkipWheelDependencyInstall) {
        python -m pip install --force-reinstall --no-deps ($wheelPaths[0]).FullName
    }
    else {
        python -m pip install --force-reinstall ($wheelPaths[0]).FullName
    }

    Remove-Item -Recurse -Force $wheelDir
}

function Build-ReleasePackageAndCli {
    Build-ReleaseWheel

    cargo build -p janiform-cli --release
}

function Build-EditablePackageAndCli {
    python -m maturin develop

    cargo build -p janiform-cli
}
