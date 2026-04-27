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

- `janiform simulate` is the default simulation entrypoint.
- `janiform simulate seeded-ticks` runs the priced fine-path workflow.
- `janiform simulate coarse-interval` runs the direct coarse-interval path.

Choose seeded ticks when you need event replay or fine-path output. Choose coarse interval when you want direct interval paths without replaying fine-scale events.

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

## Presets

Use `janiform presets list`, `show`, and `export` to inspect bundled or user-registered presets before fitting.

## Supporting Workflows

- `janiform seasonality diagnostics`
- `janiform seasonality preprocess-timestamps`
- `janiform hourly-factors materialize-seasonality`
- `janiform hourly-factors materialize-news-intensity`
- `janiform hourly-factors build-contract`
- `janiform stylized-facts horizon-comparison`
- `janiform dev regression deterministic`

These commands support preprocessing, factor construction, validation, and regression workflows around the main fit/simulate/price path.
