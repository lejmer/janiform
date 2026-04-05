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

Start with tick data. If you also want the fit to include options, add an option surface on the
same call:

- `tick_data_path=...` fits the model to the underlying data
- `tick_data_path=...` plus `option_surface_path=...` fits the model to the underlying data and the
  selected option snapshot

## Fit With One Option Snapshot

Use the same `fit(...)` call when you want to include option data.

```python
from datetime import UTC, datetime
from pathlib import Path

from janiform.calibration import JaniformEstimator
from janiform.calibration.market_data import CalibrationFlatCarryInput

tick_data_path = Path("data") / "Synthetic_EURUSD_Data_1M_ticks.parquet"
option_surface_path = Path("path/to/your_option_surface.parquet")

model = JaniformEstimator(market_profile="eurusd_spot").fit(
    tick_data_path=tick_data_path,
    option_surface_path=option_surface_path,
    option_surface_valuation_time_utc=datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
    carry=CalibrationFlatCarryInput(discount_rate=0.0, carry_rate=0.0),
)
```

When you add an option surface:

- each fit run uses one option quote timestamp
- pass `option_surface_valuation_time_utc` unless the file contains exactly one quote timestamp
- include bid and ask quotes together with prices

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

import pandas as pd

from janiform.calibration import CurrentStateEstimationRequest

simulation = model.simulate(
    seed=17,
    start_time_utc=datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
    horizon_interval_count=4,
    interval="1h",
    continuation_data=CurrentStateEstimationRequest(
        observed_tick_data=pd.DataFrame(
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
    expiry_timestamp_utc=datetime(2026, 2, 2, 16, 0, tzinfo=UTC),
)
```

To supply observed data explicitly:

```python
from datetime import UTC, date, datetime

import pandas as pd

from janiform.calibration import CurrentStateEstimationRequest

result = model.price_option(
    valuation_time_utc=datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
    spot=1.0850,
    strike=1.0900,
    expiry_date=date(2026, 2, 3),
    observed_data=CurrentStateEstimationRequest(
        observed_tick_data=pd.DataFrame(
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
`model.price_option_groups(...)` with `JaniformGroupedVanillaOptionBatch` when you want to combine
multiple payoff or carry groups in one run.

```python
from datetime import UTC, datetime

from janiform.calibration import (
    JaniformGroupedVanillaOptionBatch,
    JaniformGroupedVanillaOptionGroup,
)
from janiform.pricing import VanillaOptionPayoffType

group_result = model.price_option_groups(
    valuation_time_utc=datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
    batch=JaniformGroupedVanillaOptionBatch(
        spot=1.0850,
        groups=(
            JaniformGroupedVanillaOptionGroup(
                payoff_type=VanillaOptionPayoffType.CALL,
                risk_free_rate=0.0,
                dividend_yield=0.0,
                strikes=(1.0800, 1.0850, 1.0900),
                tenors=("1h", "2h", "4h"),
            ),
        ),
    ),
)
```

When your nodes already form a strike-by-expiry grid, use
`model.price_option_surface(...)` with `JaniformVanillaOptionSurface`.

```python
from datetime import UTC, datetime

from janiform.calibration import JaniformVanillaOptionSurface
from janiform.pricing import VanillaOptionPayoffType

surface_result = model.price_option_surface(
    valuation_time_utc=datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
    surface=JaniformVanillaOptionSurface(
        spot=1.0850,
        strikes=(1.0800, 1.0850, 1.0900),
        tenors=("1h", "2h", "4h"),
        payoff_type=VanillaOptionPayoffType.CALL,
        risk_free_rate=0.0,
        dividend_yield=0.0,
    ),
)
```

Both workflows return Janiform prices together with Janiform-computed implied volatility outputs,
per-node pricing statuses, and failure diagnostics for any nodes that do not price successfully.

## Related Example

[`examples/high_level/synthetic_eurusd_plot.py`](../../examples/high_level/synthetic_eurusd_plot.py)
is a good next step when you want the same fit and simulation flow with plotting included.
Use [`examples/README.md`](../../examples/README.md) for the grouped example index.
