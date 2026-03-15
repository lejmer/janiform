# CLI and Library First-use Workflow Usability Redesign

## Problem Statement

Janiform currently exposes internal workflow structure too directly in its first-use surfaces. The result is that basic commands do not behave the way users expect:

- CLI `fit` does not persist the fitted model by default.
- CLI persistence, checkpointing, and internal execution mechanics are mixed together.
- CLI `simulate` currently routes through JSON payloads, which does not scale to large path outputs.
- Human output is improving, but the command model still makes users think about implementation details instead of outcomes.
- The library and CLI do not yet make a clean distinction between in-memory workflows and durable artifact workflows.
- `simulate` semantics are currently more complicated than they need to be for a first-use path.
- `price` is not yet aligned with the same high-level workflow model as `fit` and `simulate`.

The redesign must make the product feel like a normal professional CLI and a normal professional Python library:

- CLI defaults should produce durable user-facing outputs.
- Library defaults should stay in memory unless the caller explicitly asks for persistence.
- `fit`, `simulate`, and `price` should feel like one coherent workflow family.
- `plan` and `staged` should remain first-class professional commands, not hidden or demoted.
- Both the CLI and the library should provide useful runtime feedback by default for slow operations.
- `simulate` and `price` should be high-level task-shaped workflows, not execution-input-first workflows.

## Pre-release Policy

Janiform is still pre-release and has no external users. This redesign should be implemented accordingly.

- Do not add migration paths.
- Do not add backward-compatibility shims.
- Do not preserve legacy aliases "just in case".
- Do not carry dual behavior paths forward when one canonical behavior is enough.
- Replace obsolete CLI and library behavior directly.
- Update in-repo call sites, tests, examples, and docs in the same change set.
- Keep all contracts and schema identifiers at `v1`.
- Do not bump versions just because the shape or semantics are changing.

## Product Principles

1. Default behavior must match user expectations.
2. CLI and library may have different defaults if that is what users normally expect from each surface.
3. Final output persistence, resumability, and debug artifact emission are separate concerns and must not be controlled by one overloaded knob.
4. Beginner paths and professional paths should share one coherent model.
5. Large simulation outputs must be streamed directly to their final file format. They must never be materialized as JSON for persistence.
6. Human-readable output should explain where artifacts are going before expensive work begins.
7. Pre-release freedom should be used to simplify the product, not to preserve accidental historical behavior.
8. Slow operations should provide continuous visible progress by default instead of leaving the user with no feedback.
9. Silence mode should be fully supported and should skip non-essential progress/reporting work rather than only hiding its rendering.

## Core UX Contract

### CLI defaults

The CLI is job-oriented. By default it should persist the primary result of a successful command.

- `janiform fit ...` saves a model and a fit report by default.
- `janiform simulate ...` saves simulation output by default.
- `janiform price ...` saves pricing output by default.
- Commands should print the resolved output location before work starts.
- Commands should print the saved artifact paths when they finish.
- Long-running commands should display progress feedback by default.

### Library defaults

The library is programmatic and in-memory-first. By default it should not persist anything.

- `JaniformEstimator.fit(...)` returns `JaniformModel` and does not write to disk unless explicitly requested.
- `JaniformModel.simulate(...)` returns simulation results and does not write to disk unless explicitly requested.
- `JaniformModel.price_option(...)` returns pricing results and does not write to disk unless explicitly requested.
- Long-running library calls should emit useful progress feedback by default unless silence mode is enabled.

### Silence and verbosity

Both surfaces should support two clear modes:

- default verbose mode for ad hoc and first-time usage
- explicit silence mode for production pipelines and automation

Behavioral requirements:

- verbose mode should be the default
- silence mode should suppress all non-error output
- silence mode should still emit errors and warnings
- silence mode should disable runtime progress-update work rather than merely hiding rendered output
- silence mode must be available from the CLI and the library public API

Recommended controls:

- CLI: `--silent`
- library: `silent: bool = False`

## Command and API Model

The following commands should remain first-class:

- `janiform fit`
- `janiform fit inspect`
- `janiform fit plan`
- `janiform fit staged`
- `janiform simulate`
- `janiform price`

The key change is not command removal. The key change is that the basic commands should behave like outcome-oriented commands, while the more explicit commands should remain available for professional workflow control.

