# CLI Command Architecture And Namespace Layout

Document version: `cli.command_architecture.pre_v0_1.milestone_1`

Status: active for the current pre-v0.1 milestone on March 8, 2026.

## Purpose

This guide defines the top-level Janiform CLI command tree for the current pre-v0.1
workflow surface. It keeps the primary user-facing workflows separate from internal
developer utilities and aligns command names with the public Python workflow modules.

## Command tree

```text
janiform
├── simulation
│   └── seeded-ticks
├── calibration
│   ├── market-data
│   └── staged
├── pricing
│   └── vanilla-option
└── dev
    └── regression
        └── open-closed-gap-leakage
```

## Namespace policy

- `simulation`, `calibration`, and `pricing` are the primary user-facing workflow groups.
- `dev` is reserved for internal and developer-only utilities.
- Developer-only commands must not be placed beside primary workflow groups at the top level.
- Command aliases are forbidden because they add ambiguity to automation and shell scripts.

## Naming policy

- Top-level command groups use the same workflow nouns as the public Python modules:
  `janiform.simulation`, `janiform.calibration`, and `janiform.pricing`.
- Subcommand names stay short while preserving the specific workflow identity:
  - `seeded-ticks` for the seeded tick-event simulator
  - `market-data` for the recommended file-driven calibration request boundary
  - `staged` for advanced staged calibration orchestration
  - `vanilla-option` for vanilla option pricing
- Developer-only validation harnesses remain explicit about their function:
  - `dev regression open-closed-gap-leakage`

## Help-output expectations

- `janiform --help` lists the four top-level namespaces in the order above.
- `janiform simulation --help`, `janiform calibration --help`, and `janiform pricing --help`
  each list their current workflow subcommands.
- `janiform dev --help` lists internal-only utility groups.
- Incomplete command paths print the help for the selected namespace branch.

## Common argument standards

- Shared concepts use long flags only. Short aliases are forbidden for script readability.
- File and directory inputs use explicit path-named flags such as `--request-json`,
  `--execution-input-json`, `--calendar-registry-json`, and `--disk-checkpoint-directory`.
- CLI help uses `PATH` for file-like values and `DIRECTORY` for directory overrides so
  the shape of each argument is consistent across commands.
- All CLI path flags accept absolute paths. Relative paths are resolved from the
  repository root so scripts behave the same even when invoked from another directory.
- Shared path flags keep the same semantics wherever they appear:
  - `--calendar-registry-json` always means a `calendar.registry.v1` payload or wrapper
    used to rehydrate runtime calendar-aware inputs.
  - `--execution-input-json` always means the typed workflow execution-input contract for
    the selected command.

## Current workflow command contracts

The current pre-v0.1 CLI uses explicit JSON contract files at the command boundary so the
command surface stays aligned with the typed Python workflow models.

### `janiform simulation seeded-ticks`

Required arguments:
- `--request-json`: `SeededTickSimulationRequest` JSON object
- `--calendar-registry-json`: `calendar.registry.v1` JSON object or a wrapper object with `calendar_registry`

Behavior:
- The command rehydrates the runtime hourly-factor handle from the request's
  `hourly_factor_summary` plus the supplied calendar registry.
- Validation fails if the public request summary does not reproduce the same hourly-factor contract.

Output:
- Writes deterministic JSON for `simulation.seeded_tick_event.path.v1` to stdout.

### `janiform pricing vanilla-option`

Required arguments:
- `--execution-input-json`: `VanillaOptionPricingExecutionInput` JSON object

Behavior:
- The command validates the execution input directly against the Python pricing contract.
- Interval factors keep `nu_h_known` and `nu_h_surprise` separate.

Output:
- Writes deterministic JSON for `pricing.vanilla_option.run_result.v1` to stdout.

### `janiform calibration market-data`

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
- `--tick-price-input-policy {explicit_price_column,trade_price_column,bid_ask_midpoint,close_price_column}`
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
- The command validates file-driven request fields, provenance labels, and normalization-policy selections.
- The command writes deterministic JSON for `calibration.market_data.request.v1` to stdout.
- Translation into the canonical staged execution input remains a lower-level step documented separately.

### `janiform calibration staged`

Required arguments:
- `--execution-input-json`: `StagedCalibrationExecutionInput` JSON object
- `--calendar-registry-json`: `calendar.registry.v1` JSON object or a wrapper object with `calendar_registry`

Optional arguments:
- `--execution-mode {disk,memory}`: defaults to `disk` on the CLI
- `--disk-checkpoint-directory`: required when the effective execution mode is `disk` and the input JSON does not already provide one

Behavior:
- This is the advanced or internal calibration command path.
- Option precedence is explicit:
  1. `--execution-mode`, when supplied, overrides `request.execution_mode` from `--execution-input-json`.
  2. When `--execution-mode` is omitted, the CLI default `disk` still overrides the
     execution-input payload so the command behaves predictably in automation.
  3. `--disk-checkpoint-directory`, when supplied, overrides
     `runtime_resources.disk_checkpoint_directory` from `--execution-input-json`.
- Validation rejects `--disk-checkpoint-directory` when `--execution-mode memory` is selected.
- Return-components stage inputs keep known-news open-jump and closed-gap semantics explicit.

Output:
- Writes deterministic JSON for `calibration.orchestration.staged_run.report.v1` to stdout.
