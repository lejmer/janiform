# CLI Command Architecture And Namespace Layout

Document version: `cli.command_architecture.pre_v0_1.milestone_2`

Status: active for the current pre-v0.1 milestone on March 13, 2026.

## Purpose

This guide defines the top-level Janiform CLI command tree for the current pre-v0.1
workflow surface. It keeps the primary user-facing workflows separate from internal
developer utilities and aligns command names with the public Python workflow modules.

It also defines the supported Python package layout for the CLI implementation so the
command tree can grow without recreating monolithic `_commands.py` or `_parser.py`
implementation buckets.

## Command tree

```text
janiform
├── fit
│   ├── [default high-level fit workflow]
│   ├── plan
│   └── staged
├── simulate
│   ├── [default high-level simulation workflow]
│   ├── coarse-interval
│   └── seeded-ticks
├── presets
│   ├── list
│   ├── show
│   └── export
├── seasonality
│   ├── diagnostics
│   └── preprocess-timestamps
├── hourly-factors
│   ├── materialize-seasonality
│   ├── materialize-news-intensity
│   └── build-contract
├── pricing
│   └── vanilla-option
├── stylized-facts
│   └── horizon-comparison
└── dev
    └── regression
        ├── deterministic
        └── open-closed-gap-leakage
```

## Namespace policy

- `fit` and `simulate` are the primary workflow roots that first-time users should encounter.
- `presets`, `seasonality`, `hourly-factors`, `pricing`, and `stylized-facts` remain top-level
  because they are standalone domains rather than variants of `fit` or `simulate`.
- `dev` is reserved for internal and developer-only utilities.
- Developer-only commands must not be placed beside primary workflow groups at the top level.
- Command aliases are forbidden because they add ambiguity to automation and shell scripts.

## Python package layout

- `janiform.cli` remains the curated convenience root for `main()` and stable CLI constants.
- `janiform.cli.commands.*` is the supported ownership path for real command groups such as:
  - `model_workflows`
  - `presets`
  - `seasonality`
  - `hourly_factors`
  - `pricing`
  - `stylized_facts`
  - `simulation`
  - `calibration`
  - `development`
- `janiform.cli.command_tree` owns only the root application object, top-level group assembly, and process entrypoint invocation.
- Each `janiform.cli.commands.*` owner module defines its live Click group or command objects with Click decorators and attaches any namespace-local subcommands directly in that module.
  Underscore-prefixed Click callbacks and builder helpers remain internal implementation details and must stay out of `__all__`.
- `janiform.cli.command_runtime` holds the small shared command-runtime layer:
  custom `Command` and `Group` classes for default resolution, common option factories, and `CliCommandContext` construction.
- Command callbacks use Click-native keyword parameters plus a small `CliCommandContext` built from `click.Context`.
  The CLI must not reintroduce synthetic `argparse.Namespace` handoff layers or parser-emulation adapters.
- Named command-group modules are preferred over catch-all buckets when the contents mirror a real command namespace.
- Low-level CLI boundary helpers such as rendering, document loading, and path resolution may live in named helper modules, but they are not command-group ownership modules and should stay smaller than the workflow-facing packages.
- New CLI modules should not use underscore-prefixed module names unless they are truly private implementation details.

## Naming policy

- Top-level command groups follow the workflow-first CLI surface:
  - `fit`
  - `simulate`
  - `presets`
  - `seasonality`
  - `hourly-factors`
  - `pricing`
  - `stylized-facts`
- Subcommand names stay short while preserving the specific workflow identity:
  - `plan` for the recommended file-driven fitting boundary
  - `staged` for advanced staged calibration orchestration
  - `coarse-interval` for direct interval-path simulation on a chosen coarse grid
  - `seeded-ticks` for the seeded tick-event simulator
  - `diagnostics` for the primary end-to-end seasonality estimation and diagnostics workflow
  - `preprocess-timestamps` for standalone timestamp-event preprocessing against the exchange
    calendar registry
  - `materialize-seasonality` for materializing hourly `s_h` from timestamped source samples
  - `materialize-news-intensity` for materializing hourly `nu_h` from known-news and surprise controls
  - `build-contract` for building the combined hourly-factor contract from explicit or materialized artifacts
  - `vanilla-option` for vanilla option pricing
  - `horizon-comparison` for stylized-fact horizon-comparison report generation
- Developer-only validation harnesses remain explicit about their function:
  - `dev regression deterministic`
  - `dev regression open-closed-gap-leakage`