## Decided Workflow Semantics

### Simulation interval model

`simulate` should use one simple model in both the CLI and the library:

- interval is required as a concept
- no interval means seeded tick simulation
- a supplied interval like `1min`, `15min`, `1h`, or `1D` means coarse simulation

This should replace the current user-facing need to think in terms of separate `ticks`, `coarse`, or `both` choices for the first-use path.

For professional workflows, richer explicit simulation controls may still exist internally, but the high-level public model should be interval-driven.

### Pricing model

`price` should be a high-level workflow just like `fit` and `simulate`.

The first-use path should be model-driven and user-facing, not execution-input-JSON-driven.

### Fit input sanity warning

When `fit` is given source data that appears to be interval data rather than tick data, Janiform should emit a warning.

The warning should make clear that:

- the input does not appear to be tick data
- fitting the model on interval data will likely produce an invalid fit

This warning should apply in both the CLI and the library verbose path. In silence mode it should still be emitted because it is a warning.

## Implementation Hygiene

This redesign should be used to clean the codebase up materially. The goal is not to layer a new UX on top of old implementation paths.

- Prefer adjusting and consolidating existing implementations over rewriting equivalent logic from scratch.
- When a current path already does most of what is needed, extend or reshape that path instead of introducing a parallel subsystem.
- Large refactors are allowed when they simplify the final design.
- Remove obsolete code paths as part of the redesign.
- Remove unused flags, helpers, contracts, rendering branches, and persistence pathways once callers are migrated.
- Do not leave dead compatibility code behind.
- Do not leave overlapping persistence or execution abstractions behind if one canonical abstraction is enough.
- The end state should be a smaller, clearer, more maintainable codebase.
- Spaghetti code is not an acceptable tradeoff for preserving temporary implementation convenience.

## Persistence Model

Persistence needs to be split into three distinct categories.

### 1. Final outputs

These are the artifacts a user normally expects to keep.

- fitted model
- fit report
- simulation output file
- pricing output file
- lightweight run manifest

### 2. Resume artifacts

These are only for continuation and recovery.

- checkpoints
- resumable execution state

### 3. Debug artifacts

These are explicit exports for troubleshooting or reproducibility.

- derived staged execution payloads
- translated market-data dumps
- verbose provenance/debug reports

Final outputs should be on by default in the CLI. Resume artifacts should be separately configurable. Debug artifacts should be opt-in.

## Output Location Design

Output location must be configurable in both the CLI and the library public API.

### CLI

Use user-facing path options instead of relying on hidden tempdirs or implicit discard semantics.

Recommended shape:

- `fit`: `--output-dir`
- `fit inspect`: optional `--output-dir` only for previewing resolved destinations, but should not write artifacts by default
- `simulate`: `--output-path`
- `price`: `--output-path`

If omitted, CLI commands should resolve sensible defaults automatically.

### Library

Use explicit public API parameters.

Recommended shape:

- `JaniformEstimator.fit(..., output_dir: str | Path | None = None, ...)`
- `JaniformModel.simulate(..., output_path: str | Path | None = None, ...)`
- `JaniformModel.price_option(..., output_path: str | Path | None = None, ...)`
- long-running library entry points should also expose a silence control through a consistent public parameter
- advanced streaming and batch-job use cases should also be supported through explicit writer or sink abstractions

Behavior:

- `None` means in-memory only.
- A provided path or directory enables explicit persistence.
- The method still returns the in-memory result object unless there is a strong reason not to.
- silence mode should suppress all non-error reporting and progress work

## Default Artifact Layout

### CLI fit

If the user does not provide `--output-dir`, default to:

```text
./.janiform/<run_id>/
```

Contents:

```text
model.json
fit_report.json
run.json
```

Do not emit bulky intermediate market-data payloads by default.

### CLI simulate

If the user does not provide `--output-path`, default to something under:

```text
./.janiform/<run_id>/
```

Examples:

```text
simulation.csv
simulation.parquet
simulation_report.json
run.json
```

The exact primary filename depends on the chosen or inferred output format.

The decided defaults are:

- seeded tick simulation with no interval:
  - `simulation.csv` or `simulation.parquet`
