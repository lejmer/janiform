# Quickstart

This is the shortest happy path after bootstrapping.

Use [concepts.md](concepts.md) first if terms like market profile, seeded ticks, or coarse intervals are unfamiliar.

## CLI

```bash
janiform --help
janiform fit --help
janiform simulate --help
janiform price --help
janiform inspect --help
janiform report --help
janiform validate --help
janiform presets --help
```

The top-level CLI is organized around:

- `fit` for calibration
- `simulate` for coarse-interval or seeded-tick simulation
- `price` for vanilla option pricing
- `inspect` for saved run and fitted model inspection
- `report` for fit report views and rendered HTML reports
- `validate` for model validation workflows
- `presets`, `seasonality`, and `hourly-factors` for supporting workflows

## Python

```python
from janiform.calibration import JaniformEstimator

model = JaniformEstimator(market_profile="eurusd_spot").fit(
    tick_data_path="data/Synthetic_EURUSD_Data_1M_ticks.parquet",
)
simulation = model.simulate(
    seed=17,
    start_time_utc="2023-03-28T19:00:00Z",
    horizon_interval_count=24,
)
print(simulation.to_pandas().head())
```

`JaniformEstimator.fit(...)` works for both tick-only fitting and option calibration. With only
tick data, it fits the underlying model (suitable for simulating price paths). When you also pass
an option surface, it fits the model to the underlying data and the selected option snapshot
(suitable for options pricing).

Use `JaniformEstimator.fit_with_report(...)` when you want the fitted model and the fit report from
one Python call without changing the return type of `fit(...)`.

When you fit with options:

- one fit run targets one option quote timestamp
- pass `option_surface_valuation_time_utc` unless the file contains exactly one unique quote time
- the option surface must contain bid and ask data together with prices
- `carry` is an optional flat discount or carry input and defaults to zero when omitted

## Example Script

Start with this example script:

```bash
python -m examples.introductory.first_model
```

`examples.introductory.first_model` is the shortest importable example because it fits the sample EUR/USD dataset and returns a pandas-ready simulation frame without plotting dependencies.

When you want the plotting walkthrough, use:

```bash
python -m examples.high_level.synthetic_eurusd_plot
```
