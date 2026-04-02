# Quickstart

This is the shortest happy path after bootstrapping.

Use [concepts.md](concepts.md) first if terms like market profile, seeded ticks, or coarse intervals are unfamiliar.

## CLI

```bash
janiform --help
janiform fit --help
janiform simulate --help
janiform price --help
```

The top-level CLI is organized around:

- `fit` for calibration
- `simulate` for coarse-interval or seeded-tick simulation
- `price` for vanilla option pricing
- `presets`, `seasonality`, `hourly-factors`, and `stylized-facts` for supporting workflows

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

## Example Script

Start with this example script:

```bash
python -m examples.synthetic_eurusd
```

`examples.synthetic_eurusd` is a good first example because it fits the sample EUR/USD dataset and plots both coarse and seeded simulation output.
