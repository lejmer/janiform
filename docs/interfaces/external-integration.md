# External integration

This page covers the main ways to integrate Janiform with other tools and systems: CLI process output, machine-readable event streams, progress callbacks in Python, and the files Janiform writes during and after a run.

## Integration surfaces

Janiform exposes external integration through four main surfaces:

| Surface | What it provides |
|---------|------------------|
| CLI stdout | Final terminal output or one final JSON object with `--json`. |
| CLI stderr event stream | Line-delimited JSON events with `--events jsonl`. |
| Written files | Saved models, reports, manifests, event logs, latest-state snapshots, checkpoints, simulations, and pricing payloads. |
| Python callbacks | In-process progress updates during fitting, simulation, and pricing. |

These surfaces can be combined. A long-running fit can emit JSONL events to stderr, write a run directory to disk, and later be inspected again from the CLI or from Python.

## CLI process output

The CLI has two machine-oriented output paths:

1. One final JSON object on stdout with `--json`
2. A streaming JSONL event log on stderr with `--events jsonl`

Example:

```bash
janiform --json --events jsonl fit \
  data/Synthetic_EURUSD_Data_1M_ticks.parquet \
  --run-id eurusd-fit
```

In that form:

- stdout contains one final JSON object
- stderr contains one JSON event per line while the command is running
- the run directory is still written to disk for later inspection

This split keeps the final result easy to parse without giving up live progress.

## Event streams

`--events jsonl` writes a machine-readable event stream to stderr. These events cover lifecycle changes, artifacts, warnings, errors, progress updates, and metric updates.

For long-running fits, the event stream is the most direct live signal from the CLI process itself.

Example:

```bash
janiform --json --events jsonl fit \
  data/Synthetic_EURUSD_Data_1M_ticks.parquet \
  --run-id eurusd-fit \
  2> fit-events.jsonl \
  1> fit-result.json
```

That pattern produces:

- `fit-result.json` with the final completion payload
- `fit-events.jsonl` with the live event stream

`--trace-events` adds trace-level records to that stream:

```bash
janiform --json --events jsonl --trace-events fit \
  data/Synthetic_EURUSD_Data_1M_ticks.parquet \
  --run-id eurusd-fit
```

## Written files

Janiform writes different file sets depending on the workflow.

### Fit runs

A disk-backed fit writes a run directory such as:

```text
.janiform/runs/eurusd-fit/
```

The main files are:

```text
.janiform/runs/eurusd-fit/artifacts/model.jf
.janiform/runs/eurusd-fit/artifacts/report.json
.janiform/runs/eurusd-fit/events.jsonl
.janiform/runs/eurusd-fit/latest-state.json
.janiform/runs/eurusd-fit/checkpoints/
```

These files serve different integration roles:

- `artifacts/model.jf` is the saved model used by later simulation, pricing, inspection, and validation commands
- `artifacts/report.json` is the durable fit summary
- `events.jsonl` is the append-only run event log
- `latest-state.json` is the current compact run-state snapshot
- `checkpoints/` contains resumable fit checkpoints

### Simulation outputs

Simulation writes the path data itself plus sidecar files beside it:

```text
.janiform/runs/model_<timestamp>/simulation.parquet
.janiform/runs/model_<timestamp>/simulation_report.json
.janiform/runs/model_<timestamp>/run.json
```

`simulation_report.json` summarizes the simulation kind, output path, and diagnostics. `run.json` records the run id and output locations.

### Pricing outputs

Pricing writes:

```text
.janiform/runs/<pricing-run-id>/pricing.json
.janiform/runs/<pricing-run-id>/pricing_report.json
.janiform/runs/<pricing-run-id>/run.json
```

`pricing.json` is the primary pricing payload. `pricing_report.json` carries diagnostics and output metadata.

## Latest-state snapshots

`latest-state.json` is the compact file to read when you want the current state of a fit run without replaying the whole event log yourself.

It is also what `janiform inspect run` reads first before falling back to the event log when needed.

Example:

```bash
janiform --json inspect run .janiform/runs/eurusd-fit
```

That works both after a run completes and while a disk-backed fit is still progressing.

## Python progress callbacks

