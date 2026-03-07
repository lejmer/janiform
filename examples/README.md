# Workflow Examples

These examples exercise the current public Python workflow contracts directly.
They import only public Janiform modules and are designed to be small enough to
read end to end.

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
- keeps `nu_h_known` and `nu_h_surprise` explicit in `VanillaOptionPricingIntervalFactors`
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

## Calibration

Run:

```bash
PYTHONPATH=python python3 -m examples.calibration_workflow
```

This example shows both calibration execution-mode paths:
- default Python importable API behavior by constructing `StagedCalibrationRequest(...)` without
  an explicit `execution_mode`, which yields `memory`
- explicit `execution_mode=disk` override with a checkpoint directory

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
