#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

# Keep Rust build outputs OS-specific (prevents collisions with Windows builds)
export CARGO_TARGET_DIR="target-wsl"

# Optional override for developers:
# export VENV_WSL=/path/to/.venv
venv="${VENV_WSL:-}"

pick_python() {
  local v="$1"
  if [[ -n "$v" && -x "$v/bin/python" ]]; then
    echo "$v/bin/python"
    return 0
  fi
  return 1
}

if py="$(pick_python "$venv")"; then
  "$py" -m maturin develop
elif [[ -x ".venv-wsl/bin/python" ]]; then
  .venv-wsl/bin/python -m maturin develop
elif [[ -x ".venv/bin/python" ]]; then
  .venv/bin/python -m maturin develop
else
  uv run maturin develop
fi
