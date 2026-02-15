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

type CalendarRegressionCase = dict[str, object]

REQUIRED_CASE_TAGS: set[str] = {
    "regular_session",
    "weekend",
    "holiday",
    "early_close",
    "boundary",
    "timezone_sensitive",
}

EXPECTED_BOUNDARY_FIXTURE_IDS: set[str] = {
    "xnys_regular_pre_open",
    "xnys_regular_at_open",
    "xnys_regular_at_close",
    "xnys_early_close_pre_boundary",
    "xnys_early_close_at_boundary",
    "xtks_before_utc_midnight_transition",
    "xtks_after_utc_midnight_transition",
}


def test_calendar_regression_fixture_covers_required_case_tags(
    calendar_regression_cases: list[CalendarRegressionCase],
) -> None:
    observed_tags: set[str] = {
        cast(str, tag)
        for regression_case in calendar_regression_cases
        for tag in cast(list[str], regression_case["tags"])
    }

    assert REQUIRED_CASE_TAGS <= observed_tags


def test_calendar_boundary_fixture_identifiers_remain_stable(
    calendar_regression_cases: list[CalendarRegressionCase],
) -> None:
    observed_boundary_fixture_ids: set[str] = {
        _fixture_id(regression_case)
        for regression_case in calendar_regression_cases
        if _case_has_tag(regression_case, "boundary")
    }

    assert observed_boundary_fixture_ids == EXPECTED_BOUNDARY_FIXTURE_IDS


def test_resolve_market_state_matches_golden_regression_matrix(
    calendar_registry: CalendarRegistry,
    calendar_regression_cases: list[CalendarRegressionCase],
) -> None:
    for regression_case in calendar_regression_cases:
        exchange_code: str = cast(str, regression_case["exchange_code"])
        timestamp_utc: datetime = _parse_fixture_timestamp(cast(str, regression_case["timestamp_utc"]))
        expected_state: MarketState = _expected_market_state(cast(str, regression_case["expected_state"]))

        resolved_state: MarketState = calendar_registry.resolve_market_state(exchange_code, timestamp_utc)

        assert resolved_state == expected_state, (
            f"fixture {_fixture_id(regression_case)} expected {expected_state.value} "
            f"for {exchange_code} at {timestamp_utc.isoformat()}"
        )


def test_resolve_market_state_boundary_policy_regression_cases(
    calendar_registry: CalendarRegistry,
    calendar_regression_cases: list[CalendarRegressionCase],
) -> None:
    boundary_cases: list[CalendarRegressionCase] = [
        regression_case
        for regression_case in calendar_regression_cases
        if _case_has_tag(regression_case, "boundary")
    ]

    assert boundary_cases

    for regression_case in boundary_cases:
        exchange_code: str = cast(str, regression_case["exchange_code"])
        timestamp_utc: datetime = _parse_fixture_timestamp(cast(str, regression_case["timestamp_utc"]))
        expected_state: MarketState = _expected_market_state(cast(str, regression_case["expected_state"]))

        assert calendar_registry.resolve_market_state(exchange_code, timestamp_utc) == expected_state


def test_resolve_market_state_timezone_sensitive_regression_cases(
    calendar_registry: CalendarRegistry,
    calendar_regression_cases: list[CalendarRegressionCase],
) -> None:
    timezone_sensitive_cases: list[CalendarRegressionCase] = [
        regression_case
        for regression_case in calendar_regression_cases
        if _case_has_tag(regression_case, "timezone_sensitive")
    ]

    assert timezone_sensitive_cases

    for regression_case in timezone_sensitive_cases:
        exchange_code: str = cast(str, regression_case["exchange_code"])
        timestamp_utc: datetime = _parse_fixture_timestamp(cast(str, regression_case["timestamp_utc"]))
        expected_state: MarketState = _expected_market_state(cast(str, regression_case["expected_state"]))

        assert calendar_registry.resolve_market_state(exchange_code, timestamp_utc) == expected_state


def test_resolve_market_state_function_matches_registry_method(
    calendar_registry_json: str,
    calendar_registry: CalendarRegistry,
    calendar_regression_cases: list[CalendarRegressionCase],
) -> None:
    at_open_case: CalendarRegressionCase = _find_case_by_fixture_id(
        calendar_regression_cases,
        "xnys_regular_at_open",
    )
    exchange_code: str = cast(str, at_open_case["exchange_code"])
    timestamp_utc: datetime = _parse_fixture_timestamp(cast(str, at_open_case["timestamp_utc"]))

    function_state: MarketState = resolve_market_state(calendar_registry_json, exchange_code, timestamp_utc)
    method_state: MarketState = calendar_registry.resolve_market_state(exchange_code, timestamp_utc)

    assert function_state == method_state


def test_resolve_market_state_raises_for_missing_exchange(calendar_registry: CalendarRegistry) -> None:
    with pytest.raises(CalendarMissingExchangeError):
        calendar_registry.resolve_market_state("MISSING", datetime(2026, 2, 2, 14, 30, 0, tzinfo=UTC))


def test_calendar_registry_raises_for_invalid_registry_json() -> None:
    invalid_registry_json: str = (
        '{"schema_version":"calendar.registry.v1","artifact_date":"2026-01-31","exchanges":[]}'
    )
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


def test_python_adapter_matches_native_resolver_outputs(
    calendar_registry_json: str,
    calendar_registry: CalendarRegistry,
    calendar_regression_cases: list[CalendarRegressionCase],
) -> None:
    native = native_module()
    native_registry = native.CalendarRegistry(calendar_registry_json)

    for regression_case in calendar_regression_cases:
        exchange_code: str = cast(str, regression_case["exchange_code"])
        timestamp_utc: datetime = _parse_fixture_timestamp(cast(str, regression_case["timestamp_utc"]))
        native_state: str = native_registry.resolve_market_state(exchange_code, timestamp_utc.isoformat())

        wrapped_state: MarketState = calendar_registry.resolve_market_state(exchange_code, timestamp_utc)
        assert wrapped_state.value == native_state


def _case_has_tag(regression_case: CalendarRegressionCase, tag: str) -> bool:
    return tag in cast(list[str], regression_case["tags"])


def _expected_market_state(expected_state_value: str) -> MarketState:
    match expected_state_value:
        case MarketState.OPEN.value:
            return MarketState.OPEN
        case MarketState.CLOSED.value:
            return MarketState.CLOSED
        case _:
            raise ValueError(f"unexpected expected_state value in regression fixture: {expected_state_value}")


def _find_case_by_fixture_id(
    calendar_regression_cases: list[CalendarRegressionCase],
    fixture_id: str,
) -> CalendarRegressionCase:
    for regression_case in calendar_regression_cases:
        if _fixture_id(regression_case) == fixture_id:
            return regression_case

    raise ValueError(f"missing regression fixture case with id: {fixture_id}")


def _fixture_id(regression_case: CalendarRegressionCase) -> str:
    return cast(str, regression_case["fixture_id"])


def _parse_fixture_timestamp(timestamp_utc: str) -> datetime:
    return datetime.fromisoformat(timestamp_utc.replace("Z", "+00:00")).astimezone(UTC)