## Help-output expectations

- `janiform --help` lists the eight top-level namespaces in the order above.
- `janiform fit --help` and `janiform simulate --help` make the high-level defaults obvious while
  still listing their explicit expert subcommands.
- `janiform presets --help`, `janiform seasonality --help`, `janiform hourly-factors --help`,
  `janiform price --help`, and `janiform stylized-facts --help` each list their current
  workflow commands or subcommands.
- `janiform dev --help` lists internal-only utility groups.
- Incomplete command paths print the help for the selected namespace branch.

## Exit codes

The CLI uses a small stable exit-code map so shell scripts and CI jobs can classify failures
without parsing English text:

- `0`: success
- `2`: validation error or usage error
- `3`: runtime workflow failure after input validation succeeded
- `4`: internal failure or integration-boundary failure

Additional policy notes:

- Click usage failures are part of the validation bucket and therefore also exit with `2`.
- Primary workflow commands write one structured error line to stderr and never emit tracebacks in
  normal operation.
- File-driven calibration validation errors include the failing file path, the field or column
  path, and a `normalization_step` context field when the failure occurs during preprocessing.

## Common argument standards

- Shared concepts use long flags only. Short aliases are forbidden for script readability.
- File and directory inputs use explicit path-named flags such as `--request-json`,
  `--execution-input-json`, `--calendar-registry-json`, and `--disk-checkpoint-directory`.
- Primary workflow commands also accept `--config-json PATH` for non-interactive automation.
- Primary workflow commands share `--output {human,json}`:
  - `json` is the deterministic machine-readable mode for automation
  - `human` is the concise operator-oriented text mode
- CLI help uses `PATH` for file-like values and `DIRECTORY` for directory overrides so
  the shape of each argument is consistent across commands.
- All CLI path flags accept absolute paths. Relative paths are resolved from the
  repository root so scripts behave the same even when invoked from another directory.
- Shared path flags keep the same semantics wherever they appear:
  - `--calendar-registry-json` always means a `calendar.registry.v1` payload or wrapper
    used to rehydrate runtime calendar-aware inputs.
  - `--execution-input-json` always means the typed workflow execution-input contract for
    the selected command.

## Automation and precedence

- Primary workflow commands are non-interactive by default:
  - they do not prompt for missing values
  - they do not read from stdin as a fallback input channel
  - when required inputs are missing, they fail with the documented validation exit code
- Argument precedence is explicit and stable:
  1. explicit CLI flags
  2. environment variables
  3. `--config-json` or `JANIFORM_CLI_CONFIG_JSON`
  4. built-in command defaults
- Supported automation environment variables:
  - `JANIFORM_CLI_CONFIG_JSON`: path to a `cli.command_config.v1` file for the selected primary command
  - `JANIFORM_CLI_OUTPUT`: default value for `--output`
  - `JANIFORM_CLI_CALIBRATION_EXECUTION_MODE`: default value for calibration `--execution-mode`
  - `JANIFORM_CLI_CALIBRATION_DISK_CHECKPOINT_DIRECTORY`: default value for calibration `--disk-checkpoint-directory`
- Relative paths supplied through CLI flags, environment variables, or command-config files are all
  resolved from the repository root.

### Command-config document

`--config-json` and `JANIFORM_CLI_CONFIG_JSON` point to a JSON object with schema
`cli.command_config.v1`:

```json
{
  "schema_version": "cli.command_config.v1",
  "command": "fit.plan",
  "arguments": {
    "run-id": "market-data-run-001",
    "workflow-scope": "seasonality_only",
    "tick-data-path": "tests/fixtures/calibration/ticks.csv",
    "tick-data-format": "csv",
    "tick-data-provenance-label": "primary-feed",
    "tick-timestamp-column": "timestamp_utc",
    "exchange-code": "XNYS",
    "session-definition-path": "tests/fixtures/calibration/sessions.json",
    "session-definition-format": "json",
    "session-definition-provenance-label": "calendar-registry",
    "session-timezone": "America/New_York",
    "timestamp-timezone": "UTC",
    "execution-mode": "disk",
    "disk-checkpoint-directory": "tmp/checkpoints",
    "output": "json"
  }
}
```

- `command` must match the selected CLI command exactly.
- `arguments` uses the long flag names without the leading `--`.
- Flag-style boolean arguments use JSON booleans. Set them to `true` to enable the flag.

## Current workflow command contracts

