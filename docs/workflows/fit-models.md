# Fit models

Fitting takes observed market data and produces a `JaniformModel` by calibrating its internals until it represents the data well. That model is what everything else builds on: simulation, derivative pricing, and model validation all assume you already have a successful fit.

This page focuses on how to run fits in a more realistic workflow, beyond the first example.

## Fit scope

A fit run has two jobs:

1. Turn raw market data into a validated calibration input.
2. Run the calibration and produce a fitted model.

Tick data is always required. Coarse historical data and option surface data are optional and change the calibration target:

| Input set                   | Best for                                                                                        |
|-----------------------------|-------------------------------------------------------------------------------------------------|
| Tick data only              | You need a model for simulation, model validation, or model-implied pricing.                    |
| Tick data + coarse data     | You want tick-level activity fit and slower return, variance, mean-reversion, gap, or tail fit. |
| Tick data + option data     | You need a model to price options.                                                              |
| Tick + coarse + option data | You want slower physical evidence and option-surface evidence in one calibration.               |

Coarse and option data do not replace tick data. Coarse data supplies slower physical evidence when available, and option data adds a forward-looking constraint on top of the underlying history.

## Inputs

To fit a model we need the market observations and the context required to interpret it.

| Input                  | Required | Notes                                                                                                                                                                            |
|------------------------|----------|----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Tick data              | Yes      | Must include timestamps and enough price information for the selected tick price policy. A direct price column or bid/ask midpoint can be used.                                  |
| Tick size              | No       | Supply it when the instrument has a known price increment. Explicit values win, market-profile tick sizes are fallbacks, and retained tick data is used for inference otherwise. |
| Market/session context | Yes      | Supplied by a market profile, explicit exchange/session options, or inferred bundled presets when the input is recognizable.                                                     |
| Known-news calendar    | No       | Include it when scheduled news should be part of the activity calibration.                                                                                                       |
| Coarse data            | No       | Optional lower-frequency OHLCV bars or close-only history for slower physical targets. Timestamp-only bars are interpreted as fixed-interval open-time observations by default.  |
| Option surface         | No       | Needed for option-aware calibration. The surface must identify quote time, expiry, strike, option type, and quote values.                                                        |

For repeatable work, make the context explicit. Market profile inference is convenient for quick runs, but production code should not rely on it.

## Python

```python
from pathlib import Path

from janiform.calibration import JaniformEstimator
from janiform.model import JaniformModel

estimator = JaniformEstimator(
    market_profile="eurusd_spot",
    tick_size=0.00001,
)
model: JaniformModel = estimator.fit(
    tick_data_path=Path("data") / "Synthetic_EURUSD_Data_1M_ticks.parquet",
)
```

In Janiform, coarse data means lower-frequency price observations such as one-minute, hourly, daily, or close-only bars. Pass `coarse_data_path` when you also have OHLCV bars or close-only history. Janiform keeps tick data as the event/activity source and routes coarse evidence to slower return, variance, mean-reversion, gap, and tail targets.
Coarse timestamp columns are interpreted as bar-open times by default. For sources stamped at bar close, use `CalibrationCoarseDataTimestampSemantics.BAR_CLOSE_TIME` in Python or pass `--coarse-timestamp-semantics bar-close-time` in the CLI.

```python
model: JaniformModel = estimator.fit(
    tick_data_path=Path("data") / "eurusd_ticks.parquet",
    coarse_data_path=Path("data") / "eurusd_1h_bars.parquet",
)
```

`fit(...)` returns the model directly. If you also need diagnostics and metadata, use `fit_with_report(...)`:

```python
from janiform.calibration import JaniformFitReport, JaniformFitResult

result: JaniformFitResult = estimator.fit_with_report(
    tick_data_path=Path("data") / "Synthetic_EURUSD_Data_1M_ticks.parquet",
)

model: JaniformModel = result.model
report: JaniformFitReport = result.report
```

To keep the fitted model as a file for later Python or CLI work:

```python
model.save()
```

With no path argument, `model.save()` writes `.janiform/runs/<run-id>/artifacts/model.jf`. Pass a path to `model.save(...)` when automation needs a specific location.

For option-aware calibration, pass an option surface as well. The surface can contain multiple strikes, expiries, and option types. If the file contains more than one quote timestamp, pass `option_surface_valuation_time_utc` so the calibration process can know which valuation snapshot to target.