The Python library exposes progress callbacks for in-process integration. Callback payloads are Rust-owned event mappings, so the same event schema is available to Python callbacks and machine-event streams.

### Fit callbacks

`JaniformEstimator.fit(...)` and `JaniformEstimator.fit_with_report(...)` accept `progress_callback`.

Example:

```python
from collections.abc import Mapping
from pathlib import Path
from typing import Any

from janiform.calibration import JaniformEstimator


def on_fit_progress(event: Mapping[str, Any]) -> None:
    print(event["event_class"], event["payload"])


estimator = JaniformEstimator(market_profile="eurusd_spot")
model = estimator.fit(
    tick_data_path=Path("data") / "Synthetic_EURUSD_Data_1M_ticks.parquet",
    progress_callback=on_fit_progress,
)
```

Fit progress event mappings expose:

- stage information when available
- phase information when available
- bounded progress measures when available

### Simulation callbacks

`JaniformModel.simulate(...)` also accepts `progress_callback`.

Example:

```python
from collections.abc import Mapping
from typing import Any

from janiform.calibration import JaniformModel


def on_simulation_progress(event: Mapping[str, Any]) -> None:
    print(event["phase"], event["completed_units"], event["total_units"])


model = JaniformModel.load(".janiform/runs/eurusd-fit/artifacts/model.jf")
simulation = model.simulate(
    horizon_interval_count=24,
    interval="1h",
    progress_callback=on_simulation_progress,
)
```

Simulation progress event mappings report the phase plus bounded progress counts.

### Pricing callbacks

Pricing callbacks are available on the Python pricing workflows that can fan out across many nodes, such as option grids and surfaces.

Example:

```python
from collections.abc import Mapping
from datetime import UTC, datetime
from typing import Any

from janiform.calibration import JaniformModel


def on_pricing_progress(event: Mapping[str, Any]) -> None:
    print(event["phase"], event["completed_units"], event["total_units"])


model = JaniformModel.load(".janiform/runs/eurusd-option-fit/artifacts/model.jf")
valuation_time_utc = datetime(2026, 2, 2, 15, 0, tzinfo=UTC)
surface = ...
pricing_result = model.price_option_grid_surface(
    valuation_time_utc=valuation_time_utc,
    surface=surface,
    progress_callback=on_pricing_progress,
)
```

Pricing progress event mappings expose:

- the current pricing phase
- bounded progress counts
- optional fields such as maturity or strike position

## Saving and reusing models

Python and CLI workflows share the same saved model file.

Example:

```python
model.save(".janiform/runs/eurusd-fit/artifacts/model.jf")
```

That file can then be:

- loaded again in Python with `JaniformModel.load(...)`
- passed to CLI commands through `--model-path`
- archived with the rest of a run directory

## Common patterns

### Capture a final JSON result and a live event stream

```bash
janiform --json --events jsonl fit \
  data/Synthetic_EURUSD_Data_1M_ticks.parquet \
  --run-id eurusd-fit \
  1> fit-result.json \
  2> fit-events.jsonl
```

### Watch run state through saved files

```bash
janiform --json inspect run .janiform/runs/eurusd-fit
```

### Fit in Python and continue in the CLI

```python
model.save(".janiform/runs/eurusd-fit/artifacts/model.jf")
```

```bash
janiform simulate \
  --model-path .janiform/runs/eurusd-fit/artifacts/model.jf \
  --horizon-interval-count 24
```

### Keep simulation and pricing sidecars next to the primary output

```python
simulation = model.simulate(
    horizon_interval_count=24,
    output_path=".janiform/runs/eurusd-simulation/simulation.parquet",
)

pricing = model.price_option(
    valuation_time_utc=valuation_time_utc,
    spot=1.0850,
    strike=1.0900,
    tenor="1h",
    output_path=".janiform/runs/eurusd-price/pricing.json",
)
```

## See also

- [Command line interface](cli.md)
- [Python library](python.md)
- [Configuration](configuration.md)
- [Fit models](../workflows/fit-models.md)
- [Simulate paths](../workflows/simulate-paths.md)
- [Price derivatives](../workflows/price-derivatives.md)
