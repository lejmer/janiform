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
| `crates/janiform-event-log/` | Run event logs, latest-state snapshots, and replay support |
| `crates/janiform-terminal-ui/` | Shared semantic terminal rendering and progress adapters |
| `crates/janiform-workflows/` | Shared workflow orchestration for entrypoints |
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
| `janiform-terminal-ui` | Shared semantic terminal rendering and progress adapters |
| `janiform-test-support` | Shared Rust test support for workspace fixtures |
| `janiform-validation` | Validation domains and regression harnesses |
| `janiform-workflows` | Shared workflow orchestration for Janiform entrypoints |

## Terminal UX Boundary

Janiform keeps terminal UX ownership split by concern:

| Layer | Owns | Does not own |
| --- | --- | --- |
| `janiform-terminal-ui` | Generic terminal primitives: semantic styles, renderers, reusable summary/table blocks, progress adapters, path and duration formatting, and summary emission policy. | Fit, pricing, simulation, or other workflow-specific labels and rows. |
| `janiform-workflows` | Workflow execution UX: start summaries, progress labels, completion summaries, and the lifecycle points where those views are emitted. | CLI output-mode selection, argument parsing, Python object conversion, or terminal rendering mechanics. |
| `janiform-cli` | CLI shell behavior: command parsing, path resolution, output-mode selection, JSON command payload emission, and non-execution command screens such as fit planning and inspection. | Workflow execution start/progress/completion UX. |
| `janiform-pybind` and Python | Boundary conversion and typed public API ergonomics. | Terminal workflow UX and workflow execution logic. |

This keeps the reusable terminal design system independent from domain workflows while ensuring every entrypoint sees the same workflow execution UX. CLI and Python can choose whether human terminal output is enabled, but the workflows decide what a fit, pricing, or simulation run says when it starts, progresses, and completes.

### Workflow Module Details

Fit, pricing, and simulation workflow internals follow the crate-level module conventions in
[workflow-module-conventions.md](workflow-module-conventions.md).

### Fit Planning Screens

Fit plan, inspect, and staged-plan commands render pre-execution planning screens in `janiform-cli`. Those screens summarize command preparation and resolved inputs before a workflow run exists, so they are classified as CLI shell UX. Once fit execution starts, lifecycle terminal output is owned by `janiform-workflows`.

## Documentation Layout

Human-facing documentation lives in `docs/`. Local `README.md` files describe their own directory. Machine-readable workflow inputs, fixture payloads, and policy artifacts live under tests, fixtures, or data directories rather than under `docs/`.

## Archived Design Material

Theory and archived design notes that are still referenced by the code live under `design/`:

- `design/janiform_compact_model_spec.tex`
- `design/archive/janiform_design_brainstorm_notes.md`