```python
from datetime import UTC, datetime
from pathlib import Path

from janiform.calibration import JaniformEstimator
from janiform.model import JaniformModel

estimator = JaniformEstimator(market_profile="eurusd_spot")
model: JaniformModel = estimator.fit(
    tick_data_path=Path("data") / "Synthetic_EURUSD_Data_1M_ticks.parquet",
    option_surface_path=(
        Path(".artifacts")
        / "examples"
        / "option_surfaces"
        / "synthetic_eurusd_option_snapshot.parquet"
    ),
    option_surface_valuation_time_utc=datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
)
```

> **Integrating into a larger application**: Both `fit(...)` and `fit_with_report(...)` accept a `progress_callback` for tracking progress.

## CLI

```bash
janiform fit \
  --tick-data-path data/Synthetic_EURUSD_Data_1M_ticks.parquet \
  --tick-size 0.00001 \
  --run-id eurusd-fit
```

`--run-id` gives the run a stable name. The Janiform CLI uses it when it builds the default output path, so later commands can refer to a predictable directory instead of a generated timestamped run name.

With the default output root, the run directory is:

```text
.janiform/runs/<run-id>
```

The fitted model and fit report are written to:

```text
.janiform/runs/<run-id>/artifacts/model.jf
.janiform/runs/<run-id>/artifacts/report.json
```

Use `--no-persist` for one-off CLI dry runs that should not keep a run directory or fitted model artifact.

The tick-data path can also be supplied positionally for the main fit command and `fit inspect`:

```bash
janiform fit data/Synthetic_EURUSD_Data_1M_ticks.parquet --run-id eurusd-fit
```

With optional coarse historical data:

```bash
janiform fit \
  --tick-data-path data/eurusd_ticks.parquet \
  --coarse-data-path data/eurusd_1h_bars.parquet \
  --run-id eurusd-fit
```

For option-aware calibration:

```bash
janiform fit \
  --tick-data-path data/Synthetic_EURUSD_Data_1M_ticks.parquet \
  --option-surface-path .artifacts/examples/option_surfaces/synthetic_eurusd_option_snapshot.parquet \
  --option-surface-valuation-time-utc 2026-02-02T15:00:00Z \
  --run-id eurusd-option-fit
```

`--output-dir` places the run directory somewhere other than `.janiform/runs/<run-id>`.

## CLI preflight checks

Before executing an expensive run from the CLI, it can be useful to first `inspect` and `plan`.

`inspect` resolves market data, column mappings, market context, and workflow scope without running calibration:

```bash
janiform fit inspect \
  --tick-data-path data/Synthetic_EURUSD_Data_1M_ticks.parquet
```

This is useful when you want to check if configuration will be inferred correctly by Janiform.

`plan` validates the same inputs and shows the output paths and execution policy:

```bash
janiform fit plan \
  --tick-data-path data/Synthetic_EURUSD_Data_1M_ticks.parquet \
  --run-id eurusd-fit
```

`plan` provides a dry run before spending time on calibration.

## Outputs

The model is the primary output. Everything else exists to explain, audit, resume, or automate the run.

| Output           | Python                                                | CLI                                              |
|------------------|-------------------------------------------------------|--------------------------------------------------|
| Fitted model     | `JaniformModel` from `fit(...)`                       | `artifacts/model.jf`                             |
| Fit result       | `JaniformFitResult` from `fit_with_report(...)`       | `artifacts/report.json`                          |
| Fit report       | `JaniformFitReport` from `JaniformFitResult.report`   | `artifacts/model.jf` and `artifacts/report.json` |
| Progress events  | `progress_callback`                                   | `events.jsonl` in the run directory              |
| Latest run state | Not written by the in-memory path                     | `latest-state.json` in the run directory         |
| Checkpoints      | Available only through disk-backed execution          | `checkpoints/` in the run directory              |

Downstream CLI commands usually need the saved model path:

```bash
janiform inspect model .janiform/runs/eurusd-fit/artifacts/model.jf
janiform report fit .janiform/runs/eurusd-fit
```

## Advanced configuration

### Explicit columns

If Janiform cannot infer the input layout, pass the column names:

```bash
janiform fit \
  --tick-data-path data/ticks.parquet \
  --tick-timestamp-column timestamp_utc \
  --tick-price-column price \
  --tick-trade-sign-column trade_sign \
  --tick-price-input-policy explicit-price-column \
  --timestamp-timezone UTC \
  --run-id explicit-column-fit
```

