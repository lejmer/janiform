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
