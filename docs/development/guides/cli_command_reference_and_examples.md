# CLI Command Reference And Examples

Document version: `cli.command_reference.pre_v0_1.control_plane_reset.v1`

Status: active for the current pre-v0.1 milestone on March 26, 2026.

## Purpose

This guide is the operator-facing command reference for Janiform's current CLI workflows.
It complements the namespace and contract guide in
`docs/development/guides/cli_command_architecture_and_namespace_layout.md` by focusing on:

- command synopsis
- option summaries
- copy-pastable example invocations
- machine-readable output examples for automation

The primary workflow is now:

1. inspect presets
2. fit from market data
3. simulate later from a saved model

The older explicit workflow-boundary commands are still available under `fit plan`, `fit staged`,
`simulate coarse-interval`, and `simulate seeded-ticks` when you need that control. They are now
artifact-backed. Expert calibration inputs reference prepared payload files instead of embedding
bulk staged evidence as giant JSON tuples.

## Shared Setup

Run the examples from the repository root after bootstrapping the Python environment.
The setup command below writes JSON requests, session definitions, known-news calendars,
tick-data files, and option-surface files under `tmp/cli-docs`.

```bash
PYTHONPATH=python python3 -m examples.support.cli_documentation_setup --output-dir tmp/cli-docs
```

The generated files include:

- `tmp/cli-docs/calendar_registry.json`
- `tmp/cli-docs/coarse_simulation_request.json`
- `tmp/cli-docs/simulation_request.json`
- `tmp/cli-docs/timestamp_event_records.json`
- `tmp/cli-docs/hourly_factor_seasonality_materialization_input.json`
- `tmp/cli-docs/hourly_factor_news_materialization_input.json`
- `tmp/cli-docs/hourly_factor_explicit_seasonality_source.json`
- `tmp/cli-docs/hourly_factor_explicit_news_source.json`
- `tmp/cli-docs/hourly_factor_materialized_seasonality.json`
- `tmp/cli-docs/hourly_factor_materialized_news_intensity.json`
- `tmp/cli-docs/stylized_fact_metric_catalog.json`
- `tmp/cli-docs/stylized_fact_comparison_request.json`
- `tmp/cli-docs/stylized_fact_artifact_export.json`
- `tmp/cli-docs/calibration_execution_input.json`
  - expert-only staged wrapper referencing a prepared payload artifact
- `tmp/cli-docs/data/ticks.csv`
- `tmp/cli-docs/data/ticks.parquet`
- `tmp/cli-docs/data/options.parquet`
- `tmp/cli-docs/config/sessions.json`
- `tmp/cli-docs/config/known-news.csv`

## Shared Conventions

- Primary workflow commands accept `--output {human,json}`.
- `--output json` is the machine-readable mode for automation and CI.
- Relative paths are resolved from the repository root.
- High-level fit commands accept preset inputs such as `--market MARKET`,
  `--session-preset SESSION_PRESET`, `--known-news KNOWN_NEWS`,
  `--session-json PATH`, and `--known-news-json PATH`.
- Use `--known-news none` to keep the off-by-default known-news baseline explicit.
- `janiform fit` persists `model.json`, `fit_report.json`, and `run.json` by default.
- Override the fit artifact location with `--output-dir DIRECTORY`.
- `janiform simulate` consumes that persisted model through `--model-path PATH`.
- Omit `--interval` on `janiform simulate` for seeded tick output.
- Provide `--interval INTERVAL` on `janiform simulate` for coarse interval output.
- Override the primary simulation artifact with `--output-path PATH`.
- `janiform price` is model-driven and writes pricing artifacts by default.
- `fit plan` and `fit staged` share `--execution-mode {disk,memory}`.
- The CLI default is `disk` for `fit plan` and `fit staged`.
- `--disk-checkpoint-directory` is required when the effective fit execution mode is `disk`.
- The high-level `fit` command defaults to `memory`.
- `fit`, `fit plan`, and `fit staged` stay config-first.
  Default users do not assemble staged evidence payloads by hand.
- The stylized-fact report command defaults to `memory`.
- Stylized-fact artifact export uses `--artifact-export-json PATH` so the export contract stays the
  same across Python and CLI entrypoints.

## Presets

Use the preset commands to inspect the curated bundled defaults before fitting.

### List Synopsis

