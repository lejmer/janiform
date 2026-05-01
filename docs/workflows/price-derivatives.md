# Price derivatives

Derivative pricing takes a fitted `JaniformModel` and evaluates derivative contracts under the model-implied dynamics. The fitted model supplies the learned activity, return, news, and mean-reversion structure. The pricing request supplies the valuation context and contract terms.

This page focuses on pricing workflows after a model has already been fitted.

## Pricing scope

Janiform is designed around derivative pricing as a broader workflow, but the currently implemented product family is vanilla options.

| Product family          | Status       | Notes                                                                                                  |
|-------------------------|--------------|--------------------------------------------------------------------------------------------------------|
| Vanilla options         | Available    | Price calls and puts from a fitted model. Python also supports observed option snapshot/surface flows. |
| Forwards and futures    | Coming soon  | Reserved Python methods exist, but pricing is not implemented yet.                                     |
| Swaps                   | Coming soon  | Reserved Python methods exist, but pricing is not implemented yet.                                     |
| Exotic options          | Coming soon  | Not implemented yet.                                                                                   |
| Multi-instrument books  | Coming soon  | Reserved Python method exists, but pricing is not implemented yet.                                     |

The rest of this page gives concrete instructions only for vanilla options.

## Inputs

To price derivatives we need a fitted model, a valuation context, and a contract definition.

| Input                     | Required | Notes                                                                                                                                      |
|---------------------------|----------|--------------------------------------------------------------------------------------------------------------------------------------------|
| Fitted model              | Yes      | Use a `JaniformModel` in Python, or the `model.jf` file produced by `janiform fit` in the CLI.                                             |
| Valuation time            | Yes      | UTC timestamp at which the contract is priced.                                                                                             |
| Current underlying state  | Yes      | Single-contract pricing accepts an explicit spot. Dataset-backed option workflows derive state from tick data and compare against option quotes. |
| Contract terms            | Yes      | For vanilla options: payoff type, strike, and one expiry representation. Use an expiry timestamp, expiry date, or tenor.                   |
| Rates and carry           | No       | `risk_free_rate` and `dividend_yield` default to `0.0`. Pass explicit values when valuation depends on them.                               |
| Option dataset            | When used | Required for observed option snapshot/surface workflows and option-aware fitting. It is separate from the underlying tick dataset.          |
| Output path               | No       | Python can keep results in memory or write pricing artifacts. The CLI writes pricing artifacts by default.                                 |

For repeatable work, make the model path, valuation time, contract terms, rates, carry, option dataset path, and output path explicit.

For vanilla option pricing, start from an option-aware fit when the model should be constrained by observed option quotes. Option data is supplied separately from underlying tick data, both when fitting that model and when pricing observed option snapshots or surfaces.

## Python

Use Python when pricing results should stay in memory for analysis, application workflows, or comparison against observed option data.

### Vanilla options

For a single vanilla option, call `price_option(...)` on a fitted model:

```python
from datetime import UTC, datetime

from janiform.calibration import JaniformModel
from janiform.pricing import VanillaOptionPayoffType, VanillaOptionPricingRunResult

model = JaniformModel.load(".janiform/runs/eurusd-option-fit/artifacts/model.jf")
pricing_result: VanillaOptionPricingRunResult = model.price_option(
    valuation_time_utc=datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
    spot=1.0850,
    strike=1.0900,
    tenor="1h",
    payoff_type=VanillaOptionPayoffType.CALL,
    risk_free_rate=0.0,
    dividend_yield=0.0,
)

price: float = pricing_result.response.price
```

Use `output_path` when the Python workflow should also write durable pricing artifacts:

```python
pricing_result: VanillaOptionPricingRunResult = model.price_option(
    valuation_time_utc=datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
    spot=1.0850,
    strike=1.0900,
    tenor="1h",
    output_path=".janiform/runs/eurusd-option-price/pricing.json",
)
```

For an observed option snapshot or surface, pass the underlying tick dataset and the option dataset separately. The option dataset must identify quote time, expiry, strike, option type, and quote values.

