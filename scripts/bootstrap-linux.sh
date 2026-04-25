#!/usr/bin/env bash
set -euo pipefail

# This bootstrapping script assumes:
# - Current dir is repo root
# - Rust already installed
# - Either uv or Python is installed

if [ "$#" -gt 1 ]; then
    echo "Usage: $0 [venv_dir]"
    exit 1
fi

VENV_DIR="${1:-.venv-linux}"

echo "Using virtual environment: ${VENV_DIR}"

have_cmd() {
    command -v "$1" >/dev/null 2>&1
}

activate_venv() {
    # shellcheck disable=SC1091
    source "${VENV_DIR}/bin/activate"
}

if have_cmd uv; then
    echo "Bootstrapping using: uv"

    # Make uv treat the chosen venv as the project's environment
    export UV_PROJECT_ENVIRONMENT="${VENV_DIR}"

    uv venv "${VENV_DIR}"
    activate_venv

    # Install project and dev extra into the chosen local venv
    uv sync --extra dev

elif have_cmd python3; then
    echo "Bootstrapping using: python3"

    python3 -m venv "${VENV_DIR}"
    activate_venv

    # Just in case pip is missing in the venv for some reason
    python -m ensurepip --upgrade || true
    python -m pip install --upgrade pip

    # Install project in editable mode with dev dependencies
    python -m pip install -e ".[dev]"

elif have_cmd python; then
    echo "Bootstrapping using: python"

    python -m venv "${VENV_DIR}"
    activate_venv

    # Just in case pip is missing in the venv for some reason
    python -m ensurepip --upgrade || true
    python -m pip install --upgrade pip

    # Install project in editable mode with dev dependencies
    python -m pip install -e ".[dev]"

else
    echo "Error: neither 'uv' nor a compatible Python interpreter was found."
    echo "Install either uv or Python >= 3.12, then rerun."
    exit 1
fi

# Build/install the Rust extension into the venv
python -m maturin develop

# Build the native Rust CLI executable
cargo build -p janiform-cli

# Smoke test
python -c "import janiform; print('janiform installed correctly.')"
