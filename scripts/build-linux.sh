#!/usr/bin/env bash
set -euo pipefail

# This build script assumes:
# - Current dir is repo root
# - Rust already installed
# - A venv already exists
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
    if [ ! -f "${VENV_DIR}/bin/activate" ]; then
        echo "Error: virtual environment not found at '${VENV_DIR}'."
        echo "Run the bootstrap script instead."
        exit 1
    fi

    # shellcheck disable=SC1091
    source "${VENV_DIR}/bin/activate"
}

activate_venv

if have_cmd uv; then
    echo "Building using: uv"
    export UV_PROJECT_ENVIRONMENT="${VENV_DIR}"
    uv sync --extra dev
else
    echo "Building using: python"
    python -m ensurepip --upgrade || true
    python -m pip install --upgrade pip
    python -m pip install -e ".[dev]"
fi

python -m maturin develop

cargo build -p janiform-cli

python -c "import janiform; print('janiform built correctly.')"
