# Janiform

Janiform is a calibrator and generator of a new affine Hawkes–NIG model with coupled news gaps.
The Rust core provides the computation engine.
The Python package exposes an importable API and CLI via PyO3 bindings.

## Baseline Support

- Python: `>=3.12`
- Rust: `>=1.89`

## Tooling

- Build backend: `maturin`
- Python-Rust bindings: `PyO3`

## Project Layout

- `crates/core/`: Rust core library
- `crates/pybind/`: PyO3 extension module (`janiform._native`)
- `python/janiform/`: Python package and CLI entrypoints
- `tests/`: Python tests and shared fixtures
- `docs/`: Design and development documentation

## Quickstart

```bash
maturin develop
python3 -m janiform --help
janiform --help
```

## Calendar Query Example

```python
from datetime import UTC, datetime

from janiform import CalendarRegistry, MarketState

calendar_registry_json = """
{
  "schema_version": "calendar.registry.v1",
  "artifact_date": "2026-01-31",
  "exchanges": [
    {
      "schema_version": "calendar.v1",
      "exchange_code": "XNYS",
      "exchange_name": "New York Stock Exchange",
      "timezone_name": "America/New_York",
      "rule_metadata": {
        "holiday_calendar_code": "US_EQUITIES",
        "early_close_rule_set": "US_EQUITIES_EARLY_CLOSE"
      },
      "regular_sessions": [
        {
          "weekday": "Monday",
          "windows": [
            {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
          ]
        }
      ],
      "holiday_dates": [],
      "early_close_sessions": []
    }
  ]
}
"""

calendar_registry = CalendarRegistry(calendar_registry_json)
market_state = calendar_registry.resolve_market_state(
    "XNYS",
    datetime(2026, 2, 2, 14, 30, 0, tzinfo=UTC),
)
assert market_state == MarketState.OPEN
```

## Seasonality Timestamp Event Input Schema

Use `preprocess_timestamp_events(...)` before nonparametric seasonality estimation.

- Schema version: `seasonality.timestamp_event.v1`
- Required event field: `'timestamp'`
- Accepted timestamp types:
  - timezone-aware `datetime`
  - RFC3339 or ISO-8601 string with timezone offset
- Exchange context:
  - `exchange_code` is required
  - session classification is resolved through `CalendarRegistry`
  - timestamps are normalized to UTC and exchange-local timezone
- Determinism:
  - output events are sorted by normalized UTC timestamp and input position
  - repeated runs with identical input produce identical output
- Sparse coverage diagnostics:
  - preprocessing returns missing and sparse hourly buckets for the selected session filter
- Output layout:
  - Rust returns compact columnar data suitable for large datasets
  - `market_state_codes` and `exchange_local_day_of_week` are compact `bytes` (`u8` values)

## Intraday Seasonality Estimation

Use `estimate_intraday_seasonality(...)` with preprocessed events to estimate `f_intra(tod)` from
timestamp density with explicit smoothing controls.

- Input:
  - preprocessed event output (`exchange_local_minute_of_day`)
  - estimator settings (`minutes_per_bin`, kernel, bandwidth, pseudocount)
- Output:
  - deterministic machine-readable bins (`bin_start_minute_of_day`, `intraday_factor_by_bin`)
  - normalized component with anchored mean (`mean(intraday_factor_by_bin) = 1`)

## Day Of Week, Day Of Year, And Composition

Use `estimate_day_of_week_seasonality(...)` and `estimate_day_of_year_seasonality(...)` on the
same preprocessed event output to estimate deterministic weekly and yearly effects.

- Day-of-week output:
  - fixed weekday buckets (`day_of_week_number` values `1..7`, Monday-first)
  - sparse bucket flags and fallback flags for deterministic missing-data handling
  - normalized factor component (`day_of_week_factor`) and additive log component
- Day-of-year output:
  - fixed daily buckets (`day_of_year_number` values `1..366`)
  - sparse bucket flags and fallback flags for deterministic missing-data handling
  - normalized factor component (`day_of_year_factor`) and additive log component

Use `compose_deterministic_seasonality_signal(...)` to combine intraday, day-of-week, and
day-of-year log components into a unified deterministic signal per accepted timestamp event.

- Combined signal normalization:
  - applies a deterministic mean-anchor policy over the fit window
  - enforces `mean(combined_factor_by_event) = 1` for identifiability with baseline intensity
  - fails fast if normalization assumptions are violated
- Normalization metadata:
  - policy identifier, fit-window event count, pre and post means
  - normalization constant and log offset used to anchor `s(t)`

## Developer Commands

```bash
python3 -m py_compile python/janiform/*.py python/tests/conftest.py
cargo check --workspace
```