```text
janiform presets list [--category {session,known-news,market-profile}] [--scope {auto,built_in,user_registered}] [--output {human,json}]
```

### Show Synopsis

```text
janiform presets show {session,known-news,market-profile} PRESET_NAME [--scope {auto,built_in,user_registered}] [--output {human,json}]
```

### Export Synopsis

```text
janiform presets export {session,known-news,market-profile} PRESET_NAME [--scope {auto,built_in,user_registered}] [--output {human,json}]
```

### Examples

```bash
janiform presets list --category market-profile --output json
```

```bash
janiform presets show market-profile us_equity_cash --output json
```

```bash
janiform presets export market-profile eurusd_spot --output json
```

## High-Level Fit

Use `janiform fit` for the default user workflow. The command resolves the selected market profile,
session preset or session inference result, and the chosen known-news baseline into one canonical
resolved configuration with provenance.

The default path prepares calibration natively and keeps empirical evidence in the prepared
artifact. The emitted fit report and JSON output expose summaries, provenance, and artifact
references rather than an eager staged evidence graph.

For the beginner path, `janiform fit PATH --market MARKET` is the intended entrypoint. The command
defaults `workflow_scope` to `full_without_option_surface`, infers `run_id` from the file stem plus
timestamp, infers the tick-data format from the file extension, auto-detects common timestamp and
price columns, and uses the selected market profile for timezone, session, and known-news defaults.
Use `--explain` to show the inferred choices on a successful fit, or `janiform fit inspect` to
preview the inferred configuration without running calibration.

For non-seasonality workflows, high-level mean-reversion calibration defaults to the canonical
endogenous anchor policy. Use `--mean-reversion-anchor-model` only when you want to spell that
policy out explicitly, and optionally provide `--mean-reversion-grid-interval-hours` when you want
to override the default median-spacing derivation policy for the lower-frequency calibration
interval.

### Synopsis

```text
janiform fit [PATH] [--market MARKET] [--run-id RUN_ID] [--workflow-scope {seasonality_only,full_without_option_surface,full_with_option_surface}] [--tick-data-path PATH] [--tick-data-format {csv,parquet}] [--tick-timestamp-column TICK_TIMESTAMP_COLUMN] [--timestamp-timezone TIMESTAMP_TIMEZONE] [--tick-data-provenance-label TICK_DATA_PROVENANCE_LABEL] [--tick-price-input-policy {explicit_price_column,bid_ask_midpoint}] [--tick-price-column TICK_PRICE_COLUMN] [--tick-trade-sign-column TICK_TRADE_SIGN_COLUMN] [--tick-bid-column TICK_BID_COLUMN] [--tick-ask-column TICK_ASK_COLUMN] [--duplicate-record-policy {keep_first,keep_last}] [--session-preset SESSION_PRESET] [--known-news KNOWN_NEWS] [--session-json PATH] [--known-news-json PATH] [--exchange-code EXCHANGE_CODE] [--session-timezone SESSION_TIMEZONE] [--mean-reversion-anchor-model {endogenous_core_expectation_one_interval}] [--mean-reversion-grid-interval-hours FLOAT] [--explain] [--silent] [--option-surface-path PATH] [--option-surface-format {csv,parquet}] [--option-surface-provenance-label OPTION_SURFACE_PROVENANCE_LABEL] [--option-quote-input-policy {mid_price_column,bid_ask_midpoint,implied_volatility_column}] [--option-quote-timestamp-column OPTION_QUOTE_TIMESTAMP_COLUMN] [--option-expiry-column OPTION_EXPIRY_COLUMN] [--option-strike-column OPTION_STRIKE_COLUMN] [--option-type-column OPTION_TYPE_COLUMN] [--option-mid-price-column OPTION_MID_PRICE_COLUMN] [--option-bid-column OPTION_BID_COLUMN] [--option-ask-column OPTION_ASK_COLUMN] [--option-implied-volatility-column OPTION_IMPLIED_VOLATILITY_COLUMN] [--execution-mode {disk,memory}] [--disk-checkpoint-directory DIRECTORY] [--emit-derived-staged-execution-input] [--omit-input-provenance-report] [--output-dir DIRECTORY] [--output {human,json}]
```

### Example

```bash
janiform fit tmp/cli-docs/data/ticks.csv \
  --market us_equity_cash \
  --explain \
  --output-dir tmp/cli-docs/fitted_run \
  --output human
```

