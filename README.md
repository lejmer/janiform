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
- `tests/`: Python tests and shared fixtures
- `docs/`: Design and development documentation

## Quickstart

```bash
maturin develop
python3 -m janiform --help
janiform --help
```

## Documentation

- Design model and rationale: `docs/design/janiform_design_brainstorm_notes.md`
- Calendar and seasonality API contracts: `docs/development/guides/calendar_and_seasonality_api_contracts.md`
- Python coding conventions: `docs/development/guides/python_style_guide.md`
- Planning artifacts: `docs/planning/`

The detailed calendar and seasonality sections were moved out of this README into the
development guide above to keep the repo root documentation concise.

## Developer Commands

```bash
python3 -m py_compile python/janiform/*.py tests/conftest.py
cargo check --workspace
```
