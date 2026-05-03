# Presets and market data

Presets describe the market context that gives raw observations meaning. Market data supplies the observations themselves. A fit needs both: tick data for the measured price process, and a session or market profile so Janiform knows how to interpret trading time, closed periods, timestamp defaults, known news, and useful column aliases.

This page focuses on choosing presets, registering your own presets, and making market-data inputs explicit when inference is not enough.

## Preset scope

Janiform has three preset categories:

| Category         | What it contains                                                                             | Typical role                                                          |
|------------------|----------------------------------------------------------------------------------------------|-----------------------------------------------------------------------|
| `market_profile` | A high-level profile that points to a session, known-news policy, timezone, and input hints. | Start here when a market profile context should set the fit defaults. |
| `session`        | Exchange/session windows, timezone, and recurring closed windows.                            | Override or define the trading calendar independently of a profile.   |
| `known_news`     | Scheduled event calendar used by activity calibration.                                       | Choose the scheduled-event calendar independently of a profile.       |

Market profiles are the most convenient entrypoint. They can select a session and recommend or apply a known-news calendar. Session and known-news presets remain available as lower-level overrides when you need precise control.

Bundled presets include common profiles such as `eurusd_spot`, `btcusd_spot`, `us_equity_cash`, and `es_futures_globex`. The exact set available in the installed package can be inspected with `PresetRegistry` in Python or with `janiform presets list`.

## Inputs

Market data needed for calibration is split by purpose:

| Input          | Required | Supported file formats  | Notes                                                                                                                           |
|----------------|----------|-------------------------|---------------------------------------------------------------------------------------------------------------------------------|
| Tick data      | Yes      | `csv`, `parquet`        | Must include timestamps and either a price column or bid/ask columns. Presets can supply column aliases and timestamp defaults. |
| Option surface | No       | `csv`, `parquet`        | Required for option-aware fitting. Must identify quote time, expiry, strike, option type, and quote values.                     |
| Known news     | No       | preset or explicit JSON | Usually selected through a known-news preset. Staged lower-level contracts can also use calendar files.                         |

For repeatable work, provide the market profile or session explicitly, specify column mappings when names are not obvious, and keep user presets beside the project configuration that depends on them.

## Python

### List and inspect presets

`PresetRegistry` exposes the same registry that the CLI uses:

```python
from janiform.calibration.presets import PresetCategory, PresetIndexEntry, PresetRegistry

registry = PresetRegistry()
profile_entries: tuple[PresetIndexEntry, ...] = registry.list_presets(category=PresetCategory.MARKET_PROFILE)
profile_names: tuple[str, ...] = tuple(entry.preset_name for entry in profile_entries)
```

Resolve a specific artifact when code needs the actual configuration:

```python
from typing import Optional

from janiform.calibration.presets import PresetRegistryScope

resolved_scope, market_profile = registry.resolve_market_profile(
    preset_name="eurusd_spot",
    registry_scope=PresetRegistryScope.BUILT_IN,
)

session_name: Optional[str] = market_profile.configuration.session_reference.preset_name
```

To build a self-contained preset snapshot, call `resolve_preset_snapshot(...)`. It resolves the requested preset and includes referenced dependencies, such as the session and known-news artifacts used by a market profile. In Python, the method returns a `ResolvedPresetSnapshot` object.

```python
from typing import Any

from janiform.calibration.presets import ResolvedPresetSnapshot

snapshot: ResolvedPresetSnapshot = registry.resolve_preset_snapshot(
    category=PresetCategory.MARKET_PROFILE,
    preset_name="eurusd_spot",
)

json: str = snapshot.to_json()
payload: dict[str, Any] = snapshot.to_payload()
```

### Presets in a fit

For most fits, pass the market profile name to `JaniformEstimator`:

```python
from pathlib import Path

from janiform.calibration import JaniformEstimator
from janiform.model import JaniformModel

estimator = JaniformEstimator(market_profile="eurusd_spot")
model: JaniformModel = estimator.fit(tick_data_path=Path("data") / "Synthetic_EURUSD_Data_1M_ticks.parquet")
```

Override session or known-news selection when the market profile is close but not exact:

```python
estimator = JaniformEstimator(
    market_profile="eurusd_spot",
    session="fx_spot_24_5",
    known_news="none",
)
```

If you need a user-registered preset, create a registry, register the artifact, then pass that registry to the estimator:

```python
from pathlib import Path

from janiform.calibration import JaniformEstimator
from janiform.model import JaniformModel
from janiform.calibration.presets import (
    PresetArtifact,
    PresetRegistry,
    deserialize_preset_artifact,
)

registry = PresetRegistry()
preset_json: str = (Path("config") / "desk_fx_profile.json").read_text(encoding="utf-8")
artifact: PresetArtifact = deserialize_preset_artifact(preset_json)
registry.register_user_preset(artifact)

estimator = JaniformEstimator(
    market_profile="desk_fx_profile",
    registry=registry,
)
model: JaniformModel = estimator.fit(tick_data_path=Path("data") / "ticks.parquet")
```

When a user preset has the same category and name as a bundled preset, normal resolution chooses the user-registered preset. Pass an explicit `PresetRegistryScope` in a reference or registry lookup when code must select one scope.

### Make market-data columns explicit

Janiform can infer common column names, especially when a market profile provides alias hints. For durable code, pass explicit mappings when your input schema is under your control:

```python
from pathlib import Path

from janiform.calibration import JaniformEstimator
from janiform.model import JaniformModel
from janiform.calibration.market_data import (
    CalibrationMarketDataNormalizationPolicy,
    CalibrationTickDataColumnMapping,
    CalibrationTickPriceInputPolicy,
)

tick_columns = CalibrationTickDataColumnMapping(
    timestamp_column="timestamp_utc",
    price_column="price",
    trade_sign_column="trade_sign",
)
normalization = CalibrationMarketDataNormalizationPolicy(
    timestamp_timezone="UTC",
    sort_records_by_timestamp=True,
    tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
)

estimator = JaniformEstimator(
    market_profile="eurusd_spot",
    tick_data_column_mapping=tick_columns,
    normalization_policy=normalization,
)
model: JaniformModel = estimator.fit(tick_data_path=Path("data") / "ticks.csv")
```

Bid/ask inputs work by changing the column mapping and tick price policy:

```python
tick_columns = CalibrationTickDataColumnMapping(
    timestamp_column="timestamp_utc",
    bid_column="bid",
    ask_column="ask",
    trade_sign_column="trade_sign",
)
normalization = CalibrationMarketDataNormalizationPolicy(
    timestamp_timezone="UTC",
    sort_records_by_timestamp=True,
    tick_price_input_policy=CalibrationTickPriceInputPolicy.BID_ASK_MIDPOINT,
)
```

For option-aware calibration, add the option surface path and option surface columns:

```python
from datetime import UTC, datetime
from pathlib import Path

from janiform.calibration import JaniformEstimator
from janiform.model import JaniformModel
from janiform.calibration.market_data import (
    CalibrationMarketDataNormalizationPolicy,
    CalibrationOptionQuoteInputPolicy,
    CalibrationOptionSurfaceColumnMapping,
    CalibrationTickDataColumnMapping,
    CalibrationTickPriceInputPolicy,
)

tick_columns = CalibrationTickDataColumnMapping(
    timestamp_column="timestamp_utc",
    price_column="price",
    trade_sign_column="trade_sign",
)
option_columns = CalibrationOptionSurfaceColumnMapping(
    quote_timestamp_column="quote_time_utc",
    expiry_timestamp_column="expiry_utc",
    strike_column="strike",
    option_type_column="option_type",
    bid_column="bid",
    ask_column="ask",
)
normalization = CalibrationMarketDataNormalizationPolicy(
    timestamp_timezone="UTC",
    sort_records_by_timestamp=True,
    tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
    option_quote_input_policy=CalibrationOptionQuoteInputPolicy.BID_ASK_MIDPOINT,
)

estimator = JaniformEstimator(
    market_profile="eurusd_spot",
    tick_data_column_mapping=tick_columns,
    option_surface_column_mapping=option_columns,
    normalization_policy=normalization,
)
model: JaniformModel = estimator.fit(
    tick_data_path=Path("data") / "ticks.parquet",
    option_surface_path=Path("data") / "options.parquet",
    option_surface_valuation_time_utc=datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
)
```

### Explicit references

String arguments are concise. Reference objects are useful when code needs to choose a registry scope or apply a small override:

```python
from janiform.calibration.presets import (
    MarketProfileReference,
    PresetReferenceUsageMode,
    PresetRegistryScope,
)

market_profile = MarketProfileReference(
    usage_mode=PresetReferenceUsageMode.NAMED,
    registry_scope=PresetRegistryScope.BUILT_IN,
    preset_name="eurusd_spot",
)

estimator = JaniformEstimator(market_profile=market_profile)
```