The current pre-v0.1 CLI uses explicit JSON contract files at the command boundary so the
command surface stays aligned with the typed Python workflow models.

### `janiform simulate seeded-ticks`

Required arguments:
- `--request-json`: `SeededTickSimulationRequest` JSON object
- `--calendar-registry-json`: `calendar.registry.v1` JSON object or a wrapper object with `calendar_registry`

Behavior:
- The command rehydrates the runtime hourly-factor handle from the request's
  `hourly_factor_summary` plus the supplied calendar registry.
- Validation fails if the public request summary does not reproduce the same hourly-factor contract.

Output:
- `--output json` writes deterministic JSON for `simulation.seeded_tick_event.path.v1` to stdout.
- `--output human` writes a concise completion summary with event-count metadata.

### `janiform simulate coarse-interval`

Required arguments:
- `--request-json`: `CoarseSimulationRunRequest` JSON object

Behavior:
- The command runs the public coarse-interval workflow directly from the canonical request
  contract.
- The request embeds the calendar registry artifact, interval factors, and news-process
  configuration needed to generate a direct Delta-spaced path without replaying seeded ticks.

Output:
- `--output json` writes deterministic JSON for `simulation.coarse_interval.path.v1` to stdout.
- `--output human` writes a concise completion summary with interval-count metadata.

### `janiform seasonality diagnostics`

Required arguments:
- `--dataset-id`
- `--event-records-json`: JSON object with top-level `event_records`
- `--calendar-registry-json`: `calendar.registry.v1` JSON object or a wrapper object with `calendar_registry`
- `--exchange-code`

Optional arguments:
- `--config-json PATH`
- `--timestamp-field-name`: defaults to `timestamp`
- `--session-filter {all,open_only,closed_only}`: defaults to `open_only`
- `--sparse-hour-event-threshold`: defaults to `1`
- `--intraday-minutes-per-bin`: defaults to `1`
- `--intraday-smoothing-kernel {gaussian,uniform}`: defaults to `gaussian`
- `--intraday-smoothing-bandwidth-bins`: defaults to `3.0`
- `--intraday-minimum-bin-events`: defaults to `1`
- `--intraday-pseudocount`: defaults to `1e-6`
- `--day-of-week-minimum-bucket-events`: defaults to `1`
- `--day-of-week-pseudocount`: defaults to `1e-6`
- `--day-of-year-minimum-bucket-events`: defaults to `1`
- `--day-of-year-pseudocount`: defaults to `1e-6`
- `--estimator-version`: defaults to `seasonality_estimator.v1`
- `--output {human,json}`

Behavior:
- The command reuses `janiform.seasonality.preprocess_timestamp_events`,
  `estimate_intraday_seasonality`, `estimate_day_of_week_seasonality`,
  `estimate_day_of_year_seasonality`, `compose_deterministic_seasonality_signal`, and
  `generate_seasonality_diagnostics_report`.
- Event records are validated against the public timestamp-event preprocessing contract.
- The command stays on the raw timestamp-event boundary so users do not need to chain
  intermediate estimation steps unless they are debugging.

Output:
- `--output json` writes the canonical diagnostics report payload from
  `seasonality.diagnostics.report.v1` to stdout.
- `--output human` writes a concise summary over the same report with fit-quality,
  sparsity, and reproducibility highlights.

### `janiform seasonality preprocess-timestamps`

Required arguments:
- `--event-records-json`: JSON object with top-level `event_records`
- `--calendar-registry-json`: `calendar.registry.v1` JSON object or a wrapper object with `calendar_registry`
- `--exchange-code`

Optional arguments:
- `--config-json PATH`
- `--timestamp-field-name`: defaults to `timestamp`
- `--session-filter {all,open_only,closed_only}`: defaults to `open_only`
- `--sparse-hour-event-threshold`: defaults to `1`
- `--output {human,json}`

Behavior:
- The command reuses `janiform.seasonality.preprocess_timestamp_events`.
- Event records are validated against the public timestamp-event preprocessing contract.
- Session filtering and sparse-hour diagnostics are applied through the canonical calendar registry.

Output:
- `--output json` writes the canonical preprocessing payload fields from
  `seasonality.timestamp_event.preprocessed.v1` to stdout.
- `--output human` writes a concise completion summary with accepted-record and coverage-diagnostic
  metadata.

### `janiform price`

Required arguments:
- `--execution-input-json`: `VanillaOptionPricingExecutionInput` JSON object

