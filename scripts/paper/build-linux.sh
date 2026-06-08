#!/usr/bin/env bash
set -euo pipefail

# This build script assumes:
# - Current dir is repo root
# - latexmk is already installed
# - A venv already exists
# - Either uv or Python is installed

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

# shellcheck source=scripts/_build-helpers-linux.sh
source "${REPO_ROOT}/scripts/_build-helpers-linux.sh"

usage() {
    echo "Usage: $0 [venv_dir] [-- latexmk_args...]"
}

VENV_DIR=".venv-linux"
LATEXMK_ARGS=()

if [ "$#" -gt 0 ]; then
    if [ "$1" = "--" ]; then
        shift
        LATEXMK_ARGS=("$@")
    elif [ "$1" = "-h" ] || [ "$1" = "--help" ]; then
        usage
        exit 0
    elif [[ "$1" == -* ]]; then
        echo "Error: unknown option '$1'."
        usage
        exit 1
    else
        VENV_DIR="$1"
        shift

        if [ "$#" -gt 0 ]; then
            if [ "$1" != "--" ]; then
                usage
                exit 1
            fi
            shift
            LATEXMK_ARGS=("$@")
        fi
    fi
fi

echo "Using virtual environment: ${VENV_DIR}"

activate_existing_venv

if have_cmd uv; then
    echo "Syncing paper dependencies using: uv"
    export UV_PROJECT_ENVIRONMENT="${VENV_DIR}"
    uv sync --extra paper
else
    echo "Syncing paper dependencies using: python"
    install_pip_tools
    python -m pip install -e ".[paper]"
fi

mkdir -p "paper/build"

latexmk \
    -pdf \
    -interaction=nonstopmode \
    -halt-on-error \
    -outdir=paper/build \
    "${LATEXMK_ARGS[@]}" \
    paper/src/main.tex
