# CLI Reference

This is a curated reference for the public CLI surface. Use `janiform <command> --help` for the complete live help text.

## Top-Level Commands

| Command | Purpose |
| --- | --- |
| `janiform fit` | Fit a Janiform model from market data |
| `janiform simulate` | Simulate market paths from a saved model |
| `janiform price` | Price derivatives with a fitted model |
| `janiform inspect` | Inspect saved runs and fitted models |
| `janiform report` | Render reports from saved Janiform outputs |
| `janiform presets` | Inspect bundled and user-registered presets |
| `janiform seasonality` | Seasonality preprocessing workflows |
| `janiform hourly-factors` | Prepare hourly seasonality and news-intensity factors |
| `janiform validate` | Model validation workflows |
| `janiform dev regression deterministic` | Developer deterministic regression harness |

## Workflow Selection

| Need | Command |
| --- | --- |
| Validate and derive a fit plan | `janiform fit plan` |
| Run from a prepared calibration input file | `janiform fit staged` |
| Simulate seeded tick paths from a saved model | `janiform simulate --model-path MODEL.jf --horizon-interval-count COUNT` |
| Simulate interval-level paths from a saved model | `janiform simulate --model-path MODEL.jf --coarse --horizon-interval-count COUNT` |
| Run from a prepared interval simulation input | `janiform simulate coarse-interval --request-json REQUEST.json` |
| Run from a prepared seeded-tick simulation input | `janiform simulate seeded-ticks --request-json REQUEST.json --calendar-registry-json CALENDAR.json` |
| Inspect a fit run directory | `janiform inspect run RUN_DIR` |
| Inspect a fitted model | `janiform inspect model MODEL_PATH` |
| Render a fit report | `janiform report fit RUN_OR_REPORT_PATH --html` |
| Validate fitted model stylized facts | `janiform validate stylized-facts --model-path MODEL.jf --intervals 5min,1h,1D` |

## Common Flags

| Area | Representative flags |
| --- | --- |
| Output | `--json`, `--silent`, `--quiet` |
| Diagnostics | `--color {auto,always,never}`, `--verbose`, `--config` |
| Events | `--events jsonl`, `--trace-events` |
| Persistence | `--output-dir`, `--output-path`, `--disk-checkpoint-directory` |
| Data inputs | `--tick-data-path`, `--known-news-path`, `--option-surface-path`, `--calendar-registry-json` |
| Workflow shape | `--workflow-scope`, `--execution-mode` |
| Calibration search | `--dynamics-search-restart-candidate-budget` |
| Simulation horizon | `--seed`, `--start-time-utc`, `--horizon-interval-count`, `--interval`, `--coarse` |

## Shared CLI Boundary

Root-level options are available to every command.

| Option | Contract |
| --- | --- |
| `--json` | Writes one final JSON object on stdout and keeps human progress off stdout. |
| `--silent` | Suppresses normal output while preserving warnings and errors. |
| `--quiet` | Limits human output to final status and output paths. |
| `--events jsonl` | Writes machine events to stderr as JSONL. |
| `--trace-events` | Enables trace-level records and requires an event stream. |
| `--color auto\|always\|never` | Controls ANSI styling for human diagnostics. Automatic color respects `NO_COLOR` and terminal capability. JSON and JSONL outputs do not contain ANSI styling. |
| `--config PATH` | Loads TOML configuration for CLI output and diagnostic defaults. Model choices, numerical settings, and command-specific inputs stay explicit on each command. |

`--json`, `--silent`, and `--quiet` are mutually exclusive where their output modes would conflict.

## Inspection And Reports

| Command | Contract |
| --- | --- |
| `janiform inspect run RUN_DIR` | Shows run state, stages, outputs, checkpoints, and warnings from a saved run directory. Add `--include-events` for progress-log metadata. |
| `janiform inspect model MODEL_PATH` | Shows model identity, provenance, fitted components, capabilities, and main fitted parameter families. Add `--parameters` for fuller compact parameter summaries. |
| `janiform report fit RUN_OR_REPORT_PATH` | Shows a concise fit report from a run directory or `report.json`. Add `--html` or `--output-path PATH` for a structured HTML report. |

`inspect` and `report` read saved outputs only. `report fit --json` and `report fit --html`
include presentation-only `parameter_fits` from the model file when available and do not rewrite
`report.json`.

## Presets

| Command | Contract |
| --- | --- |
| `janiform presets list` | Shows bundled and user-registered presets. |
| `janiform presets show CATEGORY NAME` | Shows one resolved preset. Add `--expanded` for known-news event rows or session window rows in human output. |
| `janiform presets export CATEGORY NAME` | Writes a resolved preset export with nested dependencies. |