- coarse simulation with an interval:
  - `simulation_<interval>.csv` or `simulation_<interval>.parquet`

### CLI price

If the user does not provide `--output-path`, default to something under:

```text
./.janiform/<run_id>/
```

Examples:

```text
pricing.json
pricing_report.json
run.json
```

Pricing outputs are small enough that JSON is acceptable by default for the current pricing surface.

The decided default output layout is:

- `fit`: `./.janiform/<run_id>/`
- `simulate`: `./.janiform/<run_id>/simulation.<csv|parquet>` or interval-specific equivalent
- `price`: `./.janiform/<run_id>/pricing.json`

## Simulation Output Requirements

This area has stricter constraints than `fit` and `price`.

### Non-negotiable requirements

- Simulation outputs must never be persisted as JSON files.
- Simulation persistence must not be implemented as "collect everything in memory, then convert at the end".
- The persisted simulation file should match the structure the user expects from market-data files.
- If the fitted model came from a CSV or Parquet tick dataset, the default seeded tick simulation output should mirror that market-data shape as closely as possible.

### Default output format

For high-level seeded tick simulation:

- if the fit input was CSV, default output should be CSV
- if the fit input was Parquet, default output should be Parquet

This output preference should be carried forward from fit-time metadata stored in the model.

### Output schema fidelity

For seeded tick simulation outputs:

- preserve the calibrated timestamp column name
- preserve the calibrated price column naming shape
- preserve the calibrated timestamp format policy
- preserve the calibrated timezone/rendering convention

This requires the fitted model to retain enough input-schema metadata to reconstruct user-facing output files without guessing again.

### Streaming architecture

Simulation persistence should be designed around streaming writes.

Recommended architecture:

1. Rust/native simulation produces records in bounded batches.
2. The Python boundary exposes a sink-oriented writer interface rather than only a full in-memory result materialization path.
3. CSV and Parquet writers consume batches incrementally.
4. The CLI writes directly to the final target file during simulation.
5. The library may either:
   - return an in-memory result when no output path is provided, or
   - stream directly to disk when `output_path` is provided.

This avoids both:

- giant JSON payload generation
- giant end-of-run data conversion steps

### High-level versus explicit simulation

For the simple path:

- `simulate` should default to the user-facing seeded tick path when no interval is provided
- default persistence format should be CSV or Parquet, never JSON
- an explicit interval should select the coarse path

For professional workflows:

- coarse simulation output remains supported
- coarse outputs should also avoid JSON persistence for large path data
- if JSON remains in use for small metadata envelopes, the path data itself should still be written as row-oriented files

## Fit Command Semantics

### CLI fit

The beginner command should continue to be:

```bash
janiform fit eurusd_ticks.csv --market eurusd_spot
```

By default it should:

- infer market-data schema defaults
- fit the model
- save `model.json`
- save `fit_report.json`
- save `run.json`
- avoid saving bulky intermediate artifacts
- print the output directory and model path before work starts
- warn if the source data appears to be interval data rather than true tick data

### Library fit

The corresponding library flow should remain:

```python
model = JaniformEstimator(market_profile="eurusd_spot").fit(
    tick_data_path="eurusd_ticks.csv",
)
```

By default it should:

- infer market-data defaults
- return `JaniformModel`
- write nothing to disk

Explicit persistence should be enabled with `output_dir=...` or via `model.save(...)`.

## Simulate Command Semantics

### CLI simulate

The simple high-level path should save a simulation file by default.

Examples:

```bash
janiform simulate --model-path ./.janiform/run_123/model.json --start-time-utc ... --horizon-interval-count ... --seed 7
```

Behavior:

- no interval means seeded tick simulation
- an explicit interval means coarse simulation
- infer default output format from model metadata
- stream output directly to CSV or Parquet
- save a small simulation report alongside the main output
- print the destination before work starts

### Library simulate

The library should remain in-memory by default:

```python
result = model.simulate(...)
```

Explicit persistence:

```python
result = model.simulate(..., output_path="simulation.parquet")
```

If `output_path` is provided, the implementation should prefer direct streaming to disk over full in-memory accumulation.

The library API should support both:

- simple `output_path=` convenience usage
- advanced streaming or sink-oriented usage for batch workflows

## Price Command Semantics

