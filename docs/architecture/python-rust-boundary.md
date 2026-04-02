# Python And Rust Boundary

Janiform uses a thin Python control plane on top of a Rust data plane.

## Boundary Layout

- `python/janiform/` provides the importable package, task-oriented modules, CLI command tree, and lightweight workflow glue.
- `crates/janiform-pybind/` owns the PyO3 boundary.
- Rust crates under `crates/` own the engines, contracts, calibration runtime, and validation logic.

## Build Flow

The bootstrap and build scripts install Python dependencies and then run:

```bash
python -m maturin develop
```

That command builds the PyO3 extension and installs the resulting native module into the active virtual environment.

## Intentional Separation

- Python is the user-facing surface.
- Rust is the implementation surface for performance-sensitive and numerically sensitive code.
- Public workflow changes should normally be reflected first in Python module or CLI contracts, then delegated to the Rust core.
