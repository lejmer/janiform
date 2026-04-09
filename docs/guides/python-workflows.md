# Python Workflows

Janiform keeps the public package root intentionally small. Import task-oriented modules directly.

## Main Modules

| Module | Use it for |
| --- | --- |
| `janiform.calibration` | High-level fitting and root-level calibration validation errors |
| `janiform.calibration.market_data` | Market-data calibration requests, preparation, and runtime |
| `janiform.calibration.presets` | Preset references, registries, and preset resolution |
| `janiform.calibration.staged` | Staged calibration contracts and runtime |
| `janiform.calibration.fit.pricing` | Fitted-model option-pricing contracts |
| `janiform.calibration.fit.state` | Current-state estimation contracts for fitted-model runtime methods |
| `janiform.simulation` | Coarse and seeded simulation workflows |
| `janiform.pricing` | Vanilla option pricing contracts and runtime |
| `janiform.risk` | Sufficient-statistics risk path contracts |
| `janiform.hourly_factors` | Hourly factor materialization and contract helpers |
| `janiform.validation` | Deterministic and leakage regression harnesses |
| `janiform.contracts` | Shared contract models and serialization helpers |

## Common Pattern

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
pricing = model.price_option(
    valuation_time_utc="2026-02-02T15:00:00Z",
    spot=1.0850,
    strike=1.0900,
    tenor="1h",
)
```

Use `JaniformEstimator.fit(...)` for both fitting workflows:

- supply only tick data to fit the underlying model
- add `option_surface_path` to fit the model to the underlying data and one option snapshot

When fitting with options, select the option snapshot through
`option_surface_valuation_time_utc` unless the surface file contains exactly one quote timestamp.
Optional flat `carry` is accepted on the same call and defaults to zero when omitted.

For multi-node fitted-model pricing, stay on the same high-level surface with
`model.price_option_grids(...)` or `model.price_option_surface(...)` instead of looping over
`model.price_option(...)`. Those multi-node results include Janiform-computed implied
volatilities, aligned pricing-status fields, and aligned failure-stage and failure-reason fields
for each node.

When the input is an observed option quote snapshot, prefer
`model.price_option_market_snapshot(...)`. That helper loads the observed tick and option data,
estimates the current state automatically, prices the retained option nodes in option grids,
and returns both a tidy DataFrame view through `to_pandas()` and a strike-by-expiry matrix through
`to_surface_frame()`.

## When To Drop Lower

- Use `janiform.calibration.staged` when you need explicit staged orchestration contracts.
- Use `janiform.simulation.coarse` or `janiform.simulation.seeded` when you need direct request and response objects for those workflows.
- Use `janiform.contracts` when you need deterministic serialization or validation helpers across workflows.