## High-Level Simulation

Use `janiform simulate` when you already have a persisted model and want the default seeded tick
workflow or the coarse interval workflow without reassembling runtime contracts by hand.

### Simulate Synopsis

```text
janiform simulate --model-path PATH --seed SEED --start-time-utc START_TIME_UTC --horizon-interval-count HORIZON_INTERVAL_COUNT [--interval INTERVAL] [--output-path PATH] [--silent] [--debug-trace-enabled] [--sample-every-nth-transition SAMPLE_EVERY_NTH_TRANSITION] [--max-records-per-chunk MAX_RECORDS_PER_CHUNK] [--output {human,json}]
```

### Examples

```bash
janiform simulate \
  --model-path tmp/cli-docs/fitted_run/model.json \
  --seed 17 \
  --start-time-utc 2026-02-02T15:00:00Z \
  --horizon-interval-count 4 \
  --interval 1h \
  --output json
```

## Explicit Simulation

Use `coarse-interval` when you want a direct Delta-spaced path artifact for analytics, pricing
validation, or risk-style simulation at the interval boundary. Use `seeded-ticks` when you need
a priced fine path, transition ordering, or tick replay diagnostics.

### Coarse-Interval Synopsis

```text
janiform simulate coarse-interval --request-json PATH [--output {human,json}]
```

### Coarse-Interval Options

- `--request-json`: `CoarseSimulationRunRequest` contract file
- `--output {human,json}`: operator summary or machine-readable result

### Coarse-Interval Example

Machine-readable output to stdout:

```bash
janiform simulate coarse-interval \
  --request-json tmp/cli-docs/coarse_simulation_request.json \
  --output json
```

Machine-readable output redirected to a file:

```bash
janiform simulate coarse-interval \
  --request-json tmp/cli-docs/coarse_simulation_request.json \
  --output json \
  > tmp/cli-docs/coarse_simulation_result.json
```

### Seeded-Ticks Synopsis

```text
janiform simulate seeded-ticks --request-json PATH --calendar-registry-json PATH [--output {human,json}]
```

### Seeded-Ticks Options

- `--request-json`: `SeededTickSimulationRequest` contract file
- `--calendar-registry-json`: `calendar.registry.v1` contract file used to rehydrate runtime inputs
- `--output {human,json}`: operator summary or machine-readable result

### Example

Machine-readable output to stdout:

```bash
janiform simulate seeded-ticks \
  --request-json tmp/cli-docs/simulation_request.json \
  --calendar-registry-json tmp/cli-docs/calendar_registry.json \
  --output json
```

Machine-readable output redirected to a file:

```bash
janiform simulate seeded-ticks \
  --request-json tmp/cli-docs/simulation_request.json \
  --calendar-registry-json tmp/cli-docs/calendar_registry.json \
  --output json \
  > tmp/cli-docs/simulation_result.json
```

## Seasonality

Use this command when you want one task-oriented CLI path from raw timestamp-event input to the
canonical seasonality diagnostics report. Use the standalone preprocessing command only when you
want to debug timestamp normalization or inspect calendar coverage before the full workflow.

### Synopsis

```text
janiform seasonality diagnostics --dataset-id DATASET_ID --event-records-json PATH --calendar-registry-json PATH --exchange-code EXCHANGE_CODE [--timestamp-field-name TIMESTAMP_FIELD_NAME] [--session-filter {all,open_only,closed_only}] [--sparse-hour-event-threshold SPARSE_HOUR_EVENT_THRESHOLD] [--intraday-minutes-per-bin INTRADAY_MINUTES_PER_BIN] [--intraday-smoothing-kernel {gaussian,uniform}] [--intraday-smoothing-bandwidth-bins INTRADAY_SMOOTHING_BANDWIDTH_BINS] [--intraday-minimum-bin-events INTRADAY_MINIMUM_BIN_EVENTS] [--intraday-pseudocount INTRADAY_PSEUDOCOUNT] [--day-of-week-minimum-bucket-events DAY_OF_WEEK_MINIMUM_BUCKET_EVENTS] [--day-of-week-pseudocount DAY_OF_WEEK_PSEUDOCOUNT] [--day-of-year-minimum-bucket-events DAY_OF_YEAR_MINIMUM_BUCKET_EVENTS] [--day-of-year-pseudocount DAY_OF_YEAR_PSEUDOCOUNT] [--estimator-version ESTIMATOR_VERSION] [--output {human,json}]
```

