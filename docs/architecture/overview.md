# Architecture Overview

Janiform is a mixed Python and Rust workspace. The Python package owns the user-facing API and CLI. The Rust workspace owns the numerical engines, shared contracts, and most performance-sensitive implementation.

## High-Level Shape

| Area | Role |
| --- | --- |
| `python/janiform/` | Public Python package, CLI assembly, workflow adapters |
| `crates/janiform/` | Umbrella Rust facade |
| `crates/janiform-pybind/` | PyO3 boundary crate |
| Engine crates | Pricing, risk, and simulation runtime implementations |
| Calibration crates | Calibration orchestration and artifact persistence |
| Validation and governance crates | Regression harnesses and benchmark governance |

## Important Workspace Crates

| Crate | Responsibility |
| --- | --- |
| `janiform` | Umbrella Rust facade |
| `janiform-pybind` | Python binding boundary |
| `janiform-engine-pricing` | Pricing engine |
| `janiform-engine-risk` | Risk engine |
| `janiform-engine-simulation` | Simulation engine |
| `janiform-calibration-runtime` | Calibration orchestration |
| `janiform-calibration-artifacts` | Calibration artifact contracts and persistence |
| `janiform-validation` | Validation domains and regression harnesses |
| `janiform-foundations` | Shared low-level primitives |
| `janiform-model-kernels` | Shared model-kernel domains |

## Documentation Layout

Human-facing documentation lives in `docs/`. Local `README.md` files describe their own directory. Machine-readable workflow inputs, fixture payloads, and policy artifacts live under tests, fixtures, or data directories rather than under `docs/`.

## Archived Design Material

Theory and archived design notes that are still referenced by the code live under `design/`:

- `design/janiform_compact_model_spec.tex`
- `design/archive/janiform_design_brainstorm_notes.md`
