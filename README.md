# Janiform

Janiform is a compact high-frequency asset return model that combines seasonal asymmetric Hawkes trade activity with NIG business-time shocks and marked news jumps (including closed-market gap effects). The Rust core provides the computation engine. The Python package exposes an importable API and CLI via PyO3 bindings.

The name *Janiform* comes from the Roman god *Janus* who is a god with two faces (one looking forward, one backward). Janus is also the god of transitions and duality. Asymmetry is built into the model:
- Two Lévy clocks
- Two intensities
- Two excitation channels

It's Jani*form* because it's about form: a system whose geometry is essentially bifacial.

## Prerequisites

- Python: `>=3.12` or `uv`
- Rust: `>=1.89`

## Quickstart

On Linux:

```bash
./scripts/bootstrap-linux.sh
janiform --help
janiform price --help
janiform simulate coarse-interval --help
janiform stylized-facts horizon-comparison --help
janiform seasonality diagnostics --help
janiform seasonality preprocess-timestamps --help
janiform hourly-factors build-contract --help
janiform fit plan --help
python3 -m examples.market_data_calibration_workflow
python3 -m examples.pricing_workflow
```

On Windows:

```powershell
./scripts/bootstrap-win.ps1
janiform --help
janiform price --help
janiform simulate coarse-interval --help
janiform stylized-facts horizon-comparison --help
janiform seasonality diagnostics --help
janiform seasonality preprocess-timestamps --help
janiform hourly-factors build-contract --help
janiform fit plan --help
python -m examples.market_data_calibration_workflow
python -m examples.pricing_workflow
```

## First-Time Fit

If your headers are conventional and the market preset is known, you can usually start with just
the data path and market.

```bash
janiform fit data/eurusd_ticks.csv --market eurusd_spot --explain --output human
janiform fit inspect data/eurusd_ticks.csv --market eurusd_spot --output json
```

```python
from datetime import UTC, datetime

from janiform.calibration import (
    CalibrationMeanReversionConfiguration,
    JaniformEstimator,
    MeanReversionAnchorModelKind,
)
from janiform.contracts import MeanReversionBoundaryState

model = JaniformEstimator(
    market_profile="eurusd_spot",
    mean_reversion=CalibrationMeanReversionConfiguration(
        anchor_model_kind=MeanReversionAnchorModelKind.ENDOGENOUS_CORE_PRESSURE_LEVEL,
    ),
).fit(
    tick_data_path="data/eurusd_ticks.csv",
)

simulation = model.simulate(
    seed=7,
    start_time_utc=datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
    horizon_interval_count=8,
)
pandas_frame = simulation.to_pandas()

pricing_result = model.price_option(
    valuation_time_utc=datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
    spot=1.0850,
    strike=1.0900,
    maturity_interval=1.0,
    observed_boundary_state=MeanReversionBoundaryState(
        observed_log_price=0.0,
        anchor_value=0.0,
        pressure_state=0.0,
        deviation=0.0,
    ),
)
```

When the fitted model enables mean reversion, `JaniformModel.price_option(...)` requires an
explicit `observed_boundary_state`. The high-level API and CLI do not invent that anchored pricing
state for you. Under anchored pricing, the `Q` anchor path is generated internally from the current
observed boundary state and the model's local tilted recursion.

For non-seasonality workflows, high-level calibration now uses endogenous mean reversion by
default. Pass `JaniformEstimator(mean_reversion=...)` or `janiform fit --mean-reversion-anchor-model`
only when you want to make that policy explicit or override the calibration grid interval.
The calibration workbench still exposes the anchor policy selector, but the full-model path no
longer offers a disabled mean-reversion branch.
When you omit `calibration_grid_interval_hours`, the high-level market-data translation derives it
from the median spacing of retained session-open boundary observations when that spacing is
observable, and otherwise from the median observed session duration in the dataset.
Mean-reversion `observed_log_price` and `anchor_value` are expressed in the runtime simulation
coordinate system `log(price / reference_price)`, where `reference_price` is the simulation output
reference level stored with the fitted model. `MeanReversionBoundaryState` also carries the
endogenous pressure-state coordinate used by the pressure-to-level anchor law.