### Options

- `--dataset-id`: stable identifier recorded in the diagnostics report
- `--event-records-json`: JSON object with top-level `event_records`
- `--calendar-registry-json`: `calendar.registry.v1` contract file
- `--exchange-code`: exchange identifier used for session classification
- `--timestamp-field-name`: timestamp field name in each event record, default `timestamp`
- `--session-filter {all,open_only,closed_only}`: accepted-state filter, default `open_only`
- `--sparse-hour-event-threshold`: sparse-hour diagnostic threshold, default `1`
- `--intraday-minutes-per-bin`: intraday estimator bin width, default `1`
- `--intraday-smoothing-kernel {gaussian,uniform}`: intraday smoothing kernel, default `gaussian`
- `--intraday-smoothing-bandwidth-bins`: intraday smoothing bandwidth in bins, default `3.0`
- `--intraday-minimum-bin-events`: intraday sparse-bin threshold, default `1`
- `--intraday-pseudocount`: intraday pseudocount, default `1e-6`
- `--day-of-week-minimum-bucket-events`: weekly sparse-bucket threshold, default `1`
- `--day-of-week-pseudocount`: weekly pseudocount, default `1e-6`
- `--day-of-year-minimum-bucket-events`: yearly sparse-bucket threshold, default `1`
- `--day-of-year-pseudocount`: yearly pseudocount, default `1e-6`
- `--estimator-version`: reproducibility metadata label, default `seasonality_estimator.v1`
- `--output {human,json}`: operator summary or machine-readable diagnostics report

### Example: Primary End-To-End Seasonality Diagnostics

```bash
janiform seasonality diagnostics \
  --dataset-id cli-docs-dataset \
  --event-records-json tmp/cli-docs/timestamp_event_records.json \
  --calendar-registry-json tmp/cli-docs/calendar_registry.json \
  --exchange-code XNYS \
  --session-filter all \
  --intraday-minutes-per-bin 30 \
  --output json
```

### Example: Diagnostics Output Redirected For Validation Tooling

```bash
janiform seasonality diagnostics \
  --dataset-id cli-docs-dataset \
  --event-records-json tmp/cli-docs/timestamp_event_records.json \
  --calendar-registry-json tmp/cli-docs/calendar_registry.json \
  --exchange-code XNYS \
  --session-filter all \
  --output json \
  > tmp/cli-docs/seasonality_diagnostics_report.json
```

### Standalone Preprocessing For Debugging

```text
janiform seasonality preprocess-timestamps --event-records-json PATH --calendar-registry-json PATH --exchange-code EXCHANGE_CODE [--timestamp-field-name TIMESTAMP_FIELD_NAME] [--session-filter {all,open_only,closed_only}] [--sparse-hour-event-threshold SPARSE_HOUR_EVENT_THRESHOLD] [--output {human,json}]
```

### Options

- `--event-records-json`: JSON object with top-level `event_records`
- `--calendar-registry-json`: `calendar.registry.v1` contract file
- `--exchange-code`: exchange identifier used for session classification
- `--timestamp-field-name`: timestamp field name in each event record, default `timestamp`
- `--session-filter {all,open_only,closed_only}`: accepted-state filter, default `open_only`
- `--sparse-hour-event-threshold`: sparse-hour diagnostic threshold, default `1`
- `--output {human,json}`: operator summary or machine-readable preprocessing payload

### Example: Standalone Preprocessing Before Downstream Seasonality

```bash
janiform seasonality preprocess-timestamps \
  --event-records-json tmp/cli-docs/timestamp_event_records.json \
  --calendar-registry-json tmp/cli-docs/calendar_registry.json \
  --exchange-code XNYS \
  --session-filter all \
  --sparse-hour-event-threshold 2 \
  --output json
```

### Example: Preprocessing Output Redirected For Calibration Triage

```bash
janiform seasonality preprocess-timestamps \
  --event-records-json tmp/cli-docs/timestamp_event_records.json \
  --calendar-registry-json tmp/cli-docs/calendar_registry.json \
  --exchange-code XNYS \
  --session-filter all \
  --sparse-hour-event-threshold 2 \
  --output json \
  > tmp/cli-docs/preprocessed_timestamp_events.json
```

