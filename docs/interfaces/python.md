# Python library

## Package overview

The Python library lets you fit models, simulate paths, price derivatives, and inspect results directly from Python. It works well in notebooks, scripts, services, batch applications, and larger Python applications.

Most workflows start in one of these modules:

- `janiform.calibration` for fitting models and loading saved ones
- `janiform.simulation` for simulation workflows
- `janiform.pricing` for pricing workflows

## Where to start

| Module                             | Purpose                                                                                                               |
|------------------------------------|-----------------------------------------------------------------------------------------------------------------------|
| `janiform.calibration`             | Fit models, read fit reports, load saved models, and start simulation from a fitted model.                            |
| `janiform.calibration.market_data` | Pass explicit market-data settings such as column mappings, normalization choices, workflow scope, and preprocessing. |
| `janiform.calibration.presets`     | Work with bundled presets and with your own registered presets.                                                       |
| `janiform.simulation`              | Run simulation workflows directly when you need more control over simulation inputs or progress reporting.            |
| `janiform.pricing`                 | Work with pricing requests, pricing results, progress reporting, and schedule policies.                               |
| `janiform.validation`              | Run validation workflows and work with validation reports.                                                            |

For most Python workflows, `janiform.calibration` is the starting point.

## Core objects

Most Python workflows revolve around a small set of objects:

| Type                       | Role                                                                                                          |
|----------------------------|---------------------------------------------------------------------------------------------------------------|
| `JaniformEstimator`        | Fits models from market data.                                                                                 |
| `JaniformFitResult`        | Returned by `fit_with_report(...)`. Holds the model when fitting succeeds and always includes the fit report. |
| `JaniformFitReport`        | Summarizes how the fit ran and what it produced.                                                              |
| `JaniformModel`            | The fitted model you use for simulation, pricing, saving, and loading.                                        |
| `JaniformSimulationResult` | Returned by `JaniformModel.simulate(...)`. Lets you inspect or export the simulated path.                     |

The usual pattern is:

```python
from pathlib import Path

from janiform.calibration import JaniformEstimator, JaniformModel

estimator = JaniformEstimator(market_profile="eurusd_spot")
model: JaniformModel = estimator.fit(
    tick_data_path=Path("data") / "Synthetic_EURUSD_Data_1M_ticks.parquet",
)
```

`fit(...)` returns the model directly. `fit_with_report(...)` also returns the fit report:

```python
from pathlib import Path

from janiform.calibration import JaniformEstimator, JaniformFitReport, JaniformFitResult, JaniformModel

result: JaniformFitResult = JaniformEstimator(market_profile="eurusd_spot").fit_with_report(
    tick_data_path=Path("data") / "Synthetic_EURUSD_Data_1M_ticks.parquet",
)

model: JaniformModel = result.require_model()
report: JaniformFitReport = result.report
```

After fitting, keep working from the saved or in-memory model:

```python
from datetime import UTC, datetime

from janiform.calibration import JaniformModel, JaniformSimulationResult
from janiform.pricing import VanillaOptionPricingRunResult

model = JaniformModel.load(".janiform/runs/eurusd-fit/artifacts/model.jf")

simulation: JaniformSimulationResult = model.simulate(
    horizon_interval_count=24,
    interval="1h",
)

pricing: VanillaOptionPricingRunResult = model.price_option(
    valuation_time_utc=datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
    spot=1.0850,
    strike=1.0900,
    tenor="1h",
)
```

The saved `model.jf` file works with both Python and the CLI, so you can fit in one interface and continue in the other.

## When you need explicit inputs

Many workflows can infer enough from the data and the arguments you pass to `JaniformEstimator(...)`. When they cannot, pass the input details explicitly.

Common examples are:

- `market_profile`
- `session`
- `known_news`
- `exchange_code`
- `session_timezone`
- `tick_data_column_mapping`
- `option_surface_column_mapping`
- `workflow_scope`

When you need more control, these modules are the usual next step:

- `janiform.calibration.market_data` for market-data formats, column mappings, normalization choices, and workflow scope
- `janiform.calibration.presets` for preset references and preset registry resolution
- `janiform.pricing` for pricing requests and interval schedule policies
- `janiform.simulation` for direct simulation requests and progress events

In many cases the defaults are enough, and these explicit inputs become useful only when the workflow needs tighter control.

## Configuration

The Python library does not use a separate Janiform configuration file like the CLI does. You configure each run directly in Python.

In practice:

- `JaniformEstimator(...)` arguments hold fit defaults
- market-data and preset objects override inference when needed
- method arguments such as `output_dir`, `output_path`, `seed`, and `start_time_utc` control each run

This keeps Janiform settings close to the Python code that is using them.

## Outputs

The Python library works well both for in-memory analysis and for writing files you want to keep.

### In memory

This is the default Python experience:

- `fit(...)` returns a `JaniformModel`
- `fit_with_report(...)` returns a `JaniformFitResult`
- `simulate(...)` returns a `JaniformSimulationResult`
- `price_option(...)` returns a pricing result object

This mode keeps results available immediately in Python.

### Writing files

When you pass an output path, the library can also write the same artifact families the CLI uses:

- fit outputs through `output_dir`
- simulation outputs through `output_path`
- pricing outputs through `output_path`

That includes shared artifacts such as:

```text
artifacts/model.jf
artifacts/report.json
simulation_report.json
pricing.json
pricing_report.json
run.json
```

You can also save a fitted model directly:

```python
model.save(".janiform/runs/eurusd-fit/artifacts/model.jf")
```

That file can later be loaded with `JaniformModel.load(...)` in Python or passed to CLI commands that accept `--model-path`.

This mode keeps results on disk and works well when you move between Python and the CLI.

## Automation

For application integration, the Python library exposes progress callbacks instead of CLI event streams.

Common hooks:

- `JaniformEstimator.fit(..., progress_callback=...)`
- `JaniformEstimator.fit_with_report(..., progress_callback=...)`
- `JaniformModel.simulate(..., progress_callback=...)`
- option-grid and option-surface pricing methods with `progress_callback=...`

These callbacks are useful for notebook widgets, application logging, and other same-process progress handling.

The CLI fits workflows organized around standalone processes, run directories, `events.jsonl`, `latest-state.json`, and final JSON command output.

## See also

- [Command line interface](cli.md)
- [Configuration](configuration.md)
- [Output and automation](output-and-automation.md)
- [First model](../getting-started/first-model.md)
- [Fit models](../workflows/fit-models.md)
- [Simulate paths](../workflows/simulate-paths.md)
- [Price derivatives](../workflows/price-derivatives.md)
- [Python API reference](../reference/python-api.md)