Set `JaniformEstimator(cpu_count=...)` or `janiform fit --cpu-count ...` when you want to
control how many CPU cores staged calibration uses while evaluating candidate parameter sets.
Use `JaniformEstimator(optimizer_settings_by_stage=..., preprocessing_settings=...)` or the
equivalent `MarketDataCalibrationRequest` fields when you want to override solver tolerances,
seasonality preprocessing settings, or return-components jump and gap thresholds instead of
accepting Janiform's built-in defaults.
The default full calibration workflow now runs five stages in order:
`seasonality`, `hawkes`, `return_components`, `mean_reversion`, and `joint_refinement`.
Successful staged calibration responses expose diagnostic artifacts directly in memory mode
through `diagnostic_artifact_bundle`, and expose persisted diagnostics in disk mode through
`diagnostic_artifact_references`.

`JaniformEstimator.fit(...)` and `janiform fit` no longer expose the canonical staged evidence
graph on the default path. Use `PreparedMarketDataCalibration` or `fit staged` only when you
intentionally want the advanced artifact-backed orchestration boundary.

## Developer Environment Setup

### Linux
Make sure you have **Rust >=1.89** and either **uv** or **Python >=3.12** installed first.

To set up your environment properly, run:

```bash
./scripts/bootstrap-linux.sh
```

Or if you are running a JetBrains IDE with uv:

```powershell
./scripts/bootstrap-linux.sh .venv
```

This automatically installs all missing dependencies, including **maturin**, the build system, and **pytest**.

### Windows
Make sure you have **Rust >=1.89** and either **uv** or **Python >=3.12** installed first.

To set up your environment properly, run:

```powershell
./scripts/bootstrap-win.ps1
```

Or if you are running a JetBrains IDE with uv:

```powershell
./scripts/bootstrap-win.ps1 .venv
```

This automatically installs all missing dependencies, including **maturin**, the build system, and **pytest**.

## Building

When developing you regularly need to build. The easiest way to build is to use the build scripts.

### Linux

```bash
./scripts/build-linux.sh .venv-linux
```

Or if you are running a JetBrains IDE with uv:

```bash
./scripts/build-linux.sh .venv
```

### Windows

```powershell
./scripts/build-win.ps1 .venv
```

Or if you are running a JetBrains IDE with uv:

```powershell
./scripts/build-win.ps1 .venv
```

## Testing

When developing you regularly need to run tests. The easiest way to do that is to use the test scripts.

### Linux

```bash
./scripts/test-python-linux.sh .venv-linux
./scripts/test-rust-linux.sh .venv-linux
```

Or if you are running a JetBrains IDE with uv:

```bash
./scripts/test-python-linux.sh .venv
./scripts/test-rust-linux.sh .venv
```

### Windows

```powershell
./scripts/test-python-win.ps1 .venv
./scripts/test-rust-win.ps1 .venv
```

Or if you are running a JetBrains IDE with uv:

```powershell
./scripts/test-python-win.ps1 .venv
./scripts/test-rust-win.ps1 .venv
```

## Test script syntax

The Python test scripts pass command line arguments on to **pytest**:

### Linux

```bash
./scripts/test-python-linux.sh [venv-folder] -- [pytest-command-line-arguments]
```

### Windows

```powershell
./scripts/test-python-win.ps1 [venv-folder] [pytest-command-line-arguments]
```

The Rust test scripts pass command line arguments on to `cargo nextest run` when nextest is installed, otherwise they pass them on to `cargo test`:

### Linux

```bash
./scripts/test-rust-linux.sh [venv-folder] -- [cargo-command-line-arguments]
```

### Windows

```powershell
./scripts/test-rust-win.ps1 [venv-folder] [cargo-command-line-arguments]
```

### Example on Linux

This tells the script to use the `.venv-linux` venv, and to pass on the `-v` command line argument to **pytest**.

```bash
./scripts/test-python-linux.sh .venv-linux -- -v
```

## Generated Crate READMEs

Workspace crate `README.md` files are checked in and generated from Cargo metadata. Regenerate them with:

```bash
python3 scripts/generate_crate_readmes.py
```

## Project Layout