## Pricing

### Synopsis

```text
janiform price --model-path PATH --valuation-time-utc VALUATION_TIME_UTC --spot FLOAT --strike FLOAT --maturity-interval FLOAT [--payoff-type {call,put}] [--risk-free-rate FLOAT] [--dividend-yield FLOAT] [--observed-boundary-state-json PATH] [--output-path PATH] [--silent] [--output {human,json}]
```

### Options

- `--model-path`: persisted `JaniformModel`
- `--valuation-time-utc`: valuation timestamp in UTC
- `--spot`: spot price
- `--strike`: option strike
- `--maturity-interval`: maturity in model time units
- `--observed-boundary-state-json`: explicit `MeanReversionBoundaryState` payload for anchored
  pricing. Required for mean-reverting pricing because the CLI never invents observed pricing
  state.
- `--output {human,json}`: operator summary or machine-readable result

### Example: Anchored Pricing With Explicit Observed Boundary State

```bash
janiform price \
  --model-path tmp/cli-docs/fitted_run/model.json \
  --observed-boundary-state-json tmp/cli-docs/observed_boundary_state.json \
  --valuation-time-utc 2026-02-02T15:00:00Z \
  --spot 100.0 \
  --strike 105.0 \
  --maturity-interval 1.0 \
  --output json
```

## Hourly Factors

Use these commands when you want reusable hourly `s_h`, hourly `nu_h`, or the combined contract
consumed by downstream simulation and pricing workflows. The recommended scriptable path is:

1. materialize `s_h` and `nu_h` to canonical report files when you start from timestamped source data
2. build the combined hourly-factor contract from those materialized artifacts
3. feed the contract-aligned arrays into simulation or pricing request preparation

### Materialize Hourly Seasonality

```text
janiform hourly-factors materialize-seasonality --calendar-registry-json PATH --exchange-code EXCHANGE_CODE --range-start-utc RANGE_START_UTC --range-end-utc RANGE_END_UTC --materialization-input-json PATH [--output {human,json}]
```

### Options

- `--calendar-registry-json`: `calendar.registry.v1` contract file
- `--exchange-code`: exchange identifier used for exchange-local hour labeling
- `--range-start-utc`: inclusive UTC start timestamp for the target hourly range
- `--range-end-utc`: exclusive UTC end timestamp for the target hourly range
- `--materialization-input-json`: JSON object with `source_timestamps_utc`,
  `source_seasonality_factors`, and optional settings fields
- `--output {human,json}`: operator summary or machine-readable materialization report

### Example: Materialize `s_h` And Save The Canonical Report

```bash
janiform hourly-factors materialize-seasonality \
  --calendar-registry-json tmp/cli-docs/calendar_registry.json \
  --exchange-code XNYS \
  --range-start-utc 2026-02-02T14:00:00Z \
  --range-end-utc 2026-02-02T17:00:00Z \
  --materialization-input-json tmp/cli-docs/hourly_factor_seasonality_materialization_input.json \
  --output json \
  > tmp/cli-docs/hourly_factor_seasonality_report.json
```

### Materialize Hourly News Intensity

```text
janiform hourly-factors materialize-news-intensity --calendar-registry-json PATH --exchange-code EXCHANGE_CODE --range-start-utc RANGE_START_UTC --range-end-utc RANGE_END_UTC --materialization-input-json PATH [--output {human,json}]
```

### Options

- `--calendar-registry-json`: `calendar.registry.v1` contract file
- `--exchange-code`: exchange identifier used for exchange-local hour labeling
- `--range-start-utc`: inclusive UTC start timestamp for the target hourly range
- `--range-end-utc`: exclusive UTC end timestamp for the target hourly range
- `--materialization-input-json`: JSON object with `known_scheduled_event_timestamps_utc`,
  `surprise_baseline_intensity`, and optional `surprise_deterministic_time_multiplier_by_interval_index`
- `--output {human,json}`: operator summary or machine-readable materialization report

### Example: Materialize `nu_h` And Save The Canonical Report

```bash
janiform hourly-factors materialize-news-intensity \
  --calendar-registry-json tmp/cli-docs/calendar_registry.json \
  --exchange-code XNYS \
  --range-start-utc 2026-02-02T14:00:00Z \
  --range-end-utc 2026-02-02T17:00:00Z \
  --materialization-input-json tmp/cli-docs/hourly_factor_news_materialization_input.json \
  --output json \
  > tmp/cli-docs/hourly_factor_news_report.json
```

