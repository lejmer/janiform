# CLI Command Reference And Examples

Document version: `cli.command_reference.pre_v0_1.milestone_1`

Status: active for the current pre-v0.1 milestone on March 8, 2026.

## Purpose

This guide is the operator-facing command reference for Janiform's current CLI workflows.
It complements the namespace and contract guide in
`docs/development/guides/cli_command_architecture_and_namespace_layout.md` by focusing on:

- command synopsis
- option summaries
- copy-pastable example invocations
- machine-readable output examples for automation

The primary calibration workflow is market-data-first. The staged execution JSON path is retained
for advanced or internal use.

## Shared Setup

Run the examples from the repository root after bootstrapping the Python environment.
The setup command below writes JSON requests, session definitions, known-news calendars,
tick-data files, and option-surface files under `tmp/cli-docs`.

```bash
PYTHONPATH=python python3 -m examples.cli_documentation_setup --output-dir tmp/cli-docs
```

The generated files include:

- `tmp/cli-docs/calendar_registry.json`
- `tmp/cli-docs/simulation_request.json`
- `tmp/cli-docs/pricing_execution_input.json`
- `tmp/cli-docs/calibration_execution_input.json`
- `tmp/cli-docs/data/ticks.csv`
- `tmp/cli-docs/data/ticks.parquet`
- `tmp/cli-docs/data/options.parquet`
- `tmp/cli-docs/config/sessions.json`
- `tmp/cli-docs/config/known-news.csv`

## Shared Conventions

- Primary workflow commands accept `--output {human,json}`.
- `--output json` is the machine-readable mode for automation and CI.
- Relative paths are resolved from the repository root.
- Calibration commands share `--execution-mode {disk,memory}`.
- The CLI default is `disk` for calibration commands.
- `--disk-checkpoint-directory` is required when the effective calibration execution mode is `disk`.

## Simulation

### Synopsis

```text
janiform simulation seeded-ticks --request-json PATH --calendar-registry-json PATH [--output {human,json}]
```

### Options

- `--request-json`: `SeededTickSimulationRequest` contract file
- `--calendar-registry-json`: `calendar.registry.v1` contract file used to rehydrate runtime inputs
- `--output {human,json}`: operator summary or machine-readable result

### Example

Machine-readable output to stdout:

```bash
janiform simulation seeded-ticks \
  --request-json tmp/cli-docs/simulation_request.json \
  --calendar-registry-json tmp/cli-docs/calendar_registry.json \
  --output json
```

Machine-readable output redirected to a file:

```bash
janiform simulation seeded-ticks \
  --request-json tmp/cli-docs/simulation_request.json \
  --calendar-registry-json tmp/cli-docs/calendar_registry.json \
  --output json \
  > tmp/cli-docs/simulation_result.json
```

## Pricing

### Synopsis

```text
janiform pricing vanilla-option --execution-input-json PATH [--output {human,json}]
```

### Options

- `--execution-input-json`: `VanillaOptionPricingExecutionInput` contract file
- `--output {human,json}`: operator summary or machine-readable result

### Example

```bash
janiform pricing vanilla-option \
  --execution-input-json tmp/cli-docs/pricing_execution_input.json \
  --output json
```

## Market-Data Calibration

This is the recommended calibration workflow for market users. Supply external files plus session
context and optional known-news or option-surface inputs, and Janiform derives the staged
calibration scope for you.

### Synopsis

```text
janiform calibration market-data --run-id TEXT --workflow-scope {seasonality_only,full_without_option_surface,full_with_option_surface} --tick-data-path PATH --tick-data-format {csv,parquet} --tick-data-provenance-label TEXT --tick-timestamp-column TEXT --exchange-code TEXT --session-definition-path PATH --session-definition-format {csv,parquet,json} --session-definition-provenance-label TEXT --session-timezone TEXT --timestamp-timezone TEXT [OPTIONS]
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
- `--session-definition-path`
- `--session-definition-format {csv,parquet,json}`
- `--session-definition-provenance-label`
- `--session-timezone`
- `--timestamp-timezone`

Common normalization and output options:

- `--tick-price-input-policy {explicit_price_column,trade_price_column,bid_ask_midpoint,close_price_column}`
- `--tick-price-column`
- `--tick-trade-price-column`
- `--tick-bid-column`
- `--tick-ask-column`
- `--tick-close-price-column`
- `--duplicate-record-policy {error,keep_first,keep_last}`
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

### Example: CSV Tick Data With Known-News Calendar And Default Disk Mode

This example shows the default calibration behavior on the CLI. It omits `--execution-mode`, so
the command uses `disk` and therefore includes `--disk-checkpoint-directory`.

```bash
janiform calibration market-data \
  --run-id cli-market-data-default-disk \
  --workflow-scope seasonality_only \
  --tick-data-path tmp/cli-docs/data/ticks.csv \
  --tick-data-format csv \
  --tick-data-provenance-label primary-tick-feed \
  --tick-timestamp-column timestamp_utc \
  --exchange-code XNYS \
  --session-definition-path tmp/cli-docs/config/sessions.json \
  --session-definition-format json \
  --session-definition-provenance-label session-calendar \
  --session-timezone America/New_York \
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
janiform calibration market-data \
  --run-id cli-market-data-memory-option-surface \
  --workflow-scope full_with_option_surface \
  --tick-data-path tmp/cli-docs/data/ticks.parquet \
  --tick-data-format parquet \
  --tick-data-provenance-label primary-tick-feed \
  --tick-timestamp-column timestamp_utc \
  --tick-price-input-policy trade_price_column \
  --tick-trade-price-column trade_price \
  --exchange-code XNYS \
  --session-definition-path tmp/cli-docs/config/sessions.json \
  --session-definition-format json \
  --session-definition-provenance-label session-calendar \
  --session-timezone America/New_York \
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
janiform calibration market-data \
  --run-id cli-market-data-json \
  --workflow-scope seasonality_only \
  --tick-data-path tmp/cli-docs/data/ticks.csv \
  --tick-data-format csv \
  --tick-data-provenance-label primary-tick-feed \
  --tick-timestamp-column timestamp_utc \
  --exchange-code XNYS \
  --session-definition-path tmp/cli-docs/config/sessions.json \
  --session-definition-format json \
  --session-definition-provenance-label session-calendar \
  --session-timezone America/New_York \
  --timestamp-timezone UTC \
  --disk-checkpoint-directory tmp/cli-docs/checkpoints/market-data-json \
  --output json \
  > tmp/cli-docs/market_data_summary.json
```

## Advanced Staged Calibration

This is the advanced or internal calibration path. Use it when you already have a
`StagedCalibrationExecutionInput` JSON payload and want to run staged orchestration directly.

### Synopsis

```text
janiform calibration staged --execution-input-json PATH --calendar-registry-json PATH [--execution-mode {disk,memory}] [--disk-checkpoint-directory DIRECTORY] [--output {human,json}]
```

### Options

- `--execution-input-json`: `StagedCalibrationExecutionInput` contract file
- `--calendar-registry-json`: `calendar.registry.v1` contract file
- `--execution-mode {disk,memory}`: CLI default is `disk`
- `--disk-checkpoint-directory`: required when the effective execution mode is `disk`
- `--output {human,json}`

### Example: Advanced Or Internal Workflow With Explicit Memory Override

```bash
janiform calibration staged \
  --execution-input-json tmp/cli-docs/calibration_execution_input.json \
  --calendar-registry-json tmp/cli-docs/calendar_registry.json \
  --execution-mode memory \
  --output json
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