- `crates/janiform/`: Umbrella Rust facade crate
- `crates/janiform-market-time/`: Internal shared market-time infrastructure for calendars, seasonality, and hourly factors
- `crates/janiform-pybind/`: PyO3 extension module (`janiform._native`)
- `presets/sources/`: Human-reviewed preset source manifests used to generate bundled runtime artifacts
- `python/janiform/`: Python package, CLI entrypoints, and typed workflow contracts, including package-backed workflow namespaces such as `janiform.simulation`
- `python/janiform/calibration/_bundled_presets/generated/`: Checked-in bundled preset artifacts and runtime index consumed by the preset registry
- `tools/workbench/`: PyQt6 calibration workbench plus headless report generation for staged-calibration diagnostic bundles
- `examples/`: Executable public-API workflow examples for simulation, pricing, and calibration
- `tests/`: Python tests and shared fixtures
- `docs/`: Design and development documentation

## Examples

- Workflow examples and expected output summaries: `examples/README.md`
- Seeded simulation example: `examples/simulation_workflow.py`
  - High-level `JaniformModel.simulate(...)` returns `JaniformSimulationResult`, which keeps Janiform metadata and diagnostics attached while exposing `to_pandas()`, `to_numpy()`, and `to_parquet(path)`.
  - Seeded responses still expose lazy `fine_path` and `event_tape` views plus `diagnostics`, so normal runs avoid a full fine-path JSON round-trip and only materialize chunks on demand.
- Vanilla option pricing example: `examples/pricing_workflow.py`
- Primary market-data calibration example with tick-data `csv`, tick-data `parquet`, explicit known-news inputs, and optional option-surface files: `examples/market_data_calibration_workflow.py`
- Advanced staged calibration example with default `memory` mode, explicit `disk` override, raw observed open-session return evidence including local sell-share proxies for return-components fitting, and internal close-open gap detection from raw boundary observations: `examples/calibration_workflow.py`
- Calibration diagnostics workbench: `python -m tools.workbench <run-directory-or-response-json>`
- During live runs, the workbench now surfaces joint-refinement restart and finalist-seed progress so long-running calibrations expose internal runtime state before the stage finishes
- Headless calibration diagnostics report and plot export: `python -m tools.workbench.report --input <run-directory-or-response-json> --output-dir <report-dir>`
- Default high-level fitting now lives under `janiform.calibration` through `JaniformEstimator.fit(...)`, which returns a lean `JaniformModel` and can optionally surface a separate `JaniformFitReport`
- Runtime hourly seasonality reconstructed from calibration output should use `janiform.hourly_factors.build_runtime_hourly_seasonality_source_from_calibration_payload(...)` rather than manual component recomposition in consumers
- Workflow package roots remain curated convenience surfaces, and natural public domain modules such as `janiform.simulation.seeded` and `janiform.calibration.staged.inputs` are also supported when they make ownership clearer
- Curated roots are intentionally small. If you need advanced contracts, serialization helpers, or
  CLI implementation details, import the ownership submodule instead of relying on broad root
  re-export sets

## CLI Automation

Primary workflow commands are automation-friendly by default:

- No prompts or stdin fallback paths
- `--output json` by default for machine-readable execution
- Stable precedence: explicit CLI flags override environment variables, environment variables override command-config files, and command-config files override built-in defaults

Primary commands also accept `--config-json PATH` with schema `cli.command_config.v1`. The config document uses long flag names without the leading `--` inside its `arguments` object. For example:

```json
{
  "schema_version": "cli.command_config.v1",
  "command": "fit.plan",
  "arguments": {
    "run-id": "market-data-run-001",
    "workflow-scope": "seasonality_only",
    "tick-data-path": "data/ticks.csv",
    "tick-data-format": "csv",
    "tick-data-provenance-label": "primary-feed",
    "tick-timestamp-column": "timestamp_utc",
    "exchange-code": "XNYS",
    "session-timezone": "America/New_York",
    "timestamp-timezone": "UTC",
    "execution-mode": "disk",
    "disk-checkpoint-directory": "tmp/checkpoints",
    "output": "json"
  }
}
```

Supported automation environment variables:

- `JANIFORM_CLI_CONFIG_JSON`
- `JANIFORM_CLI_OUTPUT`
- `JANIFORM_CLI_CALIBRATION_EXECUTION_MODE`
- `JANIFORM_CLI_CALIBRATION_DISK_CHECKPOINT_DIRECTORY`

## Documentation