Behavior:
- The command validates the execution input directly against the Python pricing contract.
- Interval factors keep `nu_h_known` and `nu_h_surprise` separate.

Output:
- `--output json` writes deterministic JSON for `pricing.vanilla_option.run_result.v1` to stdout.
- `--output human` writes a concise completion summary with price and delta.

### `janiform hourly-factors materialize-seasonality`

Required arguments:
- `--calendar-registry-json`: `calendar.registry.v1` JSON object or a wrapper object with `calendar_registry`
- `--exchange-code`
- `--range-start-utc`
- `--range-end-utc`
- `--materialization-input-json`: JSON object with `source_timestamps_utc`,
  `source_seasonality_factors`, and optional materialization settings

Behavior:
- The command reuses `janiform.hourly_factors.materialize_hourly_seasonality_factors`.
- Output stays on the canonical seasonality materialization report shape so the emitted file can be
  reviewed directly or reused in downstream hourly-factor contract building.

Output:
- `--output json` writes the canonical materialization report payload from
  `seasonality.hourly_s_h.materialization.v1` to stdout.
- `--output human` writes a concise completion summary with sparse, interpolated, and fallback counts.

### `janiform hourly-factors materialize-news-intensity`

Required arguments:
- `--calendar-registry-json`: `calendar.registry.v1` JSON object or a wrapper object with `calendar_registry`
- `--exchange-code`
- `--range-start-utc`
- `--range-end-utc`
- `--materialization-input-json`: JSON object with `known_scheduled_event_timestamps_utc`,
  `surprise_baseline_intensity`, and optional interval multipliers

Behavior:
- The command reuses `janiform.hourly_factors.materialize_hourly_news_intensity_factors`.
- Output stays on the canonical news-intensity materialization report shape so the emitted file can
  be reviewed directly or reused in downstream hourly-factor contract building.

Output:
- `--output json` writes the canonical materialization report payload from
  `seasonality.hourly_nu_h.materialization.v1` to stdout.
- `--output human` writes a concise completion summary with known-arrival and non-zero interval counts.

### `janiform hourly-factors build-contract`

Required arguments:
- `--calendar-registry-json`: `calendar.registry.v1` JSON object or a wrapper object with `calendar_registry`
- `--exchange-code`
- `--range-start-utc`
- `--range-end-utc`
- exactly one of `--seasonality-explicit-source-json` or `--seasonality-materialized-source-json`
- exactly one of `--news-intensity-explicit-source-json` or `--news-intensity-materialized-source-json`

Behavior:
- Explicit source files carry hourly factor arrays directly.
- Materialized source files are the canonical outputs emitted by the two hourly-factor materialization
  commands above.
- The command validates that materialized-source artifacts match the requested exchange and UTC range,
  then reuses `janiform.hourly_factors.build_hourly_factor_contract`.
- The resulting contract is ready for downstream simulation or pricing request preparation.

Output:
- `--output json` writes the canonical contract payload from
  `seasonality.hourly_factor.contract.v1` to stdout.
- `--output human` writes a concise completion summary with contract metadata and timezone-transition counts.

### `janiform stylized-facts horizon-comparison`

Required arguments:
- `--metric-catalog-json`: stylized-fact metric catalog JSON object
- `--comparison-request-json`: stylized-fact horizon-comparison request JSON object

Optional arguments:
- `--config-json PATH`
- `--artifact-export-json PATH`: stylized-fact artifact-export config JSON object
- `--execution-mode {disk,memory}`: defaults to `memory` to match the public Python workflow
- `--output {human,json}`

Behavior:
- The command reuses `janiform.stylized_facts.run_stylized_fact_horizon_comparison_report_generator`.
- File inputs are loaded as JSON objects and passed through the existing Python validation path.
- Artifact export config is caller-supplied as a JSON object instead of separate CLI-only flags.
- Export is valid only when `--execution-mode disk` is selected.

Output:
- `--output json` writes deterministic JSON for `validation.stylized_fact.horizon_comparison.report_bundle.v1`.
- `--output human` writes a concise summary over the same canonical report bundle, including
  report status and any exported artifact metadata.

### `janiform fit plan`

Required arguments:
- `--run-id`
- `--workflow-scope {seasonality_only,full_without_option_surface,full_with_option_surface}`
- `--tick-data-path`
- `--tick-data-format {csv,parquet}`
- `--tick-data-provenance-label`
- `--tick-timestamp-column`
- `--exchange-code`
- `--session-definition-path`
- `--session-definition-format {csv,parquet,json}`
- `--session-definition-provenance-label`
- `--session-timezone`
- `--timestamp-timezone`