Bid/ask columns work with `--tick-price-input-policy bid-ask-midpoint` when the fit should derive the tick price from quotes.

### Explicit market context

If profile inference is not enough, provide the market context directly:

```bash
janiform fit \
  --tick-data-path data/ticks.parquet \
  --exchange-code XNYS \
  --session-timezone America/New_York \
  --session-definition-path config/xnys-session.json \
  --run-id explicit-session-fit
```

The same idea applies in Python through `JaniformEstimator(...)` arguments such as `market_profile`, `session`, `known_news`, `exchange_code`, and `session_timezone`.

### Mean-reversion expert controls

Janiform chooses the mean-reversion calibration grid from the data by default. Here, "grid" means the time spacing used by the mean-reversion calibration stage. Use the explicit interval only when you deliberately want to audit or reproduce a specific time spacing:

```python
from janiform.calibration import CalibrationMeanReversionConfiguration, JaniformEstimator

estimator = JaniformEstimator(
    market_profile="eurusd_spot",
    mean_reversion=CalibrationMeanReversionConfiguration(
        calibration_grid_interval_hours=24.0,
    ),
)
```

The CLI equivalents are available on `fit`, `fit plan`, and `fit staged plan`:

```bash
janiform fit staged plan \
  --tick-data-path data/eurusd_ticks.parquet \
  --mean-reversion-grid-interval-hours 24 \
  --run-id eurusd-fit
```

Advanced users can override the predictive evidence level with `predictive_evidence_alpha` in Python or `--mean-reversion-predictive-evidence-alpha` in the CLI. This changes the documented statistical gate for selecting a model with `kappa > 0`, so keep the default unless the run policy requires a different level.

### Prepared staged runs

Staged execution is useful when another process needs to prepare the calibration input before the expensive run starts:

```bash
janiform fit staged plan \
  --tick-data-path data/Synthetic_EURUSD_Data_1M_ticks.parquet \
  --execution-input-output-path .janiform/prepared/eurusd-fit.json \
  --run-id eurusd-fit

janiform fit staged run \
  --execution-input-path .janiform/prepared/eurusd-fit.json
```

The staged path is an advanced workflow. For normal terminal usage, start with `janiform fit`, `janiform fit inspect`, and `janiform fit plan`.

### Resuming disk execution

Disk-backed fits write checkpoints. When a compatible checkpoint exists, resume by reference:

```bash
janiform fit \
  --tick-data-path data/Synthetic_EURUSD_Data_1M_ticks.parquet \
  --run-id eurusd-fit \
  --resume-checkpoint-reference checkpoint://eurusd-fit/last-success
```

Checkpoint references use `checkpoint://<run-id>/<checkpoint-token>`. The run id is the original fit run.
Disk execution uses the run directory as the persistence root. Checkpoint files live below `checkpoints/`, while staged calibration artifacts live below `artifacts/calibration/`.
Resume runs stage a copy of the committed artifact set, update it during execution, and replace the committed artifact set only after the resumed run succeeds.

| Checkpoint token    | Meaning                                                                   |
|---------------------|---------------------------------------------------------------------------|
| `last-success`      | Resume from the latest successfully written checkpoint for the run.       |
| `seasonality`       | Resume from the checkpoint written after seasonality calibration.         |
| `hawkes`            | Resume from the checkpoint written after event-activity calibration.      |
| `return_components` | Resume from the checkpoint written after return-distribution calibration. |
| `long_variance`     | Resume from the checkpoint written after long-variance calibration.       |
| `mean_reversion`    | Resume from the checkpoint written after mean-reversion calibration.      |
| `joint_refinement`  | Resume from the checkpoint written after final dynamics refinement.       |
| `option_refinement` | Resume from the checkpoint written after option-surface calibration.      |
| `finalization`      | Resume from the checkpoint written after final model assembly.            |

A stage token is valid only after that stage has completed and written its checkpoint. For most resumes, use `last-success` unless you need to force a specific earlier checkpoint.

When adding an option surface to a completed tick-only fit, resume from `joint_refinement` and start at `option-refinement`. The earlier tick-data stages and selected dynamics frontier are reused, while option refinement fits the option-surface pricing block for the supplied option snapshot:

```bash
janiform fit data/databento-converted/ES_FUT-TBBO/continuous/es_continuous.parquet \
  --option-surface-path data/databento-converted/ES_OPT-BBO-1min/janiform_option_surface.parquet \
  --option-surface-valuation-time-utc 2026-04-01T23:59:00Z \
  --market-profile es_futures_globex \
  --run-id es_continuous_20260507T195059Z \
  --resume-checkpoint-reference checkpoint://es_continuous_20260507T195059Z/joint_refinement \
  --resume-from-stage option-refinement
```

Joint-refinement resume state is finer grained than the top-level stage checkpoint. Dynamics restart outcomes are reusable across tick-only and option-aware runs when the run id and stage seed match. Option search, selection, and audit tier pricing artifacts are scoped to a fingerprint of the option-pricing context so changing the option surface recomputes only the option-dependent work.

The same resume flow is available from Python. Use disk output preferences, pass `output_dir` as the directory that contains the run folders, and point `disk_checkpoint_directory` at the run directory. For the CLI default layout, `output_dir` is `.janiform/runs` and `disk_checkpoint_directory` is `.janiform/runs/<run-id>`:

```python
from datetime import UTC, datetime
from pathlib import Path

from janiform.calibration import JaniformEstimator
from janiform.calibration.market_data import (
    CalibrationMarketDataOutputPreferences,
)
from janiform.calibration.staged import (
    CalibrationExecutionMode,
    CalibrationResumeSettings,
    CalibrationResumeSource,
    CalibrationStage,
)

run_id = "es_continuous_20260507T195059Z"
output_root = Path(".janiform") / "runs"
run_directory = output_root / run_id

estimator = JaniformEstimator(
    market_profile="es_futures_globex",
    output_preferences=CalibrationMarketDataOutputPreferences(
        execution_mode=CalibrationExecutionMode.DISK,
        disk_checkpoint_directory=str(run_directory),
    ),
)

result = estimator.fit_with_report(
    run_id=run_id,
    tick_data_path=Path("data/databento-converted/ES_FUT-TBBO/continuous/es_continuous.parquet"),
    option_surface_path=Path(
        "data/databento-converted/ES_OPT-BBO-1min/janiform_option_surface.parquet"
    ),
    option_surface_valuation_time_utc=datetime(2026, 4, 1, 23, 59, tzinfo=UTC),
    output_dir=output_root,
    resume_settings=CalibrationResumeSettings(
        source=CalibrationResumeSource.PERSISTED_CHECKPOINT_REFERENCE,
        checkpoint_reference=f"checkpoint://{run_id}/mean_reversion",
        resume_from_stage=CalibrationStage.JOINT_REFINEMENT,
    ),
)
```

You can use the fit report and event log to confirm which stage completed before resuming.

## Troubleshooting

**Missing tick data**

Every fit needs tick data. Pass `--tick-data-path` or use the main-command shorthand `janiform fit PATH`.

**Profile or session resolution fails**

Janiform does not have enough market context. Provide a market profile in Python, or pass explicit CLI session options such as `--exchange-code`, `--session-timezone`, and `--session-definition-path`.

**Columns are not recognized**

Pass explicit column mappings. Check timestamp, price, bid, ask, trade-sign, option quote timestamp, expiry, strike, option type, and quote-value column names before adding more calibration settings.

**Timestamp validation fails**

Naive timestamps need an explicit timezone convention. For tick data, pass `--timestamp-timezone`. For known-news calendars, pass `--known-news-timezone`.

**Option fit fails because the snapshot is ambiguous**

Pass `option_surface_valuation_time_utc` in Python or `--option-surface-valuation-time-utc` in the CLI so the calibration targets one quote timestamp. Option rows that cannot provide a usable bid/ask quote are filtered and counted in the option surface source summary.

**A fit takes a long time**

Calibration is the expensive part of Janiform and is expected to take a long time, sometimes many hours. `fit plan` helps before a long run starts, and disk execution keeps the work inspectable and resumable.

**A downstream command cannot find the model**

Check the run id and model path. The default CLI model path is `.janiform/runs/<run-id>/artifacts/model.jf`.

## See also

- [First model](../getting-started/first-model.md)
- [Concepts](../getting-started/concepts.md)
- [Model validation](model-validation.md)
- [Inspect and report](inspect-and-report.md)
- [Presets and market data](presets-and-market-data.md)
- [Calibration diagnostics](calibration-diagnostics.md)
- [Command line interface](../interfaces/cli.md)
- [Python library](../interfaces/python.md)
- [CLI reference](../reference/cli.md)
- [Python API reference](../reference/python-api.md)