### Build The Combined Hourly-Factor Contract

```text
janiform hourly-factors build-contract --calendar-registry-json PATH --exchange-code EXCHANGE_CODE --range-start-utc RANGE_START_UTC --range-end-utc RANGE_END_UTC (--seasonality-explicit-source-json PATH | --seasonality-materialized-source-json PATH) (--news-intensity-explicit-source-json PATH | --news-intensity-materialized-source-json PATH) [--output {human,json}]
```

### Options

- `--calendar-registry-json`: `calendar.registry.v1` contract file
- `--exchange-code`: exchange identifier used for exchange-local hour labeling
- `--range-start-utc`: inclusive UTC start timestamp for the target hourly range
- `--range-end-utc`: exclusive UTC end timestamp for the target hourly range
- `--seasonality-explicit-source-json`: explicit hourly `s_h` source with `hourly_factors`
- `--seasonality-materialized-source-json`: canonical report emitted by
  `hourly-factors materialize-seasonality`
- `--news-intensity-explicit-source-json`: explicit hourly `nu_h` source with `hourly_factors`
- `--news-intensity-materialized-source-json`: canonical report emitted by
  `hourly-factors materialize-news-intensity`
- `--output {human,json}`: operator summary or machine-readable contract

### Example: Build The Contract From Materialized Artifacts

```bash
janiform hourly-factors build-contract \
  --calendar-registry-json tmp/cli-docs/calendar_registry.json \
  --exchange-code XNYS \
  --range-start-utc 2026-02-02T14:00:00Z \
  --range-end-utc 2026-02-02T17:00:00Z \
  --seasonality-materialized-source-json tmp/cli-docs/hourly_factor_materialized_seasonality.json \
  --news-intensity-materialized-source-json tmp/cli-docs/hourly_factor_materialized_news_intensity.json \
  --output json \
  > tmp/cli-docs/hourly_factor_contract.json
```

### Example: Build The Contract From Explicit Precomputed Arrays

```bash
janiform hourly-factors build-contract \
  --calendar-registry-json tmp/cli-docs/calendar_registry.json \
  --exchange-code XNYS \
  --range-start-utc 2026-02-02T14:00:00Z \
  --range-end-utc 2026-02-02T17:00:00Z \
  --seasonality-explicit-source-json tmp/cli-docs/hourly_factor_explicit_seasonality_source.json \
  --news-intensity-explicit-source-json tmp/cli-docs/hourly_factor_explicit_news_source.json \
  --output json
```

### Integration Notes

- The materialization reports preserve the canonical report shapes used by the Python library.
- The combined contract preserves the canonical `seasonality.hourly_factor.contract.v1` shape.
- Simulation workflows still rehydrate runtime hourly-factor handles from their request summaries.
- Pricing workflows still keep `nu_h_known` and `nu_h_surprise` separate inside pricing execution
  inputs, so the combined `nu_h` contract is primarily a shared reusable artifact for upstream
  preparation and validation.

## Stylized Facts

### Synopsis

```text
janiform stylized-facts horizon-comparison --metric-catalog-json PATH --comparison-request-json PATH [--artifact-export-json PATH] [--execution-mode {disk,memory}] [--output {human,json}]
```

### Options

- `--metric-catalog-json`: stylized-fact metric catalog contract file
- `--comparison-request-json`: stylized-fact horizon-comparison request contract file
- `--artifact-export-json`: optional artifact-export config contract file
- `--execution-mode {disk,memory}`: report execution mode, default `memory`
- `--output {human,json}`: operator summary or machine-readable report bundle

### Execution-Mode Semantics

- If `--execution-mode` is omitted, the command uses `memory`.
- Use `--execution-mode disk` together with `--artifact-export-json` when deterministic report,
  table-summary, and plot-payload files should be written to disk.
- Artifact export config is passed through the same validation path used by
  `janiform.stylized_facts.run_stylized_fact_horizon_comparison_report_generator`.

### Example: In-Memory Report Bundle

```bash
janiform stylized-facts horizon-comparison \
  --metric-catalog-json tmp/cli-docs/stylized_fact_metric_catalog.json \
  --comparison-request-json tmp/cli-docs/stylized_fact_comparison_request.json \
  --execution-mode memory \
  --output json
```