### CLI price

`price` should follow the same durable-output pattern as the other top-level commands.

- save pricing result by default
- print where the pricing result will be saved before execution
- keep result format user-facing and predictable
- use a high-level model-driven command shape rather than requiring raw execution-input JSON for first use

For the current single-run pricing workflow, JSON remains acceptable because the result is small.

### Library price

The library should remain in-memory by default:

```python
pricing = model.price_option(...)
```

Explicit persistence:

```python
pricing = model.price_option(..., output_path="pricing.json")
```

Advanced writer or sink-oriented extensions are acceptable if needed, but the simple public path should remain straightforward.

## Human Output Design

All top-level CLI workflows should follow the same reporting shape.

## Progress Reporting

Slow operations should provide visible progress by default in both the CLI and the library.

### Requirements

- Use in-place TUI-style progress bars or equivalent in-place progress rendering.
- Do not use append-only "dot" progress or endlessly growing log spam.
- Standardize the progress implementation so all long-running stages can use the same mechanism.
- Progress code should support stage-level updates, elapsed time, and meaningful labels.
- The implementation should be reusable across fit, simulate, price, and any explicit staged workflows that need it.

### Silence-mode interaction

- In silence mode, progress bars must not be rendered.
- In silence mode, progress-update work should also not be performed if it is not required for correctness.
- The goal is to reduce overhead in production pipelines, not just hide terminal output.

### Library interaction

Library progress behavior should follow the same rules as the CLI:

- verbose by default
- fully silent when requested
- standardized progress plumbing instead of workflow-specific ad hoc prints
- default progress destination should be `stdout`
- caller-provided hooks or callbacks should not be suppressed by Janiform silence mode unless they are Janiform-owned

### Shared implementation

Progress and output plumbing should live in a shared implementation layer used by both the CLI and the library.

Requirements:

- do not duplicate progress-rendering logic
- do not duplicate silence checks throughout the codebase
- provide a shared Janiform-owned output helper that can no-op immediately when silence mode is enabled
- keep the progress bar implementation separate from plain text output helpers because it is materially different

### Before execution

Print:

- command summary
- run id
- input path or model path
- market or model context
- primary output location
- report location
- checkpoint policy

### On success

Print:

- primary saved artifact path
- report path
- run directory
- next useful command

### On failure

Print:

- run directory
- any saved artifacts
- whether resume is possible
- the resume command if applicable

### During execution

Render:

- current stage
- progress bar or equivalent in-place indicator
- elapsed time
- meaningful status text when stage-level progress can be estimated

When exact progress cannot be estimated, the command should still provide in-place activity feedback rather than remaining idle and silent.

## Checkpointing and Resume

Checkpointing should be treated as separate from final output persistence.

Recommended user-facing control:

- `checkpoint_policy = off | auto | on`

CLI defaults:

- likely `auto` for long-running workflows such as fit

Library defaults:

- `off` unless explicitly requested

Checkpointing should store only what is needed to resume. It should not imply emission of large debug artifacts.

## Professional Command Alignment

`fit plan` and `fit staged` should remain normal first-class commands.

The redesign work should align them with the same persistence model rather than hiding them:

- predictable output destinations
- clear distinction between final outputs and debug artifacts
- consistent path options
- consistent human output

The high-level and explicit paths should feel like different levels of control over the same system, not like unrelated product surfaces.

## Required Data Model Additions

To support the redesign, the fitted model and related result objects will need to retain more metadata than they do today.

### Fit-time metadata needed for simulation output

- source file format preference
- user-facing timestamp column name
- user-facing price column naming pattern
- timestamp formatting policy
- timezone rendering policy

This metadata should be part of the persisted model payload so that follow-on simulation commands can reconstruct user-facing outputs correctly.

### Result object persistence helpers

The following gaps should be addressed:

- `JaniformModel` already supports `save(...)`
- simulation result objects need explicit persistence helpers or sink-based APIs
- pricing result objects need explicit persistence helpers if we want a symmetric library UX
- fit reports should have an explicit persistence path if that does not already exist

## Rollout Phases

### Phase 1: Contract reset

