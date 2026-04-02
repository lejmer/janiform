# First Model

This walkthrough uses the checked-in synthetic EUR/USD dataset and the high-level Python workflow.

Use [concepts.md](concepts.md) if you want the terms around anchors, reference prices, or seeded ticks defined before running the workflow.

## Fit

```python
from pathlib import Path

from janiform.calibration import JaniformEstimator

data_path = Path("data") / "Synthetic_EURUSD_Data_1M_ticks.parquet"
estimator = JaniformEstimator(market_profile="eurusd_spot")
model = estimator.fit(tick_data_path=data_path)
```

## Simulate

```python
from datetime import UTC, datetime

simulation = model.simulate(
    seed=17,
    start_time_utc=datetime(2023, 3, 28, 19, 0, tzinfo=UTC),
    horizon_interval_count=96,
    interval="15min",
)
frame = simulation.to_pandas()
```

## Price

`model.price_option(...)` derives the pricing state from the fitted model, the valuation timestamp, and the supplied spot.

```python
from datetime import UTC, datetime

result = model.price_option(
    valuation_time_utc=datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
    spot=1.0850,
    strike=1.0900,
    maturity_interval=1.0,
)
```

## Related Example

[`examples/high_level/synthetic_eurusd_plot.py`](../../examples/high_level/synthetic_eurusd_plot.py) is a good next step when you want the same fit and simulation flow with plotting included. Use [`examples/README.md`](../../examples/README.md) for the grouped example index.