### Example: Disk-Backed Artifact Export

```bash
janiform stylized-facts horizon-comparison \
  --metric-catalog-json tmp/cli-docs/stylized_fact_metric_catalog.json \
  --comparison-request-json tmp/cli-docs/stylized_fact_comparison_request.json \
  --artifact-export-json tmp/cli-docs/stylized_fact_artifact_export.json \
  --execution-mode disk \
  --output json
```

## Fit Staged

This is the explicit fit workflow boundary. Use it when you already have a small
`StagedCalibrationExecutionInput` wrapper JSON payload that points at a prepared calibration
artifact and want to run staged orchestration directly.

### Synopsis

```text
janiform fit staged --execution-input-json PATH --calendar-registry-json PATH [--execution-mode {disk,memory}] [--disk-checkpoint-directory DIRECTORY] [--output {human,json}]
```

### Options

- `--execution-input-json`: `StagedCalibrationExecutionInput` wrapper contract file
- `--calendar-registry-json`: `calendar.registry.v1` contract file
- `--execution-mode {disk,memory}`: CLI default is `disk`
- `--disk-checkpoint-directory`: required when the effective execution mode is `disk`
- `--output {human,json}`

### Example: Explicit Staged Fit Workflow With Memory Override

```bash
janiform fit staged \
  --execution-input-json tmp/cli-docs/calibration_execution_input.json \
  --calendar-registry-json tmp/cli-docs/calendar_registry.json \
  --execution-mode memory \
  --output json
```

## Fit Plan

This is the recommended calibration workflow for market users. Supply external files plus session
context and optional known-news or option-surface inputs, and Janiform derives the staged
calibration scope for you.

### Synopsis

```text
janiform fit plan --run-id TEXT --workflow-scope {seasonality_only,full_without_option_surface,full_with_option_surface} --tick-data-path PATH --tick-data-format {csv,parquet} --tick-data-provenance-label TEXT --tick-timestamp-column TEXT --exchange-code TEXT --timestamp-timezone TEXT [OPTIONS]
```

### Options

Required workflow inputs:

- `--run-id`
- `--workflow-scope {seasonality_only,full_without_option_surface,full_with_option_surface}`
- `--tick-data-path`
- `--tick-data-format {csv,parquet}`
- `--tick-data-provenance-label`
- `--tick-timestamp-column`
- `--exchange-code`
- `--timestamp-timezone`

Optional session-resolution overrides:

- `--session-definition-path`
- `--session-definition-format {csv,parquet,json}`
- `--session-definition-provenance-label`
- `--session-timezone`

Common normalization and output options:

- `--tick-price-input-policy {explicit_price_column,bid_ask_midpoint}`
- `--tick-price-column`
- `--tick-trade-price-column`
- `--tick-trade-sign-column`
- `--tick-bid-column`
- `--tick-ask-column`
- `--tick-close-price-column`
- `--duplicate-record-policy {keep_first,keep_last}`: Optional. Omit to preserve duplicate tick rows.
- `--disable-timestamp-sort`
- `--output {human,json}`
- `--execution-mode {disk,memory}`: CLI default is `disk`
- `--disk-checkpoint-directory`
- `--emit-derived-staged-execution-input`
- `--omit-input-provenance-report`

Known-news inputs:

- `--known-news-path`
- `--known-news-format {csv,parquet,json}`
- `--known-news-provenance-label`
- `--known-news-timezone`
- `--known-news-timestamp-column`
- `--known-news-event-id-column`

Optional option-surface inputs:

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

### Execution-Mode Semantics

- If `--execution-mode` is omitted, the CLI uses `disk`.
- When the effective execution mode is `disk`, you must provide `--disk-checkpoint-directory`.
- Use `--execution-mode memory` when you want an in-memory prepared staged contract without disk
  artifact targets.

### Example: CSV Tick Data With Known-News Calendar And Automatic Session Inference

This example shows the default calibration behavior on the CLI. It omits `--execution-mode`, so
the command uses `disk` and therefore includes `--disk-checkpoint-directory`.

