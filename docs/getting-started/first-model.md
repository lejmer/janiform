# First Model

This walkthrough uses the checked-in synthetic EUR/USD dataset and the high-level Python workflow.

Use [concepts.md](concepts.md) if you want the terms around anchors, reference prices, or seeded
ticks defined before running the workflow.

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

When you already have recent observed continuation trades, pass them through
`continuation_data`. Janiform estimates the live Hawkes and mean-reversion runtime state before
continuing the simulation.

```python
from datetime import UTC, datetime

import pandas

from janiform.calibration import CurrentStateEstimationRequest

simulation = model.simulate(
    seed=17,
    start_time_utc=datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
    horizon_interval_count=4,
    interval="1h",
    continuation_data=CurrentStateEstimationRequest(
        observed_tick_data=pandas.DataFrame(
            {
                "timestamp_utc": [
                    datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
                    datetime(2026, 2, 2, 14, 45, tzinfo=UTC),
                ],
                "trade_price": [1.0845, 1.0850],
                "trade_sign": ["buy", "sell"],
            }
        ),
    ),
)
```

## Price

`model.price_option(...)` derives the pricing state from the fitted model, the valuation timestamp,
and the supplied spot. When you also provide observed continuation trades, Janiform estimates the
live Hawkes and mean-reversion boundary state from those observations before pricing. Without
observed data, Janiform falls back to the stationary Hawkes state.

```python
from datetime import UTC, datetime

result = model.price_option(
    valuation_time_utc=datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
    spot=1.0850,
    strike=1.0900,
    maturity_interval=1.0,
)
```

To supply observed data explicitly:

```python
from datetime import UTC, datetime

import pandas

from janiform.calibration import CurrentStateEstimationRequest

result = model.price_option(
    valuation_time_utc=datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
    spot=1.0850,
    strike=1.0900,
    maturity_interval=1.0,
    observed_data=CurrentStateEstimationRequest(
        observed_tick_data=pandas.DataFrame(
            {
                "timestamp_utc": [
                    datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
                    datetime(2026, 2, 2, 14, 45, tzinfo=UTC),
                ],
                "trade_price": [1.0845, 1.0850],
                "trade_sign": ["buy", "sell"],
            }
        ),
    ),
)
```

If you want the derived live state directly, call `model.estimate_current_state(...)` with the same
request contract.

To price several nodes from the same fitted-model snapshot without writing a loop, use
`model.price_options(...)` with `JaniformVanillaOptionBatch`, or use
`model.price_option_surface(...)` with `JaniformVanillaOptionSurface` when your nodes form a
strike-by-expiry grid. Both workflows return Janiform prices together with Janiform-computed
implied-volatility outputs.

## Related Example

[`examples/high_level/synthetic_eurusd_plot.py`](../../examples/high_level/synthetic_eurusd_plot.py)
is a good next step when you want the same fit and simulation flow with plotting included.
Use [`examples/README.md`](../../examples/README.md) for the grouped example index.
