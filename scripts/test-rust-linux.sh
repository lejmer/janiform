#!/usr/bin/env bash
set -euo pipefail

# This test script assumes:
# - Current dir is repo root
# - Rust already installed
# - A venv already exists
# - The project has already been bootstrapped

usage() {
    echo "Usage: $0 [venv_dir] [-- cargo_args...]"
    exit 1
}

have_cmd() {
    command -v "$1" >/dev/null 2>&1
}

have_nextest() {
    cargo nextest --version >/dev/null 2>&1
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
CARGO_ARGS=()

if [ "$#" -gt 0 ]; then
    if [ "$1" = "--" ]; then
        shift
        CARGO_ARGS=("$@")
    else
        VENV_DIR="$1"
        shift

        if [ "$#" -gt 0 ]; then
            if [ "$1" != "--" ]; then
                usage
            fi
            shift
            CARGO_ARGS=("$@")
        fi
    fi
fi

echo "Using virtual environment: ${VENV_DIR}"

activate_venv

if have_cmd uv; then
    export UV_PROJECT_ENVIRONMENT="${VENV_DIR}"
fi

export TMPDIR=/tmp

if have_nextest; then
    if [ "${#CARGO_ARGS[@]}" -eq 0 ]; then
        cargo nextest run
    else
        cargo nextest run "${CARGO_ARGS[@]}"
    fi
else
    if [ "${#CARGO_ARGS[@]}" -eq 0 ]; then
        cargo test
    else
        cargo test "${CARGO_ARGS[@]}"
    fi
fi