## CLI

List available presets:

```bash
janiform presets list
janiform presets list --category market-profile
janiform presets list --category session
janiform presets list --category known-news
```

Show one preset artifact:

```bash
janiform presets show market-profile eurusd_spot
janiform presets show session fx_spot_24_5
janiform presets show known-news eu_us_macro_fx_core
```

Create a resolved preset snapshot when you need the market profile and its nested dependencies in one payload:

```bash
janiform presets export market-profile eurusd_spot --output json > profile-export.json
```

The CLI writes that payload to stdout, so redirect it when you want a file.

Register user presets for a command with one or more `--user-preset` flags:

```bash
janiform presets list \
  --user-preset config/desk_fx_session.json \
  --user-preset config/desk_fx_profile.json
```

The user registry exists for that command invocation. The same `--user-preset` flags need to appear on `fit`, `fit inspect`, `fit plan`, and `presets` commands that should resolve those presets.

## Fit with presets

The fit command accepts the same market-profile, session, and known-news concepts:

```bash
janiform fit \
  --tick-data-path data/Synthetic_EURUSD_Data_1M_ticks.parquet \
  --market eurusd_spot \
  --run-id eurusd-fit
```

Override the lower-level components directly:

```bash
janiform fit \
  --tick-data-path data/ticks.parquet \
  --market eurusd_spot \
  --session-preset fx_spot_24_5 \
  --known-news none \
  --run-id explicit-context-fit
```

Explicit JSON works better when a preset name is not the right boundary for the workflow:

```bash
janiform fit \
  --tick-data-path data/ticks.parquet \
  --session-json config/session.json \
  --known-news-json config/known-news.json \
  --run-id explicit-json-fit
```

For explicit tick columns:

```bash
janiform fit \
  --tick-data-path data/ticks.csv \
  --tick-timestamp-column timestamp_utc \
  --tick-price-column price \
  --tick-trade-sign-column trade_sign \
  --tick-price-input-policy explicit_price_column \
  --timestamp-timezone UTC \
  --market eurusd_spot \
  --run-id explicit-columns-fit
```

For bid/ask tick data:

```bash
janiform fit \
  --tick-data-path data/ticks.csv \
  --tick-timestamp-column timestamp_utc \
  --tick-bid-column bid \
  --tick-ask-column ask \
  --tick-trade-sign-column trade_sign \
  --tick-price-input-policy bid_ask_midpoint \
  --timestamp-timezone UTC \
  --market eurusd_spot \
  --run-id bid-ask-fit
```

For option-aware calibration:

```bash
janiform fit \
  --tick-data-path data/ticks.parquet \
  --option-surface-path data/options.parquet \
  --option-surface-valuation-time-utc 2026-02-02T15:00:00Z \
  --option-quote-timestamp-column quote_time_utc \
  --option-expiry-column expiry_utc \
  --option-strike-column strike \
  --option-type-column option_type \
  --option-bid-column bid \
  --option-ask-column ask \
  --option-quote-input-policy bid_ask_midpoint \
  --market eurusd_spot \
  --run-id option-aware-fit
```

## Troubleshooting

**The wrong preset is selected**

A user-registered preset can shadow a bundled preset with the same category and name. In Python, pass `PresetRegistryScope.BUILT_IN` or `PresetRegistryScope.USER_REGISTERED` in a reference when the scope must be explicit. In the CLI, use `--market-scope`, `--session-scope`, `--known-news-scope`, or `--scope` for `presets` commands.

**Column inference picked the wrong price**

Pass explicit column mappings and a matching price policy. A direct price column requires `explicit_price_column`. Bid/ask columns require `bid_ask_midpoint`.

**Option data is ignored**

Option-aware calibration needs both `option_surface_path` and option surface column information when inference cannot identify quote timestamp, expiry, strike, option type, and quote values. If the option file contains multiple quote timestamps, pass `option_surface_valuation_time_utc`.

**Known-news behavior is unclear**

Market profiles may recommend known news without applying it automatically. Pass `known_news="none"` or `--known-news none` to make the no-known-news baseline explicit, or pass the known-news preset name that should be used.

## See also

- [Fit models](fit-models.md)
- [Simulate paths](simulate-paths.md)
- [Price derivatives](price-derivatives.md)
- [Inspect and report](inspect-and-report.md)
- [Command line interface](../interfaces/cli.md)
- [Python library](../interfaces/python.md)
- [CLI reference](../reference/cli.md)
- [Python API reference](../reference/python-api.md)
