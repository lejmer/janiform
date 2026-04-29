# CLI Workflows

Use the high-level commands first. Drop to explicit subcommands only when you need direct control over a contract boundary.

## Fit

- `janiform fit` is the default high-level calibration path.
- `janiform fit plan` validates inputs and builds the derived fit plan without running staged calibration.
- `janiform fit staged` runs from an explicit staged execution-input contract.

`janiform fit` chooses the fit from the supplied inputs:

- tick data only fits the underlying model
- tick data plus an option surface fits the model to the underlying data and the selected option snapshot

Use `fit plan` when you want the derived request and persistence policy reviewed before execution.
Use `fit staged` when orchestration artifacts are already prepared and you need the explicit
boundary.

## Simulate

- `janiform simulate --model-path MODEL.jf --horizon-interval-count COUNT` is the
  simulation-from-model-artifact entrypoint.
- `janiform simulate --model-path MODEL.jf --coarse --horizon-interval-count COUNT` runs the
  model-artifact coarse-interval path.
- `janiform simulate seeded-ticks` runs from an explicit seeded-tick request JSON.
- `janiform simulate coarse-interval` runs from an explicit coarse-interval request JSON.

The model-artifact command defaults to seeded ticks because that is the fine-path simulation most
callers want after fitting. `--interval` controls the horizon unit and defaults to `1h`; it does not
select the path kind. Add `--coarse` when you want direct interval paths without replaying
fine-scale events.

`--seed` defaults to `0`, `--start-time-utc` defaults to the current UTC timestamp, and
`--output-path` defaults under `.janiform/runs/<run-id>/`. Use `--output-batch-size` to control
artifact export batching when writing large simulation outputs.

## Price

`janiform price` prices a vanilla option from a persisted model and derives the pricing state from the model plus the valuation inputs.

## Inspect

- `janiform inspect run RUN_DIR` shows the current or completed state of a persisted run.
- `janiform inspect model MODEL_PATH` shows model identity, provenance, fitted components,
  capabilities, and fitted parameter summaries.

Use `inspect run` while a fit is running or after it completes when you need a status-oriented view
of the run directory. The command reads `latest-state.json` first and falls back to replaying
`events.jsonl` only when the snapshot is missing or stale. The replay path tolerates a trailing
in-progress JSONL row so a concurrent event write does not make inspection fail.

Use `inspect model` when you already have a `model.jf` artifact and need to understand what was fit
and what workflows the model supports. Add `--parameters` for fuller parameter summaries that stay
compact enough for terminal output.

## Report

- `janiform report fit RUN_OR_REPORT_PATH` summarizes a fit report from a run directory or from a
  `report.json` artifact.
- `janiform report fit RUN_OR_REPORT_PATH --html` writes a structured HTML report.
- `janiform --json report fit RUN_OR_REPORT_PATH` writes a machine-readable report view.

Use `report fit` after a fit run when the natural question is "what did this calibration produce?"
rather than "what is the run doing right now?" The default terminal view stays concise. JSON and
HTML outputs include presentation-only parameter fits extracted from the sibling model artifact
when available. Those parameter fits are not written back into `report.json`.

## Validate

- `janiform validate stylized-facts plan --model-path MODEL.jf --intervals 5m,1h,1d` shows the validation computation plan, using the model artifact's fitted tick-data source when available.
- `janiform validate stylized-facts --model-path MODEL.jf --intervals 5m,1h,1d` runs the validation workflow. Add `--empirical-data-path TICKS.parquet` to validate against a different empirical sample or when the model artifact has no tick source reference.

The stylized-facts workflow compares empirical return metrics with metrics computed from seeded
simulation paths generated from the fitted model. It infers empirical data format from the file
extension when possible, derives the simulation span from the empirical timestamps, and writes a
report bundle and lineage file under the validation output directory. Add `--cache` to retain
derived comparison inputs and metric observations, or `--cache=full` to also retain generated
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