- define the shared persistence model for CLI and library
- define path option and parameter names
- define run directory layout
- define simulation output-format rules
- identify current code paths that should be deleted or collapsed rather than adapted piecemeal
- define the shared verbosity and silence contract for CLI and library
- define the standardized progress-reporting abstraction
- define the interval-driven high-level simulation contract
- define the high-level pricing contract
- define the shared run-manifest and report-envelope contracts

### Phase 2: Fit redesign

- make CLI `fit` persist final outputs by default
- add explicit library `output_dir` parameter with default `None`
- update startup and completion rendering
- remove superseded fit persistence branches once the canonical path is in place
- add standardized progress reporting for long-running fit steps
- ensure silence mode bypasses progress-update work
- add interval-data detection and invalid-fit warning behavior

### Phase 3: Simulation redesign

- add streaming file sinks for seeded tick outputs
- persist seeded tick outputs directly to CSV or Parquet
- carry fit-time schema metadata into model persistence
- add library `output_path` support without forcing persistence by default
- remove JSON-based persistence paths for simulation outputs
- add standardized progress reporting for long-running simulation output generation and streaming
- replace the high-level `ticks` versus `coarse` choice with interval-driven semantics
- add advanced shared sink/writer support for batch workloads

### Phase 4: Pricing redesign

- align CLI `price` with durable-output defaults
- add explicit library `output_path` support
- standardize output reporting
- remove redundant pricing persistence glue if it becomes unnecessary
- apply the shared verbosity and silence model to pricing execution
- replace the first-use execution-input-JSON shape with a high-level model-driven pricing path

### Phase 5: Resume and checkpoint refinement

- separate checkpoint policy from final artifact persistence
- expose resume behavior clearly in CLI output
- ensure restart and failure behavior are predictable
- collapse any legacy overlap between checkpointing, export, and final-output persistence

### Phase 6: Documentation and examples

- rewrite quickstarts around realistic first-use workflows
- add examples for fit, simulate, and price using both CLI and library
- document the persistence split explicitly
- ensure docs describe only the final intended behavior, not transitional behavior
- document the interval-driven simulate contract and the tick-data fit warning explicitly

## Acceptance Criteria

The redesign is complete when all of the following are true.

### CLI

- `janiform fit ...` saves a model and fit report by default
- `janiform simulate ...` saves simulation output by default
- `janiform price ...` saves pricing output by default
- output location is configurable via explicit path options
- each command prints its resolved destination before expensive work starts
- simulation outputs are written as CSV or Parquet, never JSON
- simulation persistence uses streaming writes rather than end-of-run conversion
- bulky intermediate artifacts are not emitted by default
- CLI workflows are verbose by default and show in-place progress for slow operations
- `--silent` suppresses all non-error output and disables non-essential progress-update work
- high-level `simulate` uses no-interval equals ticks and explicit-interval equals coarse semantics
- high-level `price` is model-driven and does not require raw execution-input JSON for first use
- `fit` warns when the supplied data appears to be interval data rather than tick data

### Library

- `JaniformEstimator.fit(...)` remains in-memory by default
- `JaniformModel.simulate(...)` remains in-memory by default
- `JaniformModel.price_option(...)` remains in-memory by default
- each workflow supports explicit output-path or output-dir persistence parameters
- explicit persistence does not require awkward post-hoc manual wiring for common use cases
- long-running library workflows are verbose by default
- library silence mode suppresses all non-error output and disables non-essential progress-update work
- the library also supports advanced writer or sink-oriented persistence for streaming and batch use cases
- library progress defaults to `stdout`
- Janiform silence mode suppresses only Janiform-owned output

### Cross-surface consistency

- `fit`, `simulate`, and `price` share a coherent persistence model
- `fit inspect`, `fit plan`, and `fit staged` remain first-class and consistent with the same artifact model
- the product no longer makes first-time users guess where results went
- `run.json` is a shared lightweight manifest format across fit, simulate, and price
- fit, simulation, and pricing reports share one easy-to-read common envelope with workflow-specific sections
- progress and output plumbing are implemented in one shared reusable layer

## Immediate Recommendation

The implementation should start by resetting persistence semantics for `fit`, then use that same model for `simulate` and `price` before rewriting docs. The current docs should not constrain the redesign. Use the pre-release status to remove obsolete code and docs directly and converge on one clean implementation path per concern.
