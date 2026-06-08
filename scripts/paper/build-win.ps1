$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $PSCommandPath
$RepoRoot = Split-Path -Parent (Split-Path -Parent $ScriptDir)
. (Join-Path $RepoRoot "scripts\_build-helpers-win.ps1")

# This build script assumes:
# - Current dir is repo root
# - latexmk is already installed
# - A venv already exists
# - Either uv or Python is installed

function Show-Usage {
    Write-Host "Usage: $($MyInvocation.MyCommand.Name) [venv_dir] [-- latexmk_args...]"
}

$VenvDir = ".venv-win"
$LatexmkArgs = @()

if ($args.Count -gt 0) {
    if ($args[0] -eq "--") {
        if ($args.Count -gt 1) {
            $LatexmkArgs = @($args[1..($args.Count - 1)])
        }
    }
    elseif ($args[0] -eq "--help" -or $args[0] -eq "-Help" -or $args[0] -eq "-h") {
        Show-Usage
        exit 0
    }
    elseif ($args[0].StartsWith("-")) {
        Write-Host "Error: unknown option '$($args[0])'."
        Show-Usage
        exit 1
    }
    else {
        $VenvDir = $args[0]

        if ($args.Count -gt 1) {
            if ($args[1] -ne "--") {
                Show-Usage
                exit 1
            }

            if ($args.Count -gt 2) {
                $LatexmkArgs = @($args[2..($args.Count - 1)])
            }
        }
    }
}

Write-Host "Using virtual environment: $VenvDir"

Activate-ExistingVenv

if (Have-Cmd "uv") {
    Write-Host "Syncing paper dependencies using: uv"
    $env:UV_PROJECT_ENVIRONMENT = $VenvDir
    uv sync --extra paper
}
else {
    Write-Host "Syncing paper dependencies using: python"
    Install-PipTools
    python -m pip install -e ".[paper]"
}

New-Item -ItemType Directory -Force -Path "paper\build" | Out-Null

$LatexmkBaseArgs = @(
    "-pdf",
    "-interaction=nonstopmode",
    "-halt-on-error",
    "-outdir=paper/build"
)

latexmk @LatexmkBaseArgs @LatexmkArgs "paper/src/main.tex"