Optional arguments:
- `--config-json PATH`
- `--tick-price-input-policy {explicit_price_column,bid_ask_midpoint}`
- `--tick-price-column`
- `--tick-trade-price-column`
- `--tick-bid-column`
- `--tick-ask-column`
- `--tick-close-price-column`
- `--known-news-path`
- `--known-news-format {csv,parquet,json}`
- `--known-news-provenance-label`
- `--known-news-timezone`
- `--known-news-timestamp-column`
- `--known-news-event-id-column`
- `--option-surface-path`
- `--option-surface-format {csv,parquet}`
- `--option-surface-provenance-label`
- `--option-quote-input-policy {mid_price_column,bid_ask_midpoint,implied_volatility_column}`
- `--option-quote-timestamp-column`
- `--option-expiry-column`
- `--option-strike-column`
- `--option-type-column`
- `--option-mid-price-column`
- `--option-bid-column`
- `--option-ask-column`
- `--option-implied-volatility-column`
- `--duplicate-record-policy {error,keep_first,keep_last}`
- `--disable-timestamp-sort`
- `--execution-mode {disk,memory}`: defaults to `disk` on the CLI
- `--disk-checkpoint-directory`: required when the effective execution mode is `disk`
- `--emit-derived-staged-execution-input`
- `--omit-input-provenance-report`

Behavior:
- This is the recommended calibration CLI boundary for market users.
- The command validates file-driven request fields, provenance labels, normalization-policy selections,
  and the derived staged calibration scope.
- The command can be driven entirely from `--config-json` plus environment defaults without any stdin input.
- `--output json` writes deterministic JSON for `cli.calibration.market_data.summary.v1`.
- The summary schema is intentionally user-facing and includes:
  - `request_schema_version` so downstream tooling can trace back to `calibration.market_data.request.v1`
  - `preparation_schema_version` so downstream tooling can tie the summary to the prepared staged-input boundary
  - `input_files` for tick data, session definitions, and optional known-news or option-surface files
  - `column_mapping` and `normalization_policy`
  - `workflow_scope`, `session_context`, and output artifact targets
  - `diagnostics_summary` with selected stages, preprocessing steps, and workflow-level record counts
  - `provenance_summary` with loaded-source summaries, derived dataset references, and deterministic derivation policies
  - `output_schema_metadata` with the field-selection policy `user_facing_workflow_metadata_only.v1`
- The default structured summary does not embed full internal staged execution payload sections like
  `seasonality_stage_input`, but it does expose the selected staged scope for support triage.
- `--output human` writes a concise operator-oriented summary of the same user-facing metadata.
- Translation into the canonical staged execution input remains a lower-level step documented separately.

### `janiform fit staged`

Required arguments:
- `--execution-input-json`: `StagedCalibrationExecutionInput` JSON object
- `--calendar-registry-json`: `calendar.registry.v1` JSON object or a wrapper object with `calendar_registry`

Optional arguments:
- `--config-json PATH`
- `--execution-mode {disk,memory}`: defaults to `disk` on the CLI
- `--disk-checkpoint-directory`: required when the effective execution mode is `disk` and the input JSON does not already provide one

Behavior:
- This is the advanced or internal calibration command path.
- Option precedence is explicit:
  1. `--execution-mode`, when supplied, overrides `request.execution_mode` from `--execution-input-json`.
  2. When `--execution-mode` is omitted, `JANIFORM_CLI_CALIBRATION_EXECUTION_MODE`
     overrides command-config values, and the CLI default `disk` still overrides the
     execution-input payload when neither higher-precedence source supplies a value.
  3. `--disk-checkpoint-directory`, when supplied, overrides
     `runtime_resources.disk_checkpoint_directory` from `--execution-input-json`.
  4. `JANIFORM_CLI_CALIBRATION_DISK_CHECKPOINT_DIRECTORY` overrides command-config values
     when the flag is omitted.
- Validation rejects `--disk-checkpoint-directory` when `--execution-mode memory` is selected.
- Return-components stage inputs keep known-news open-jump and closed-gap semantics explicit.

Output:
- `--output json` writes deterministic JSON for `calibration.orchestration.staged_run.report.v1` to stdout.
- `--output human` writes a concise completion summary with run id, execution mode, and stage counts.