- Design model and rationale: `docs/design/janiform_design_brainstorm_notes.md`
- CLI command architecture and namespace layout: `docs/development/guides/cli_command_architecture_and_namespace_layout.md`
- CLI command reference and practical examples: `docs/development/guides/cli_command_reference_and_examples.md`
- Python public API surface map and namespace policy: `docs/development/guides/python_public_api_surface_map_and_namespace_policy.md`
- Python experimental API policy: `docs/development/guides/python_experimental_api_policy.md`
- Python API contract compatibility and version guardrails: `docs/development/guides/python_api_contract_compatibility_and_version_guardrails.md`
- Python error taxonomy and exception contract: `docs/development/guides/python_error_taxonomy_and_exception_contract.md`
- Calendar and seasonality API contracts: `docs/development/guides/calendar_and_seasonality_api_contracts.md`
- Hawkes parameter validation report: `docs/development/guides/hawkes_parameter_validation_report.md`
- Hawkes failure diagnostics report: `docs/development/guides/hawkes_failure_diagnostics_report.md`
- NIG characteristic exponent guardrails: `docs/development/guides/nig_characteristic_exponent_guardrails.md`
- NIG cross-engine interface and reference fixtures: `docs/development/guides/nig_cross_engine_interface_and_reference_fixtures.md`
- Return decomposition API contract: `docs/development/guides/return_decomposition_api_contract.md`
- Coarse simulation workflow contract: `docs/development/guides/coarse_simulation_workflow_contract.md`
- Coarse versus seeded-tick scale consistency regression contract: `docs/development/guides/coarse_seeded_tick_scale_consistency_regression_contract.md`
- Vanilla option pricing contract: `docs/development/guides/vanilla_option_pricing_contract.md`
  - Includes `interval_delta` (`Delta`) semantics with hourly (`1.0`) intraday default and configurable non-hourly support when compatible factors are supplied
- Sufficient-statistics risk sampler contract: `docs/development/guides/sufficient_statistics_risk_sampler_contract.md`
- Pricing diagnostics and calibration hooks schema: `docs/development/guides/pricing_diagnostics_and_calibration_hooks_schema.md`
- Calibration optimizer adapter and objective composition schema: `docs/development/guides/calibration_optimizer_adapter_and_objective_composition_schema.md`
- Calibration artifact manifest schema and versioning policy: `docs/development/guides/calibration_artifact_manifest_schema_and_versioning_policy.md`
- Calibration dataset snapshot and input provenance schema: `docs/development/guides/calibration_dataset_snapshot_and_input_provenance_schema.md`
- Calibration stage parameter and optimizer artifact schema: `docs/development/guides/calibration_stage_parameter_and_optimizer_artifact_schema.md`
- Calibration diagnostics metrics and comparison artifact schema: `docs/development/guides/calibration_diagnostics_metrics_and_comparison_artifact_schema.md`
- Calibration experiment index and lookup helpers: `docs/development/guides/calibration_experiment_index_and_lookup_helpers.md`
- Calibration reload resolver and compatibility checks: `docs/development/guides/calibration_reload_resolver_and_compatibility_checks.md`
- Staged calibration failure playbook: `docs/development/guides/staged_calibration_failure_playbook.md`
- Open closed gap summary and diff report contract: `docs/development/guides/open_closed_gap_summary_and_diff_report_contract.md`
- Scale-consistency threshold policy and decision contract: `docs/development/guides/scale_consistency_threshold_policy_and_decision_contract.md`
- Stylized fact metric catalog schema: `docs/development/guides/stylized_fact_metric_catalog_schema.md`
- Stylized fact horizon comparison report schema: `docs/development/guides/stylized_fact_horizon_comparison_report_schema.md`
- Affine oracle solver diagnostics and telemetry schema: `docs/development/guides/affine_oracle_solver_diagnostics_schema.md`
- Seeded tick fine path schema: `docs/development/guides/seeded_tick_event_tape_schema.md`
- Scale consistency report and drift diagnostics schema: `docs/development/guides/scale_consistency_report_and_drift_diagnostics_schema.md`
- Validation fallback policy: `docs/development/guides/validation_fallback_policy.md`
- CI tiered execution strategy: `docs/development/guides/ci_tiered_execution_strategy.md`
- Python coding conventions: `docs/development/guides/python_style_guide.md`
- Planning artifacts: `docs/planning/`

The detailed calendar and seasonality sections were moved out of this README into the development guide above to keep the repo root documentation concise.
