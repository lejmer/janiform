# Workflow Examples

These examples exercise the current public Python workflow contracts directly.
They import only public Janiform modules and are designed to be small enough to
read end to end.

Internal fixture builders and documentation setup helpers live under
`examples/support/` and are not part of the example list below.

## Synthetic EUR/USD

Run:

```bash
PYTHONPATH=python python3 -m examples.synthetic_eurusd
```

This a very short-and-to-the-point end-to-end example. It:
- loads the checked-in synthetic EUR/USD parquet fixture
- fits a `JaniformModel` with `JaniformEstimator`
- runs both coarse-interval and seeded-tick simulations from the fitted model
- opens a two-panel plot so you can inspect the simulated paths visually

This example uses the bundled dataset at
`data/Synthetic_EURUSD_Data_1M_ticks.parquet`, so you can run it without
preparing any separate input files first.

## Simulation

Run:

```bash
PYTHONPATH=python python3 -m examples.simulation_workflow
```

This example:
- builds a public `CalendarRegistry`
- materializes explicit hourly `s_h` and `nu_h` inputs through the simulation-facing loader
- constructs a typed `SeededTickSimulationRequest`
- runs `run_seeded_tick_event_simulation(...)`

Expected summary shape:

```json
{
  "request_summary": {
    "seed": 17,
    "horizon_interval_count": 3,
    "hourly_factor_interval_count": 3
  },
  "response_summary": {
    "schema_version": "simulation.seeded_tick_event.path.v1",
    "event_count": 5,
    "first_event": {
      "event_index": 0,
      "side": "sell",
      "state_snapshot": null,
      "timestamp_utc_nanos": 1770044373742285686
    },
    "last_event": {
      "event_index": 4,
      "side": "sell",
      "state_snapshot": null,
      "timestamp_utc_nanos": 1770051218918372793
    }
  }
}
```

## Pricing

Run:

```bash
PYTHONPATH=python python3 -m examples.pricing_workflow
```

This example:
- builds a typed `VanillaOptionPricingRequest`
- keeps `nu_h_known` and `nu_h_surprise` explicit in a native-backed `HourlyFactorContract`
- runs `run_vanilla_option_pricing(...)`

Expected summary shape:

```json
{
  "request_summary": {
    "interval_delta": 1.0,
    "payoff_type": "call",
    "nu_h_known": 0.01,
    "nu_h_surprise": 0.02
  },
  "run_result_summary": {
    "schema_version": "pricing.vanilla_option.run_result.v1",
    "price": 3.872311,
    "forward_price": 102.015067,
    "run_diagnostics_included": true,
    "calibration_hooks_included": true
  }
}
```

## Market-Data Calibration

Run:

```bash
PYTHONPATH=python python3 -m examples.market_data_calibration_workflow
```

This example shows the recommended market-data-first preparation path:
- tick-data `csv` with explicit known-news inputs
- tick-data `parquet` with the same user-facing contract
- optional option-surface `parquet` input layered onto the same workflow
- preparation diagnostics and provenance summaries that explain what Janiform loaded,
  how it normalized the files, and which staged calibration scope it derived

Expected summary shape:

```json
{
  "tick_csv_primary": {
    "preparation_schema_version": "calibration.market_data.preparation.v1",
    "workflow_scope": "full_without_option_surface",
    "tick_data_format": "csv",
    "selected_stages": ["seasonality", "hawkes", "return_components"],
    "known_news_event_count": 2,
    "option_surface_record_count": 0,
    "prepared_execution_mode": "memory",
    "prepared_stage_count": 3
  },
  "tick_parquet_primary": {
    "preparation_schema_version": "calibration.market_data.preparation.v1",
    "workflow_scope": "full_without_option_surface",
    "tick_data_format": "parquet",
    "selected_stages": ["seasonality", "hawkes", "return_components"],
    "known_news_event_count": 2,
    "option_surface_record_count": 0,
    "prepared_execution_mode": "memory",
    "prepared_stage_count": 3
  },
  "tick_csv_with_option_surface": {
    "preparation_schema_version": "calibration.market_data.preparation.v1",
    "workflow_scope": "full_with_option_surface",
    "tick_data_format": "csv",
    "selected_stages": ["seasonality", "hawkes", "return_components"],
    "known_news_event_count": 2,
    "option_surface_record_count": 1,
    "prepared_execution_mode": "memory",
    "prepared_stage_count": 3
  }
}
```

## Advanced Staged Calibration

Run:

```bash
PYTHONPATH=python python3 -m examples.calibration_workflow
```

This example is the expert-oriented staged path and shows both execution-mode overrides:
- default Python importable API behavior by constructing `StagedCalibrationRequest(...)` without
  an explicit `execution_mode`, which yields `memory`
- explicit `execution_mode=disk` override with a checkpoint directory
- return-components candidate generation from raw close/open boundary observations plus an explicit
  material-gap tolerance instead of caller-supplied detected gap candidates

Expected summary shape:

```json
{
  "memory_mode": {
    "request_execution_mode": "memory",
    "response_execution_mode": "memory",
    "status": "success",
    "stage_count": 3,
    "checkpoint_count": 3
  },
  "disk_mode": {
    "request_execution_mode": "disk",
    "response_execution_mode": "disk",
    "status": "success",
    "stage_count": 3,
    "checkpoint_count": 3,
    "checkpoint_file_count": 10
  }
}
```
