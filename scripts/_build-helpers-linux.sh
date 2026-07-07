# Shared helper functions for bootstrap/build scripts.
# This file is sourced by script entrypoints and is not intended to run directly.

have_cmd() {
    command -v "$1" >/dev/null 2>&1
}

activate_venv() {
    # shellcheck disable=SC1091
    source "${VENV_DIR}/bin/activate"
}

activate_existing_venv() {
    if [ ! -f "${VENV_DIR}/bin/activate" ]; then
        echo "Error: virtual environment not found at '${VENV_DIR}'."
        echo "Run the bootstrap script instead."
        exit 1
    fi

    activate_venv
}

install_pip_tools() {
    # Just in case pip is missing in the venv for some reason
    python -m ensurepip --upgrade || true
    python -m pip install --upgrade pip
}

install_editable_with_pip() {
    install_pip_tools
    python -m pip install -e ".[dev]"
}

install_release_build_tool_with_pip() {
    install_pip_tools
    python -m pip install "maturin>=1.9,<2.0"
}

build_release_wheel() {
    local wheel_dir
    wheel_dir="$(mktemp -d)"

    python -m maturin build --release --out "${wheel_dir}"

    local wheel_paths
    wheel_paths=("${wheel_dir}"/*.whl)
    if [ "${#wheel_paths[@]}" -ne 1 ] || [ ! -f "${wheel_paths[0]}" ]; then
        echo "Error: expected exactly one wheel in '${wheel_dir}'."
        exit 1
    fi

    if [ "${USE_UV_PACKAGE_INSTALLER}" = true ]; then
        uv pip install --reinstall --no-deps "${wheel_paths[0]}"
    elif [ "${SKIP_WHEEL_DEPENDENCY_INSTALL}" = true ]; then
        python -m pip install --force-reinstall --no-deps "${wheel_paths[0]}"
    else
        python -m pip install --force-reinstall "${wheel_paths[0]}"
    fi

    rm -rf "${wheel_dir}"
}

build_release_package_and_cli() {
    build_release_wheel

    cargo build -p janiform-cli --release
}

build_editable_package_and_cli() {
    python -m maturin develop

    cargo build -p janiform-cli
}
