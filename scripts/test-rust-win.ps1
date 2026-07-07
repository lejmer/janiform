param(
    [Parameter(Position=0)]
    [string]$VenvDir = ".venv-win",

    [Parameter(Position=1, ValueFromRemainingArguments=$true)]
    [string[]]$CargoArgs
)

$ErrorActionPreference = "Stop"

function Have-Cmd {
    param([string]$Cmd)
    return [bool](Get-Command $Cmd -ErrorAction SilentlyContinue)
}

function Have-Nextest {
    & cargo nextest --version *> $null
    return $LASTEXITCODE -eq 0
}

function Activate-Venv {
    $activate = Join-Path $VenvDir "Scripts\Activate.ps1"
    if (-not (Test-Path $activate)) {
        Write-Host "Error: virtual environment not found at '$VenvDir'."
        Write-Host "Run the bootstrap script first."
        exit 1
    }

    . $activate
}

Write-Host "Using virtual environment: $VenvDir"

Activate-Venv

if (Have-Cmd "uv") {
    $env:UV_PROJECT_ENVIRONMENT = $VenvDir
}

if ($null -eq $CargoArgs -or $CargoArgs.Count -eq 0) {
    if (Have-Nextest) {
        cargo nextest run
    }
    else {
        cargo test
    }
}
else {
    if (Have-Nextest) {
        cargo nextest run @CargoArgs
    }
    else {
        cargo test @CargoArgs
    }
}
