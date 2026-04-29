# CLI Reference

This is a curated reference for the public CLI surface. Use `janiform <command> --help` for the complete live help text.

## Top-Level Commands

| Command | Purpose |
| --- | --- |
| `janiform fit` | Fit a Janiform model from market data |
| `janiform simulate` | Simulate from a persisted model |
| `janiform price` | Price a vanilla option from a persisted model |
| `janiform inspect` | Inspect persisted runs and fitted model artifacts |
| `janiform report` | Build artifact-oriented reports from persisted runs |
| `janiform presets` | Inspect bundled and user-registered presets |
| `janiform seasonality` | Seasonality preprocessing workflows |
| `janiform hourly-factors` | Hourly factor materialization and contract-building workflows |
| `janiform validate` | Model validation workflows |
| `janiform dev regression deterministic` | Developer deterministic regression harness |

## High-Level Workflow Selection

| Need | Command |
| --- | --- |
| Validate and derive a fit plan | `janiform fit plan` |
| Run the prepared staged calibration boundary | `janiform fit staged` |
| Simulate from a persisted model | `janiform simulate --model-path MODEL.jf --horizon-interval-count COUNT` |
| Simulate a coarse interval path from a persisted model | `janiform simulate --model-path MODEL.jf --coarse --horizon-interval-count COUNT` |
| Run from an explicit coarse request JSON | `janiform simulate coarse-interval --request-json REQUEST.json` |
| Run from an explicit seeded request JSON | `janiform simulate seeded-ticks --request-json REQUEST.json --calendar-registry-json CALENDAR.json` |
| Inspect a fit run directory | `janiform inspect run RUN_DIR` |
| Inspect a fitted model artifact | `janiform inspect model MODEL_PATH` |
| Render a fit report | `janiform report fit RUN_OR_REPORT_PATH --html` |
| Validate fitted model stylized facts | `janiform validate stylized-facts --model-path MODEL.jf --intervals 5m,1h,1d` |

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
| `--quiet` | Limits human output to final status and artifact paths. |
| `--events jsonl` | Writes machine events to stderr as JSONL. |
| `--trace-events` | Enables trace-level records and requires an event stream. |
| `--color auto\|always\|never` | Controls ANSI styling for human diagnostics. Automatic color respects `NO_COLOR` and terminal capability. JSON and JSONL outputs do not contain ANSI styling. |
| `--config PATH` | Loads TOML configuration for CLI boundary values only, such as output and diagnostic defaults. Model behavior, numerical policy, and workflow-specific calibration defaults remain in typed Rust contracts. |

`--json`, `--silent`, and `--quiet` are mutually exclusive where their contracts would conflict.

## Inspection And Reports

| Command | Contract |
| --- | --- |
| `janiform inspect run RUN_DIR` | Shows run state, stages, artifacts, checkpoints, and warnings from a persisted run directory. Add `--include-events` for event-log metadata. |
| `janiform inspect model MODEL_PATH` | Shows model identity, provenance, fitted components, capabilities, and main fitted parameter families. Add `--parameters` for fuller compact parameter summaries. |
| `janiform report fit RUN_OR_REPORT_PATH` | Shows a concise fit report from a run directory or `report.json`. Add `--html` or `--output-path PATH` for a structured HTML artifact. |

`inspect` and `report` read persisted artifacts only. `report fit --json` and `report fit --html`
include presentation-only `parameter_fits` from the model artifact when available and do not rewrite
the source `report.json`.

## Presets

| Command | Contract |
| --- | --- |
| `janiform presets list` | Shows bundled and user-registered presets. |
| `janiform presets show CATEGORY NAME` | Shows one resolved preset artifact. Add `--expanded` for known-news event rows or session window rows in human output. |
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
| `janiform hourly-factors materialize-seasonality` | Materializes interval-aligned `s_h` factors from source timestamps and seasonality factors. |
| `janiform hourly-factors materialize-news-intensity` | Materializes interval-aligned `nu_h` factors from scheduled news timestamps and surprise intensity controls. |
| `janiform hourly-factors build-contract` | Builds a combined hourly factor contract from explicit factor source documents or canonical materialization reports. |

Seasonality commands use `--event-records`, optional `--event-records-format`,
`--timestamp-column`, `--timestamp-unit`, `--calendar-registry-json`,
`--exchange-code`, and optional session filtering controls. Hourly-factor
commands use `--calendar-registry-json`, `--exchange-code`, `--range-start-utc`,
`--range-end-utc`, and `--interval`; materialization commands also take
`--materialization-input-json`.

## Validation

| Command | Contract |
| --- | --- |
| `janiform validate stylized-facts` | Loads a fitted model and empirical tick data, simulates seeded paths over the empirical span, computes return-based stylized-fact metrics, compares empirical and simulated metrics, and emits a validation report bundle. |
| `janiform validate stylized-facts plan` | Resolves the validation computation plan without simulation or metric computation. Human output shows the compact plan; `--json` emits the resolved plan contract. |

Raw-data mode requires `--model-path` and `--intervals`. When `--empirical-data-path` is omitted,
the workflow uses `metadata.input_inference.tick_data_source` from the model artifact when present.
Use `--empirical-data-path` when the model artifact does not carry that source reference or when
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
`--cache=full` retain intermediate validation work products. The derived cache includes the
generated comparison request, simulation manifest, and derived metric observations. Full cache also
writes generated seeded tick paths.

Human output is an analytical report with summary counts, failed or closest checks, coverage notes
when coverage is incomplete, and artifact paths. `--fail-on-failed-checks`
returns a nonzero process exit when metric checks fail. `--show-checks summary|failed|closest|all`
and `--max-checks N` control check detail in human output.

## Notes

- `fit plan` defaults to an automation-friendly disk-oriented execution policy.
- `fit` chooses the fit from the supplied inputs, so adding `--option-surface-path` fits the model
  to the underlying data *and* the selected option snapshot.
- `fit staged` is the explicit orchestration boundary, not the recommended starting point.
- `simulate` reads a fitted `model.jf` artifact through `--model-path`. It defaults to seeded-tick
  generation, uses `--interval 1h` for horizon units when omitted, and switches to coarse-interval
  generation only when `--coarse` is supplied.
- `simulate --seed` defaults to `0`, `--start-time-utc` defaults to the current UTC timestamp, and
  `--output-path` defaults to `.janiform/runs/<run-id>/simulation.<model-default-format>`.
- `simulate --output-batch-size` controls exported artifact batching and defaults to `256` rows.
- `simulate seeded-ticks` and `simulate coarse-interval` are direct request-JSON boundaries for
  workflows that already have native simulation request artifacts.
- `price` derives its pricing state from the persisted model, the valuation timestamp, and the supplied spot.
