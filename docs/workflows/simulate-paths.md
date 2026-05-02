# Simulate paths

Simulation takes a fitted `JaniformModel` and generates synthetic market paths from the behavior learned during fitting. The simulated path is not a forecast of one exact future. It is a plausible path drawn from the fitted activity, return, news, and mean-reversion structure.

This page focuses on running simulation as a workflow after a model has already been fitted.

## Simulation scope

A simulation run generates a new path from a fitted model.

Janiform supports two simulation modes from the same fitted model:

| Mode            | Best for                                                                                                              |
|-----------------|-----------------------------------------------------------------------------------------------------------------------|
| Seeded tick     | You need detailed tick-level synthetic path output. This is the default, but is slower.                               |
| Coarse interval | You need interval-level paths and do not need generated tick events. This is much faster than seeded tick simulation. |

Both modes use the same fitted model. The choice controls how much path detail is emitted, which engine is used to produce the output, and how quickly it is generated.

## Inputs

To simulate a path we need a fitted model and a horizon.

| Input                  | Required | Notes                                                                                                                                      |
|------------------------|----------|--------------------------------------------------------------------------------------------------------------------------------------------|
| Fitted model           | Yes      | The usual inputs are a `JaniformModel` in Python or the `model.jf` file produced by `janiform fit` in the CLI.                            |
| Horizon interval count | Yes      | Number of intervals to simulate.                                                                                                           |
| Interval duration      | No       | Defaults to `1h`. Exact duration labels such as `1min`, `15min`, `1h`, or `1D` also work, as does a Python `timedelta`.                  |
| Start time             | No       | Defaults to the current UTC time. Pass an aware Python `datetime` or an RFC 3339 CLI timestamp when the run must be repeatable.            |
| Seed                   | No       | Defaults to `0`. Pass it explicitly when outputs must be reproducible.                                                                     |
| Output path            | No       | Python can keep the result in memory, export from the result, or write artifacts during simulation. The CLI defaults to a timestamped run. |
| Simulation mode        | No       | Defaults to seeded tick simulation. `coarse=True` in Python or `--coarse` in the CLI switches to interval-level simulation.               |

For repeatable work, make the model path, start time, interval, interval count, seed, output path, and simulation mode explicit.

## Python

```python
from datetime import UTC, datetime

import pandas as pd

from janiform.calibration import JaniformModel, JaniformSimulationResult

model = JaniformModel.load(".janiform/runs/eurusd-fit/artifacts/model.jf")
simulation: JaniformSimulationResult = model.simulate(
    horizon_interval_count=24,
    interval="1h",
    seed=123,
    start_time_utc=datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
)

df: pd.DataFrame = simulation.to_pandas()
```

By default, `simulate(...)` runs seeded tick simulation. `coarse=True` switches to interval-level output:

```python
from datetime import UTC, datetime

import pandas as pd

from janiform.calibration import JaniformModel, JaniformSimulationResult

model = JaniformModel.load(".janiform/runs/eurusd-fit/artifacts/model.jf")
coarse_simulation: JaniformSimulationResult = model.simulate(
    horizon_interval_count=24,
    interval="1h",
    seed=123,
    start_time_utc=datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
    coarse=True,
)

df: pd.DataFrame = coarse_simulation.to_pandas()
```

`output_path` writes the same path data, simulation report, and run manifest sidecars that a durable workflow needs:

```python
from datetime import UTC, datetime
from pathlib import Path

from janiform.calibration import JaniformModel, JaniformSimulationResult

model = JaniformModel.load(".janiform/runs/eurusd-fit/artifacts/model.jf")
simulation: JaniformSimulationResult = model.simulate(
    horizon_interval_count=24,
    interval="1h",
    seed=123,
    start_time_utc=datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
    output_path=Path(".janiform/runs/eurusd-simulation/simulation.parquet"),
)
```

If the path is already in memory, export it from the result:

```python
simulation.to_parquet(".janiform/runs/eurusd-simulation/simulation.parquet")
```

`to_pandas()` returns a `pandas.DataFrame`, and `to_numpy()` returns an object array using the same output columns. Seeded tick output includes generated tick events. Coarse output includes one row per simulated interval.

> **Integrating into a larger application**: `simulate(...)` accepts a `progress_callback` for tracking progress.

## CLI