```bash
janiform fit plan \
  --run-id cli-market-data-default-disk \
  --workflow-scope seasonality_only \
  --tick-data-path tmp/cli-docs/data/ticks.csv \
  --tick-data-format csv \
  --tick-data-provenance-label primary-tick-feed \
  --tick-timestamp-column timestamp_utc \
  --exchange-code XNYS \
  --known-news-path tmp/cli-docs/config/known-news.csv \
  --known-news-format csv \
  --known-news-provenance-label known-news-calendar \
  --known-news-timezone UTC \
  --known-news-timestamp-column event_timestamp_utc \
  --known-news-event-id-column event_code \
  --timestamp-timezone UTC \
  --disk-checkpoint-directory tmp/cli-docs/checkpoints/market-data-default-disk \
  --output json
```

### Example: Parquet Tick Data Plus Option Surface With Explicit Memory Override

This example shows the same market-data-first boundary with:

- `parquet` tick data
- a known-news calendar
- an optional option-surface file
- explicit `--execution-mode memory`

```bash
janiform fit plan \
  --run-id cli-market-data-memory-option-surface \
  --workflow-scope full_with_option_surface \
  --tick-data-path tmp/cli-docs/data/ticks.parquet \
  --tick-data-format parquet \
  --tick-data-provenance-label primary-tick-feed \
  --tick-timestamp-column timestamp_utc \
  --tick-price-input-policy explicit_price_column \
  --tick-price-column trade_price \
  --tick-trade-sign-column trade_sign \
  --exchange-code XNYS \
  --known-news-path tmp/cli-docs/config/known-news.csv \
  --known-news-format csv \
  --known-news-provenance-label known-news-calendar \
  --known-news-timezone UTC \
  --known-news-timestamp-column event_timestamp_utc \
  --known-news-event-id-column event_code \
  --option-surface-path tmp/cli-docs/data/options.parquet \
  --option-surface-format parquet \
  --option-surface-provenance-label options-close \
  --option-quote-input-policy bid_ask_midpoint \
  --option-quote-timestamp-column quote_timestamp_utc \
  --option-expiry-column expiry_utc \
  --option-strike-column strike \
  --option-type-column option_type \
  --option-bid-column bid \
  --option-ask-column ask \
  --timestamp-timezone UTC \
  --execution-mode memory \
  --output json
```

### Machine-Readable Output Example

```bash
janiform fit plan \
  --run-id cli-market-data-json \
  --workflow-scope seasonality_only \
  --tick-data-path tmp/cli-docs/data/ticks.csv \
  --tick-data-format csv \
  --tick-data-provenance-label primary-tick-feed \
  --tick-timestamp-column timestamp_utc \
  --exchange-code XNYS \
  --timestamp-timezone UTC \
  --disk-checkpoint-directory tmp/cli-docs/checkpoints/market-data-json \
  --output json \
  > tmp/cli-docs/market_data_summary.json
```


## Developer Regression

These commands live under `dev regression` because they are intended for validation,
fixture maintenance, and CI checks rather than end-user workflows.

### Deterministic Calendar And Hourly-Factor Harness

```text
janiform dev regression deterministic --calendar-registry-json PATH --calendar-cases-json PATH --seasonality-cases-json PATH [--seasonality-absolute-tolerance FLOAT] [--news-intensity-absolute-tolerance FLOAT] [--output {human,json}]
```

Options:

- `--calendar-registry-json`: `calendar.registry.v1` contract file or wrapper with `calendar_registry`
- `--calendar-cases-json`: calendar regression fixture document with top-level `cases`
- `--seasonality-cases-json`: hourly-factor regression fixture document with top-level `canonical_output_cases`
- `--seasonality-absolute-tolerance FLOAT`: absolute tolerance for `s_h` comparisons
- `--news-intensity-absolute-tolerance FLOAT`: absolute tolerance for `nu_h` comparisons
- `--output {human,json}`: human-readable summary or full machine-readable harness report

Example:

```bash
janiform dev regression deterministic \
  --calendar-registry-json tests/fixtures/calendar/calendar_service_regression_matrix.json \
  --calendar-cases-json tests/fixtures/calendar/calendar_service_regression_matrix.json \
  --seasonality-cases-json tests/fixtures/hourly_factors/hourly_factor_regression_matrix.json \
  --output json
```

This command reuses the public Python entrypoint `janiform.validation.run_deterministic_regression_harness`
and emits `validation.deterministic_regression_harness.report.v1` when `--output json` is selected.

## See Also

- `docs/development/guides/cli_command_architecture_and_namespace_layout.md`
- `examples/README.md`
