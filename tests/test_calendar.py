from datetime import UTC, datetime
from typing import cast

import pytest

from janiform.api import native_module
from janiform.calendar import (
    CalendarInvalidTimestampError,
    CalendarMissingExchangeError,
    CalendarRegistry,
    CalendarRegistryLoadError,
    MarketState,
    resolve_market_state,
)


@pytest.mark.parametrize(
    ("timestamp_utc", "expected_state"),
    [
        (datetime(2026, 2, 2, 14, 29, 59, tzinfo=UTC), MarketState.CLOSED),
        (datetime(2026, 2, 2, 14, 30, 0, tzinfo=UTC), MarketState.OPEN),
        (datetime(2026, 2, 2, 20, 59, 59, tzinfo=UTC), MarketState.OPEN),
        (datetime(2026, 2, 2, 21, 0, 0, tzinfo=UTC), MarketState.CLOSED),
    ],
)
def test_resolve_market_state_regular_session_boundaries(
    calendar_registry: CalendarRegistry,
    timestamp_utc: datetime,
    expected_state: MarketState,
) -> None:
    market_state: MarketState = calendar_registry.resolve_market_state("XNYS", timestamp_utc)
    assert market_state == expected_state


def test_resolve_market_state_for_special_sessions(calendar_registry: CalendarRegistry) -> None:
    holiday_timestamp: datetime = datetime(2026, 2, 3, 15, 0, 0, tzinfo=UTC)
    before_early_close_timestamp: datetime = datetime(2026, 2, 4, 17, 59, 59, tzinfo=UTC)
    at_early_close_timestamp: datetime = datetime(2026, 2, 4, 18, 0, 0, tzinfo=UTC)

    assert calendar_registry.resolve_market_state("XNYS", holiday_timestamp) == MarketState.CLOSED
    assert calendar_registry.resolve_market_state("XNYS", before_early_close_timestamp) == MarketState.OPEN
    assert calendar_registry.resolve_market_state("XNYS", at_early_close_timestamp) == MarketState.CLOSED


def test_resolve_market_state_for_weekend(calendar_registry: CalendarRegistry) -> None:
    weekend_timestamp: datetime = datetime(2026, 2, 1, 3, 0, 0, tzinfo=UTC)
    assert calendar_registry.resolve_market_state("XNYS", weekend_timestamp) == MarketState.CLOSED


def test_resolve_market_state_function_matches_registry_method(
    calendar_registry_json: str,
    calendar_registry: CalendarRegistry,
) -> None:
    timestamp_utc: datetime = datetime(2026, 2, 2, 14, 30, 0, tzinfo=UTC)
    function_state: MarketState = resolve_market_state(calendar_registry_json, "XNYS", timestamp_utc)

    method_state: MarketState = calendar_registry.resolve_market_state("XNYS", timestamp_utc)
    assert function_state == method_state


def test_resolve_market_state_raises_for_missing_exchange(calendar_registry: CalendarRegistry) -> None:
    with pytest.raises(CalendarMissingExchangeError):
        calendar_registry.resolve_market_state("MISSING", datetime(2026, 2, 2, 14, 30, 0, tzinfo=UTC))


def test_calendar_registry_raises_for_invalid_registry_json() -> None:
    invalid_registry_json: str = '{"schema_version":"calendar.registry.v1","artifact_date":"2026-01-31","exchanges":[]}'
    with pytest.raises(CalendarRegistryLoadError):
        CalendarRegistry(invalid_registry_json)


def test_resolve_market_state_raises_for_non_datetime_timestamp(calendar_registry: CalendarRegistry) -> None:
    invalid_timestamp: datetime = cast(datetime, "2026-02-02T14:30:00Z")
    with pytest.raises(CalendarInvalidTimestampError):
        calendar_registry.resolve_market_state("XNYS", invalid_timestamp)


def test_resolve_market_state_raises_for_naive_datetime(calendar_registry: CalendarRegistry) -> None:
    invalid_timestamp: datetime = datetime(2026, 2, 2, 14, 30, 0)
    with pytest.raises(CalendarInvalidTimestampError):
        calendar_registry.resolve_market_state("XNYS", invalid_timestamp)


@pytest.mark.parametrize(
    ("exchange_code", "timestamp_utc"),
    [
        ("XNYS", datetime(2026, 2, 2, 14, 30, 0, tzinfo=UTC)),
        ("XNYS", datetime(2026, 2, 3, 15, 0, 0, tzinfo=UTC)),
        ("XNYS", datetime(2026, 2, 4, 18, 0, 0, tzinfo=UTC)),
        ("XTKS", datetime(2026, 2, 2, 0, 30, 0, tzinfo=UTC)),
    ],
)
def test_python_adapter_matches_native_resolver_outputs(
    calendar_registry_json: str,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    timestamp_utc: datetime,
) -> None:
    native = native_module()
    native_registry = native.CalendarRegistry(calendar_registry_json)
    native_state: str = native_registry.resolve_market_state(exchange_code, timestamp_utc.isoformat())

    wrapped_state: MarketState = calendar_registry.resolve_market_state(exchange_code, timestamp_utc)
    assert wrapped_state.value == native_state