```bash
janiform simulate \
  --model-path .janiform/runs/eurusd-fit/artifacts/model.jf \
  --horizon-interval-count 24 \
  --interval 1h \
  --seed 123 \
  --start-time-utc 2026-02-02T15:00:00Z
```

By default, `janiform simulate` runs seeded tick simulation. If `--output-path` is omitted, Janiform writes to a timestamped run directory:

```text
.janiform/runs/simulate_model_<timestamp>/simulation.<format>
```

The default output format follows the fitted model's source tick-data format when it is CSV or Parquet, and otherwise uses Parquet.

The simulation report and run manifest are written next to the path data:

```text
.janiform/runs/simulate_model_<timestamp>/simulation_report.json
.janiform/runs/simulate_model_<timestamp>/run.json
```

`--output-path` places the simulation artifacts somewhere predictable:

```bash
janiform simulate \
  --model-path .janiform/runs/eurusd-fit/artifacts/model.jf \
  --horizon-interval-count 24 \
  --interval 1h \
  --seed 123 \
  --start-time-utc 2026-02-02T15:00:00Z \
  --output-path .janiform/runs/eurusd-simulation/simulation.parquet
```

`--coarse` switches to interval-level output:

```bash
janiform simulate \
  --model-path .janiform/runs/eurusd-fit/artifacts/model.jf \
  --horizon-interval-count 24 \
  --interval 1h \
  --seed 123 \
  --start-time-utc 2026-02-02T15:00:00Z \
  --coarse \
  --output-path .janiform/runs/eurusd-coarse-simulation/simulation.parquet
```

JSON mode works well when automation needs the final completion summary:

```bash
janiform --json simulate \
  --model-path .janiform/runs/eurusd-fit/artifacts/model.jf \
  --horizon-interval-count 24 \
  --interval 1h \
  --seed 123 \
  --start-time-utc 2026-02-02T15:00:00Z
```

The JSON summary includes the simulation kind, seed, simulated interval count, output path, and export record counts.

## Outputs

The path data is the primary output. Python returns a `JaniformSimulationResult` and can materialize rows in memory. Durable workflows also write the report and manifest so downstream tools can audit what was run and where the primary artifact was written.

| Output            | Python                                            | CLI                            | Notes                                                                            |
|-------------------|---------------------------------------------------|--------------------------------|----------------------------------------------------------------------------------|
| Simulation result | `JaniformSimulationResult`                        | Not returned in process        | Simulation kind, metadata, provenance, diagnostics, and simulation payload.      |
| Path data         | `to_pandas()`, `to_numpy()`, or `to_parquet(...)` | `simulation.csv` or `.parquet` | Timestamped simulated prices, optional trade side, and fitted state columns.     |
| Report            | Written when `output_path` is used                | `simulation_report.json`       | Workflow, simulation kind, primary output path, diagnostics, and export summary. |
| Run manifest      | Written when `output_path` is used                | `run.json`                     | Run id, run directory, primary output path, and report path.                     |
| JSON completion   | Not applicable                                    | Standard output                | Emitted only when `--json` is used.                                              |

Path data includes a timestamp column and a price column. Seeded tick output can also include a side column when the fitted model records an explicit trade-sign column. Both seeded and coarse output include state columns for `observed_log_price`, `anchor_value`, `pressure_state`, and `deviation_from_anchor`.

## Advanced configuration

### Output format

Simulation outputs must use `.csv` or `.parquet`.

In Python, write Parquet from an in-memory result:

```python
simulation.to_parquet(".janiform/runs/eurusd-simulation/simulation.parquet")
```

Or pass `output_path` to write the path plus sidecar artifacts during simulation:

```python
simulation: JaniformSimulationResult = model.simulate(
    horizon_interval_count=24,
    output_path=".janiform/runs/eurusd-simulation/simulation.parquet",
)
```

In the CLI, choose the format with `--output-path`:

```bash
janiform simulate \
  --model-path .janiform/runs/eurusd-fit/artifacts/model.jf \
  --horizon-interval-count 24 \
  --output-path .janiform/runs/eurusd-simulation/simulation.csv
```

If the output path has no extension, Janiform adds the model-derived default extension:

```python
simulation: JaniformSimulationResult = model.simulate(
    horizon_interval_count=24,
    output_path=".janiform/runs/eurusd-simulation/simulation",
)
```