```python
from datetime import UTC, datetime
from pathlib import Path

import pandas as pd

from janiform.calibration import JaniformModel
from janiform.calibration.fit.pricing import JaniformObservedOptionSurfacePricingResult

model = JaniformModel.load(".janiform/runs/eurusd-option-fit/artifacts/model.jf")
surface_result: JaniformObservedOptionSurfacePricingResult = model.price_option_surface(
    tick_data=Path("data") / "Synthetic_EURUSD_Data_1M_ticks.parquet",
    option_surface=(
        Path(".artifacts")
        / "examples"
        / "option_surfaces"
        / "synthetic_eurusd_option_snapshot.parquet"
    ),
    valuation_time_utc=datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
)

df: pd.DataFrame = surface_result.to_pandas()
```

`price_option_surface(...)` prices each selected observed option node and returns a tidy table through `to_pandas()`. Use `to_surface_df(...)` when a pivoted strike-by-expiry or strike-by-valuation view is more useful.

> **Integrating into a larger application**: option grid and surface pricing methods accept `progress_callback` where the workload can fan out across many nodes.

## CLI

Use the CLI when a pricing run should produce durable artifacts for review, scripts, schedulers, or automation.

```bash
janiform price \
  --model-path .janiform/runs/eurusd-option-fit/artifacts/model.jf \
  --valuation-time-utc 2026-02-02T15:00:00Z \
  --spot 1.0850 \
  --strike 1.0900 \
  --tenor 1h \
  --payoff-type call
```

If `--output-path` is omitted, Janiform writes pricing artifacts to a timestamped run directory:

```text
.janiform/<model-run-id>_price_<timestamp>/pricing.json
.janiform/<model-run-id>_price_<timestamp>/pricing_report.json
.janiform/<model-run-id>_price_<timestamp>/run.json
```

Use `--output-path` when the pricing artifact should live somewhere predictable:

```bash
janiform price \
  --model-path .janiform/runs/eurusd-option-fit/artifacts/model.jf \
  --valuation-time-utc 2026-02-02T15:00:00Z \
  --spot 1.0850 \
  --strike 1.0900 \
  --tenor 1h \
  --payoff-type call \
  --output-path .janiform/runs/eurusd-option-price/pricing.json
```

Use JSON mode when automation needs the final completion summary:

```bash
janiform --json price \
  --model-path .janiform/runs/eurusd-option-fit/artifacts/model.jf \
  --valuation-time-utc 2026-02-02T15:00:00Z \
  --spot 1.0850 \
  --strike 1.0900 \
  --tenor 1h \
  --payoff-type call
```

The JSON summary includes the run id, model path, artifact paths, pricing response, and run diagnostics.

The CLI currently exposes single vanilla-option pricing. Use Python for observed option snapshot and surface workflows.

## Outputs

The pricing response is the primary output. Python returns typed result objects and can materialize observed option pricing rows in memory. Durable workflows write the pricing payload, report, and run manifest.

| Output          | Python                                                                 | CLI                   | Notes                                                                 |
|-----------------|------------------------------------------------------------------------|-----------------------|-----------------------------------------------------------------------|
| Pricing result  | `VanillaOptionPricingRunResult` or option surface result object        | Not returned in process | Price, status, diagnostics, and optional calibration hooks.           |
| Observed rows   | `to_pandas()` from observed option snapshot/surface results            | Not available         | Tidy observed quote rows with model prices and implied volatilities.  |
| Surface view    | `to_surface_df(...)` from observed option snapshot/surface results     | Not available         | Pivoted option result view for review and plotting.                   |
| Pricing payload | Written when `output_path` is used                                     | `pricing.json`        | Primary pricing result artifact.                                      |
| Report          | Written when `output_path` is used                                     | `pricing_report.json` | Output path and pricing diagnostics.                                  |
| Run manifest    | Written when `output_path` is used                                     | `run.json`            | Run id, run directory, primary output path, and report path.          |
| JSON completion | Not applicable                                                         | Standard output       | Emitted only when `--json` is used.                                   |

## Derivative families

### Vanilla options

Vanilla option pricing is available for calls and puts. The contract must define exactly one expiry representation:

| Expiry input            | Use when                                                        |
|-------------------------|-----------------------------------------------------------------|
| `expiry_timestamp_utc`  | The exact UTC expiry timestamp is known.                        |
| `expiry_date`           | The expiry is date-based and should preserve valuation time of day. |
| `tenor`                 | The expiry should be expressed relative to valuation time.       |

For observed option datasets, Janiform can price either one quote snapshot or a valuation-time surface:

