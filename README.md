# Janiform

Janiform is a calibrator and generator of a new affine Hawkes–NIG model with coupled news gaps.
The Rust core provides the computation engine.
The Python package exposes an importable API and CLI via PyO3 bindings.

## Baseline Support

- Python: `>=3.12`
- Rust: `>=1.89`

## Tooling

- Build backend: `maturin`
- Python-Rust bindings: `PyO3`

## Project Layout

- `crates/core/`: Rust core library
- `crates/pybind/`: PyO3 extension module (`janiform._native`)
- `python/janiform/`: Python package and CLI entrypoints
- `tests/python/`: Python tests and shared fixtures
- `tests/rust/`: Rust tests
- `docs/`: Design and development documentation

## Quickstart

```bash
maturin develop
python -m janiform --help
janiform --help
```

## Developer Commands

```bash
python -m py_compile python/janiform/*.py tests/python/conftest.py
cargo check --workspace
```