Preset commands accept repeatable `--user-preset PATH` flags and repeatable
`--preset-registry PATH` manifest flags. Registry manifests use
`schema_version: "preset.user_registry.v1"` and a `preset_artifact_paths` array.
Manifest artifact paths resolve relative to the manifest file unless absolute.

## Seasonality And Hourly Factors

| Command | Contract |
| --- | --- |
| `janiform seasonality preprocess-timestamps` | Loads timestamp events from CSV, parquet, or JSONL, resolves the calendar registry, and emits preprocessed timestamp diagnostics. |
| `janiform seasonality diagnostics` | Runs timestamp preprocessing, intraday estimation, day-of-week estimation, day-of-year estimation, deterministic signal composition, and diagnostics report generation. |
| `janiform hourly-factors materialize-seasonality` | Builds interval-aligned `s_h` factors from timestamps and seasonality factors. |
| `janiform hourly-factors materialize-news-intensity` | Builds interval-aligned `nu_h` factors from scheduled news timestamps and surprise intensity controls. |
| `janiform hourly-factors build-contract` | Combines seasonality and news-intensity inputs for downstream model use. |

Seasonality commands use `--event-records`, optional `--event-records-format`,
`--timestamp-column`, `--timestamp-unit`, `--calendar-registry-json`,
`--exchange-code`, and optional session filtering controls. Hourly-factor
commands use `--calendar-registry-json`, `--exchange-code`, `--range-start-utc`,
`--range-end-utc`, and `--interval`; materialization commands also take
`--materialization-input-json`.

## Validation

| Command | Contract |
| --- | --- |
| `janiform validate stylized-facts` | Compares empirical tick data with synthetic paths generated from a fitted model, computes return-based stylized-fact metrics, and emits a validation report. |
| `janiform validate stylized-facts plan` | Resolves the validation computation plan without simulation or metric computation. Human output shows the compact plan; `--json` emits the resolved plan. |

Raw-data mode requires `--model-path` and `--intervals`. When `--empirical-data-path` is omitted,
the workflow uses the tick-data input recorded with the fitted model when present.
Use `--empirical-data-path` when the saved model does not carry that input reference or when
validating against another empirical sample. The empirical format is inferred from `.csv`,
`.parquet`, `.pq`, or `.json` when `--empirical-data-format` is omitted.
`--simulation-path-count` defaults to `1`, `--seed` defaults to `0`, and `--output-dir` defaults
under the fitted model's validation directory.

Use `--comparison-request PATH` for an advanced precomputed comparison request. In that mode the
command skips empirical loading, simulation, and metric computation. `--metric-catalog PATH`
overrides the built-in catalog.

Without `--threshold-policy`, decisions use the built-in bootstrap confidence intervals and compare
the empirical/simulated delta against the pooled interval half-width. `--significance-level`
overrides the catalog's default confidence level in that statistical mode.

Every report output directory includes `lineage.json`. `--cache`, `--cache=derived`, and
`--cache=full` retain intermediate validation results. The derived cache includes intermediate
comparison data, simulation metadata, and metric observations. Full cache also
writes generated seeded tick paths.

Human output is an analytical report with summary counts, failed or closest checks, coverage notes
when coverage is incomplete, and output paths. `--fail-on-failed-checks`
returns a nonzero process exit when metric checks fail. `--show-checks summary|failed|closest|all`
and `--max-checks N` control check detail in human output.

## Notes

- `fit plan` defaults to an automation-friendly disk-oriented execution policy.
- `fit` chooses the fit from the supplied inputs, so adding `--option-surface-path` fits the model
  to the underlying data *and* the selected option snapshot.
- `fit staged` runs from a prepared calibration input file and is not the recommended starting point.
- `simulate` reads a fitted `model.jf` file through `--model-path`. It defaults to seeded-tick
  generation, uses `--interval 1h` for horizon units when omitted, and switches to coarse-interval
  generation only when `--coarse` is supplied.
- `simulate --seed` defaults to `0`, `--start-time-utc` defaults to the current UTC timestamp, and
  `--output-path` defaults to `.janiform/runs/<run-id>/simulation.<model-default-format>`.
- `simulate --output-batch-size` controls export batching and defaults to `256` rows.
- `simulate seeded-ticks` and `simulate coarse-interval` run from prepared simulation input files.
- `price` derives its pricing state from the saved model, the valuation timestamp, and the supplied spot.