```bash
janiform simulate \
  --model-path .janiform/runs/eurusd-fit/artifacts/model.jf \
  --horizon-interval-count 24 \
  --output-path .janiform/runs/eurusd-simulation/simulation
```

For CLI exports, use `--output-batch-size` when large Parquet exports need a specific row batch size:

```bash
janiform simulate \
  --model-path .janiform/runs/eurusd-fit/artifacts/model.jf \
  --horizon-interval-count 240 \
  --output-batch-size 4096 \
  --output-path .janiform/runs/eurusd-simulation/simulation.parquet
```

### Reproducible runs

For reproducibility, set both the seed and the start time:

```python
from datetime import UTC, datetime

simulation: JaniformSimulationResult = model.simulate(
    horizon_interval_count=24,
    interval="1h",
    seed=123,
    start_time_utc=datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
)
```

```bash
janiform simulate \
  --model-path .janiform/runs/eurusd-fit/artifacts/model.jf \
  --horizon-interval-count 24 \
  --interval 1h \
  --seed 123 \
  --start-time-utc 2026-02-02T15:00:00Z
```

Changing the seed changes the random path. Changing the start time changes the calendar, seasonality, news, and state materialization context used by the run.

### Choosing intervals

Intervals use exact UTC durations. Supported units include nanoseconds, microseconds, milliseconds, seconds, minutes, hours, and days, with common pandas-like labels such as `15min`, `1h`, and `1D`.

```python
simulation: JaniformSimulationResult = model.simulate(
    horizon_interval_count=96,
    interval="15min",
    seed=123,
    start_time_utc=datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
)
```

```bash
janiform simulate \
  --model-path .janiform/runs/eurusd-fit/artifacts/model.jf \
  --horizon-interval-count 96 \
  --interval 15min \
  --seed 123 \
  --start-time-utc 2026-02-02T15:00:00Z
```

The simulated horizon is the interval duration multiplied by `horizon_interval_count` in Python or `--horizon-interval-count` in the CLI.

### Prepared simulation requests

You will usually simulate directly from a `JaniformModel` in Python or from a saved model with `janiform simulate --model-path`. Prepared request commands are available when another tool has already built a simulation request artifact.

For seeded tick simulation:

```bash
janiform simulate seeded-ticks \
  --request-json .janiform/prepared/seeded-simulation.json \
  --calendar-registry-json .janiform/prepared/calendar-registry.json \
  --output-path .janiform/runs/prepared-seeded-simulation/simulation.parquet
```

For coarse interval simulation:

```bash
janiform simulate coarse-interval \
  --request-json .janiform/prepared/coarse-simulation.json \
  --output-path .janiform/runs/prepared-coarse-simulation/simulation.parquet
```

Prepared request commands are advanced boundary commands. They expect validated simulation inputs rather than a `model.jf` file.

## Troubleshooting

**The command says `--model-path` is missing**

CLI simulation from a fitted model needs the saved `model.jf` path. Pass the model artifact from a completed fit run. In Python, load the file with `JaniformModel.load(...)` or simulate from the in-memory model returned by fitting.

**The horizon interval count is missing**

Pass a positive interval count with `horizon_interval_count=...` in Python or `--horizon-interval-count` in the CLI. The interval count is required because it controls the simulated horizon.

**Interval parsing fails**

Exact duration labels such as `5min`, `15min`, `1h`, or `1D` are required. Intervals must be positive and exactly representable as UTC nanoseconds.

**The output path is rejected**

Simulation output paths must end in `.csv` or `.parquet`. If the path has no extension, Janiform adds the model-derived default extension.

**The same simulation settings produce a different path later**

Check whether the start time was omitted. When start time is omitted, Janiform uses the current UTC time, so calendar and seasonality context can change between runs.

**A simulation run takes a long time**

Seeded tick simulation can produce many records. `--coarse` reduces the output to intervals, and a shorter horizon or larger interval reduces it further when that matches the analysis requirement.

## See also

- [First model](../getting-started/first-model.md)
- [Fit models](fit-models.md)
- [Model validation](model-validation.md)
- [Price derivatives](price-derivatives.md)
- [Inspect and report](inspect-and-report.md)
- [Command line interface](../interfaces/cli.md)
- [Python library](../interfaces/python.md)
- [CLI reference](../reference/cli.md)
- [Python API reference](../reference/python-api.md)
