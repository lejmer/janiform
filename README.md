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

## Developer Commands

```bash
python3 -m py_compile python/janiform/*.py python/tests/conftest.py
cargo check --workspace
```
