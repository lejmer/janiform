#!/usr/bin/env bash
set -euo pipefail

# This test script assumes:
# - Current dir is repo root
# - A venv already exists
# - The project has already been bootstrapped

usage() {
    echo "Usage: $0 [venv_dir] [-- pytest_args...]"
    exit 1
}

have_cmd() {
    command -v "$1" >/dev/null 2>&1
}

activate_venv() {
    if [ ! -f "${VENV_DIR}/bin/activate" ]; then
        echo "Error: virtual environment not found at '${VENV_DIR}'."
        echo "Run the bootstrap script first."
        exit 1
    fi

    # shellcheck disable=SC1091
    source "${VENV_DIR}/bin/activate"
}

VENV_DIR=".venv-linux"
PYTEST_ARGS=()

if [ "$#" -gt 0 ]; then
    if [ "$1" = "--" ]; then
        shift
        PYTEST_ARGS=("$@")
    else
        VENV_DIR="$1"
        shift

        if [ "$#" -gt 0 ]; then
            if [ "$1" != "--" ]; then
                usage
            fi
            shift
            PYTEST_ARGS=("$@")
        fi
    fi
fi

echo "Using virtual environment: ${VENV_DIR}"

activate_venv

# If uv exists, keep it aligned with the chosen venv in case pytest or plugins
# trigger uv-managed project behavior somewhere in the workflow.
if have_cmd uv; then
    export UV_PROJECT_ENVIRONMENT="${VENV_DIR}"
fi

if [ "${#PYTEST_ARGS[@]}" -eq 0 ]; then
    python -m pytest
else
    python -m pytest "${PYTEST_ARGS[@]}"
fi
