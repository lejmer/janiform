# Architecture Overview

Janiform is a mixed Python and Rust workspace. The Python package owns the public Python API and
Python-facing workflow adapters. The Rust workspace owns the native command shell, numerical
engines, shared contracts, and most performance-sensitive implementation.

## High-Level Shape

| Area | Role |
| --- | --- |
| `python/janiform/` | Public Python package, CLI assembly, workflow adapters |
| `crates/janiform/` | Umbrella Rust facade |
| `crates/janiform-pybind/` | PyO3 boundary crate |
| `crates/janiform-cli/` | Native Rust command shell |
| `crates/janiform-cli-ui/` | Shared semantic terminal rendering for the command shell |
| `crates/janiform-event-log/` | Run event logs, latest-state snapshots, and replay support |
| Engine crates | Pricing, sufficient-statistics, and simulation runtime implementations |
| Calibration crates | Calibration orchestration and artifact persistence |
| Analytics, validation, and governance crates | Analytical reports, regression harnesses, and benchmark governance |
| Worker and support crates | Process-isolated worker binaries and shared test fixtures |

## Important Workspace Crates

| Crate | Responsibility |
| --- | --- |
| `janiform` | Umbrella Rust facade |
| `janiform-affine-oracle` | Shared affine-oracle solver contracts and implementation |
| `janiform-analytics-stylized-facts` | Stylized-fact analytics |
| `janiform-calibration-artifacts` | Calibration artifact contracts and persistence |
| `janiform-calibration-runtime` | Calibration runtime orchestration |
| `janiform-cli` | Native Rust command shell with shared configuration, output, path, and diagnostic boundary |
| `janiform-cli-ui` | Semantic terminal rendering and progress adapters for the command shell |
| `janiform-engine-pricing` | Pricing runtime engine |
| `janiform-engine-simulation` | Simulation runtime engine |
| `janiform-engine-sufficient-statistics` | Sufficient-statistics runtime engine |
| `janiform-event-log` | Structured run event logs, latest-state snapshots, and replay support |
| `janiform-foundations` | Low-level shared foundation primitives |
| `janiform-governance` | Governance and regression orchestration |
| `janiform-governance-bench` | Benchmark governance support |
| `janiform-joint-refinement-worker` | Process-isolated joint-refinement summary simulation worker |
| `janiform-market-time` | Shared market-time infrastructure |
| `janiform-model-kernels` | Shared model-kernel domains |
| `janiform-pybind` | Python binding boundary |
| `janiform-test-support` | Shared Rust test support for workspace fixtures |
| `janiform-validation` | Validation domains and regression harnesses |

## Documentation Layout

Human-facing documentation lives in `docs/`. Local `README.md` files describe their own directory. Machine-readable workflow inputs, fixture payloads, and policy artifacts live under tests, fixtures, or data directories rather than under `docs/`.

## Archived Design Material

Theory and archived design notes that are still referenced by the code live under `design/`:

- `design/janiform_compact_model_spec.tex`
- `design/archive/janiform_design_brainstorm_notes.md`
