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
| `janiform.sufficient_statistics` | Sufficient-statistics sufficient-statistics path contracts |
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
`model.price_option_grid(...)` or `model.price_option_grid_surface(...)` instead of looping over
`model.price_option(...)`. Those multi-node results include Janiform-computed implied
volatilities, aligned pricing-status fields, and aligned failure-stage and failure-reason fields
for each node.

When the input is an observed option quote snapshot, prefer
`model.price_option_snapshot(...)`. That helper loads the observed tick and option data,
estimates the current state automatically, prices the retained option nodes in fitted-model option
grids, and returns both a tidy DataFrame view through `to_pandas()` and a strike-by-expiry matrix
through `to_surface_frame()`.

When you want a higher-level observed quote surface result, use `model.price_option_surface(...)`.
For multi-expiry retained quotes, it returns the observed snapshot as a surface result. When the
retained history shares one expiry, it can also span multiple valuation times through
`valuation_count` or `use_all_valuation_times`.

`model.simulate(...)` defaults to a seeded-tick path with a `1h` horizon interval. Pass
`interval="15min"` or another duration to change the horizon unit. Add `coarse=True` when you want
a direct coarse-interval path instead. The optional `progress_callback` receives typed
`SimulationProgressEvent` values from the Rust workflow.

`JaniformEstimator.fit(...)` and `fit_with_report(...)` also accept `progress_callback`; those
callbacks receive typed `FitProgressEvent` values from the Rust fit workflow.

`model.price_option_grid(...)`, `model.price_option_grid_surface(...)`, and
`janiform.pricing.run_vanilla_option_surface_pricing(...)` accept `progress_callback`; those
callbacks receive typed `PricingProgressEvent` values from the Rust pricing workflow.

## When To Drop Lower

- Use `janiform.calibration.staged` when you need explicit staged orchestration contracts.
- Use `janiform.simulation.coarse` or `janiform.simulation.seeded` when you need direct request and response objects for those workflows.
- Use `janiform.contracts` when you need deterministic serialization or validation helpers across workflows.