| Python method              | Use when                                                                 |
|----------------------------|--------------------------------------------------------------------------|
| `price_option(...)`        | You have one vanilla option contract and explicit valuation inputs.      |
| `price_option_snapshot(...)` | You have one observed option quote snapshot to price and compare.       |
| `price_option_surface(...)`  | You have observed option quotes across valuation times or snapshots.    |
| `price_option_grid(...)`   | You already have a typed batch of vanilla option grid contracts.         |
| `price_option_grid_surface(...)` | You already have a typed vanilla option surface contract.          |

### Forwards and futures

Coming soon. The `JaniformModel.price_future(...)` method is reserved but not implemented yet.

### Swaps

Coming soon. The `JaniformModel.price_swap(...)` method is reserved but not implemented yet.

### Exotic options

Coming soon.

### Multi-instrument books

Coming soon. The `JaniformModel.price_instruments(...)` method is reserved but not implemented yet.

## Advanced configuration

### Observed option columns

If Janiform cannot infer the observed option dataset layout, pass an option surface column mapping in Python. The dataset must identify quote timestamp, expiry, strike, option type, and quote values.

```python
from pathlib import Path

from janiform.calibration import JaniformModel
from janiform.calibration.fit.pricing import JaniformObservedOptionSurfacePricingResult
from janiform.calibration.market_data import CalibrationOptionSurfaceColumnMapping

model = JaniformModel.load(".janiform/runs/eurusd-option-fit/artifacts/model.jf")
option_columns = CalibrationOptionSurfaceColumnMapping(
    quote_timestamp_column="quote_time_utc",
    expiry_timestamp_column="expiry_utc",
    strike_column="strike",
    option_type_column="option_type",
    bid_column="bid",
    ask_column="ask",
)

surface_result: JaniformObservedOptionSurfacePricingResult = model.price_option_surface(
    tick_data=Path("data") / "ticks.parquet",
    option_surface=Path("data") / "options.parquet",
    option_surface_column_mapping=option_columns,
)
```

### Diagnostics

Single-option pricing can include additional diagnostics:

```python
from datetime import UTC, datetime

from janiform.pricing import VanillaOptionPricingRunResult

pricing_result: VanillaOptionPricingRunResult = model.price_option(
    valuation_time_utc=datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
    spot=1.0850,
    strike=1.0900,
    tenor="1h",
    include_transform_diagnostics=True,
    include_integration_quality_diagnostics=True,
)
```

The CLI exposes the same diagnostics flags:

```bash
janiform price \
  --model-path .janiform/runs/eurusd-option-fit/artifacts/model.jf \
  --valuation-time-utc 2026-02-02T15:00:00Z \
  --spot 1.0850 \
  --strike 1.0900 \
  --tenor 1h \
  --include-transform-diagnostics \
  --include-integration-quality-diagnostics
```

## Troubleshooting

**The command says `--model-path` is missing**

CLI pricing needs the saved `model.jf` path. Pass the model artifact from a completed fit run. In Python, load the file with `JaniformModel.load(...)` or price from the in-memory model returned by fitting.

**No expiry was supplied**

Vanilla option pricing needs one expiry representation. Pass `expiry_timestamp_utc`, `expiry_date`, or `tenor` in Python, or one of `--expiry-timestamp-utc`, `--expiry-date`, or `--tenor` in the CLI.

**Columns are not recognized**

For observed option workflows, pass explicit option surface columns. Check quote timestamp, expiry, strike, option type, bid, ask, mid, and price columns before changing pricing settings.

**The option surface has multiple quote times**

Pass `valuation_time_utc` when the workflow should price one valuation snapshot. Use `use_all_valuation_times=True` in Python when the workflow should price every quote time supported by the dataset.

**Pricing fails for some option nodes**

Inspect the pricing status, failure stage, failure reason, transform diagnostics, and integration quality diagnostics. Failures can be local to a strike or maturity, so review the full result table instead of only the aggregate run status.

**A derivative family is not available**

Only vanilla option pricing is currently implemented. Forwards, futures, swaps, exotic options, and multi-instrument books are reserved for future releases.

## See also

- [First model](../getting-started/first-model.md)
- [Fit models](fit-models.md)
- [Simulate paths](simulate-paths.md)
- [Model validation](model-validation.md)
- [Inspect and report](inspect-and-report.md)
- [Command line interface](../interfaces/cli.md)
- [Python interface](../interfaces/python.md)
- [CLI reference](../reference/cli.md)
- [Python API reference](../reference/python-api.md)
