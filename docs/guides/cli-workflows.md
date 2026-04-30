# CLI Workflows

Use the main commands first. Drop to advanced input-file subcommands only when another tool has already prepared the input file for Janiform.

## Fit

- `janiform fit` validates market data, calibrates the Janiform model, and saves the fitted model and fit report.
- `janiform fit plan` validates inputs and previews the planned run without running calibration.
- `janiform fit staged` runs from a prepared calibration input file.

`janiform fit` chooses the fit from the supplied inputs:

- tick data only fits the underlying model
- tick data plus an option surface fits the model to the underlying data and the selected option snapshot

Use `fit plan` when you want resolved inputs, output paths, and execution policy reviewed before
execution. Use `fit staged` when another tool has already prepared the calibration input file.

## Simulate

- `janiform simulate --model-path MODEL.jf --horizon-interval-count COUNT` generates seeded tick
  paths from a saved Janiform model.
- `janiform simulate --model-path MODEL.jf --coarse --horizon-interval-count COUNT` generates
  interval-level paths from a saved Janiform model.
- `janiform simulate seeded-ticks` runs from a prepared seeded-tick simulation input file.
- `janiform simulate coarse-interval` runs from a prepared interval simulation input file.

The main simulate command defaults to seeded tick paths. `--interval` controls the horizon unit and
defaults to `1h`; it does not select the path kind. Add `--coarse` when you want interval-level
paths instead of tick-level output.

`--seed` defaults to `0`, `--start-time-utc` defaults to the current UTC timestamp, and
`--output-path` defaults under `.janiform/runs/<run-id>/`. Use `--output-batch-size` to control
export batching when writing large simulation outputs.

## Price

`janiform price` values derivatives with a fitted Janiform model. The default command prices one
vanilla option from the saved model and the supplied valuation inputs.

## Inspect

- `janiform inspect run RUN_DIR` shows the current or completed state of a saved run.
- `janiform inspect model MODEL_PATH` shows model identity, provenance, fitted components,
  capabilities, and fitted parameter summaries.

Use `inspect run` while a fit is running or after it completes when you need a status-oriented view
of the run directory. The command reads `latest-state.json` first and falls back to replaying
`events.jsonl` only when the snapshot is missing or stale. The replay path tolerates a trailing
in-progress JSONL row so a concurrent event write does not make inspection fail.

Use `inspect model` when you already have a saved `model.jf` file and need to understand what was fit
and what tasks the model supports. Add `--parameters` for fuller parameter summaries that stay
compact enough for terminal output.

## Report

- `janiform report fit RUN_OR_REPORT_PATH` summarizes a fit report from a run directory or from a
  `report.json` file.
- `janiform report fit RUN_OR_REPORT_PATH --html` writes a structured HTML report.
- `janiform --json report fit RUN_OR_REPORT_PATH` writes a machine-readable report view.

Use `report fit` after a fit run when the natural question is "what did this calibration produce?"
rather than "what is the run doing right now?" The default terminal view stays concise. JSON and
HTML outputs include presentation-only parameter fits extracted from the sibling model file
when available. Those parameter fits are not written back into `report.json`.

## Validate

- `janiform validate stylized-facts plan --model-path MODEL.jf --intervals 5min,1h,1D` shows the validation computation plan, using the fitted model's recorded tick-data input when available.
- `janiform validate stylized-facts --model-path MODEL.jf --intervals 5min,1h,1D` runs the validation workflow. Add `--empirical-data-path TICKS.parquet` to validate against a different empirical sample or when the saved model has no tick-data input reference.

The stylized-facts workflow compares empirical return metrics with metrics computed from seeded
simulation paths generated from the fitted model. It infers empirical data format from the file
extension when possible, derives the simulation span from the empirical timestamps, and writes a
validation report and lineage file under the validation output directory. Add `--cache` to retain
intermediate comparison data and metric observations, or `--cache=full` to also retain generated
seeded tick paths.

## Presets

Use `janiform presets list`, `show`, and `export` to inspect bundled or user-registered presets before fitting.

## Supporting Workflows

- `janiform seasonality diagnostics`
- `janiform seasonality preprocess-timestamps`
- `janiform hourly-factors materialize-seasonality`
- `janiform hourly-factors materialize-news-intensity`
- `janiform hourly-factors build-contract`
- `janiform validate stylized-facts`
- `janiform dev regression deterministic`

These commands support preprocessing, factor construction, validation, and regression workflows around the main fit/simulate/price path.
