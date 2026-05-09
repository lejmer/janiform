#!/usr/bin/env bash
set -euo pipefail

# This bootstrapping script assumes:
# - Current dir is repo root
# - Rust already installed
# - Either uv or Python is installed

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck source=scripts/_build-helpers-linux.sh
source "${SCRIPT_DIR}/_build-helpers-linux.sh"

usage() {
    echo "Usage: $0 [--release-wheel] [venv_dir]"
}

RELEASE_WHEEL=false
USE_UV_PACKAGE_INSTALLER=false
VENV_DIR=".venv-linux"
VENV_DIR_SET=false

for ARGUMENT in "$@"; do
    case "${ARGUMENT}" in
        --release-wheel)
            RELEASE_WHEEL=true
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        -*)
            echo "Error: unknown option '${ARGUMENT}'."
            usage
            exit 1
            ;;
        *)
            if [ "${VENV_DIR_SET}" = true ]; then
                usage
                exit 1
            fi

            VENV_DIR="${ARGUMENT}"
            VENV_DIR_SET=true
            ;;
    esac
done

echo "Using virtual environment: ${VENV_DIR}"
if [ "${RELEASE_WHEEL}" = true ]; then
    echo "Using package build mode: release wheel"
else
    echo "Using package build mode: editable"
fi

if have_cmd uv; then
    echo "Bootstrapping using: uv"
    USE_UV_PACKAGE_INSTALLER=true

    # Make uv treat the chosen venv as the project's environment
    export UV_PROJECT_ENVIRONMENT="${VENV_DIR}"

    uv venv "${VENV_DIR}"
    activate_venv

    if [ "${RELEASE_WHEEL}" = true ]; then
        uv sync --no-install-project
        uv pip install "maturin>=1.9,<2.0"
    else
        # Install project and dev extra into the chosen local venv.
        uv sync --extra dev
    fi

elif have_cmd python3; then
    echo "Bootstrapping using: python3"

    python3 -m venv "${VENV_DIR}"
    activate_venv

    if [ "${RELEASE_WHEEL}" = true ]; then
        install_release_build_tool_with_pip
    else
        # Install project in editable mode with dev dependencies.
        install_editable_with_pip
    fi

elif have_cmd python; then
    echo "Bootstrapping using: python"

    python -m venv "${VENV_DIR}"
    activate_venv

    if [ "${RELEASE_WHEEL}" = true ]; then
        install_release_build_tool_with_pip
    else
        # Install project in editable mode with dev dependencies.
        install_editable_with_pip
    fi

else
    echo "Error: neither 'uv' nor a compatible Python interpreter was found."
    echo "Install either uv or Python >= 3.12, then rerun."
    exit 1
fi

if [ "${RELEASE_WHEEL}" = true ]; then
    build_release_package_and_cli
else
    build_editable_package_and_cli
fi

# Smoke test
python -c "import janiform; print('janiform installed correctly.')"
