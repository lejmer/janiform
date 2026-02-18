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
from tests.fixture_validation import CalendarRegressionCase, CalendarRegressionFixtureDocument


def test_calendar_regression_fixture_covers_required_case_tags(
    calendar_regression_fixture_document: CalendarRegressionFixtureDocument,
    calendar_regression_cases: list[CalendarRegressionCase],
) -> None:
    required_case_tags: set[str] = set(
        calendar_regression_fixture_document["canonical_regression_metadata"]["required_tags"],
    )
    observed_tags: set[str] = {
        cast(str, tag)
        for regression_case in calendar_regression_cases
        for tag in cast(list[str], regression_case["tags"])
    }

    assert required_case_tags <= observed_tags


def test_calendar_boundary_fixture_identifiers_remain_stable(
    calendar_regression_fixture_document: CalendarRegressionFixtureDocument,
    calendar_regression_cases: list[CalendarRegressionCase],
) -> None:
    stable_fixture_ids_by_tag: dict[str, list[str]] = calendar_regression_fixture_document[
        "canonical_regression_metadata"
    ]["stable_fixture_ids_by_tag"]
    expected_boundary_fixture_ids: set[str] = set(stable_fixture_ids_by_tag["boundary"])
    observed_boundary_fixture_ids: set[str] = {
        _fixture_id(regression_case)
        for regression_case in calendar_regression_cases
        if _case_has_tag(regression_case, "boundary")
    }

    assert observed_boundary_fixture_ids == expected_boundary_fixture_ids


def test_historical_holiday_fixture_coverage_spans_years_and_intraday_slots(
    calendar_regression_fixture_document: CalendarRegressionFixtureDocument,
    calendar_regression_cases: list[CalendarRegressionCase],
) -> None:
    canonical_metadata: dict[str, object] = cast(
        dict[str, object],
        calendar_regression_fixture_document["canonical_regression_metadata"],
    )
    historical_holiday_cases: list[CalendarRegressionCase] = [
        regression_case
        for regression_case in calendar_regression_cases
        if _case_has_tag(regression_case, "historical_holiday")
    ]

    assert historical_holiday_cases

    observed_years: set[int] = {
        _parse_fixture_timestamp(cast(str, regression_case["timestamp_utc"])).year
        for regression_case in historical_holiday_cases
    }
    observed_intraday_tags: set[str] = {
        tag
        for regression_case in historical_holiday_cases
        for tag in cast(list[str], regression_case["tags"])
        if tag.startswith("holiday_intraday_")
    }
    expected_years: set[int] = set(cast(list[int], canonical_metadata["historical_holiday_expected_years"]))
    expected_intraday_tags: set[str] = set(
        cast(list[str], canonical_metadata["historical_holiday_expected_intraday_tags"]),
    )

    assert observed_years == expected_years
    assert observed_intraday_tags == expected_intraday_tags


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


def test_resolve_market_state_historical_holiday_regression_cases(
    calendar_registry: CalendarRegistry,
    calendar_regression_cases: list[CalendarRegressionCase],
) -> None:
    historical_holiday_cases: list[CalendarRegressionCase] = [
        regression_case
        for regression_case in calendar_regression_cases
        if _case_has_tag(regression_case, "historical_holiday")
    ]

    assert historical_holiday_cases

    for regression_case in historical_holiday_cases:
        exchange_code: str = cast(str, regression_case["exchange_code"])
        timestamp_utc: datetime = _parse_fixture_timestamp(cast(str, regression_case["timestamp_utc"]))

        assert calendar_registry.resolve_market_state(exchange_code, timestamp_utc) == MarketState.CLOSED


def test_resolve_market_state_holiday_adjacent_non_holiday_cases(
    calendar_registry: CalendarRegistry,
    calendar_regression_cases: list[CalendarRegressionCase],
) -> None:
    adjacent_non_holiday_cases: list[CalendarRegressionCase] = [
        regression_case
        for regression_case in calendar_regression_cases
        if _case_has_tag(regression_case, "holiday_adjacent_non_holiday")
    ]

    assert adjacent_non_holiday_cases

    for regression_case in adjacent_non_holiday_cases:
        exchange_code: str = cast(str, regression_case["exchange_code"])
        timestamp_utc: datetime = _parse_fixture_timestamp(cast(str, regression_case["timestamp_utc"]))

        assert calendar_registry.resolve_market_state(exchange_code, timestamp_utc) == MarketState.OPEN


def test_resolve_market_state_early_close_transition_boundaries(
    calendar_registry: CalendarRegistry,
    calendar_regression_cases: list[CalendarRegressionCase],
) -> None:
    early_close_boundary_case_ids: list[str] = [
        "xnys_early_close_pre_boundary",
        "xnys_early_close_at_boundary",
        "xnys_early_close_post_boundary",
    ]
    early_close_boundary_cases: list[CalendarRegressionCase] = _cases_by_fixture_id(
        calendar_regression_cases,
        early_close_boundary_case_ids,
    )

    assert len(early_close_boundary_cases) == len(early_close_boundary_case_ids)

    for regression_case in early_close_boundary_cases:
        exchange_code: str = cast(str, regression_case["exchange_code"])
        timestamp_utc: datetime = _parse_fixture_timestamp(cast(str, regression_case["timestamp_utc"]))
        expected_state: MarketState = _expected_market_state(cast(str, regression_case["expected_state"]))

        assert calendar_registry.resolve_market_state(exchange_code, timestamp_utc) == expected_state


def test_resolve_market_state_delayed_open_transition_boundaries(
    calendar_registry: CalendarRegistry,
    calendar_regression_cases: list[CalendarRegressionCase],
) -> None:
    delayed_open_transition_case_ids: list[str] = [
        "xnys_delayed_open_regular_open_conflict_at_regular_open",
        "xnys_delayed_open_pre_boundary",
        "xnys_delayed_open_at_boundary",
    ]
    delayed_open_transition_cases: list[CalendarRegressionCase] = _cases_by_fixture_id(
        calendar_regression_cases,
        delayed_open_transition_case_ids,
    )

    assert len(delayed_open_transition_cases) == len(delayed_open_transition_case_ids)

    for regression_case in delayed_open_transition_cases:
        exchange_code: str = cast(str, regression_case["exchange_code"])
        timestamp_utc: datetime = _parse_fixture_timestamp(cast(str, regression_case["timestamp_utc"]))
        expected_state: MarketState = _expected_market_state(cast(str, regression_case["expected_state"]))

        assert calendar_registry.resolve_market_state(exchange_code, timestamp_utc) == expected_state


def test_resolve_market_state_anomaly_precedence_conflict_cases(
    calendar_registry: CalendarRegistry,
    calendar_regression_cases: list[CalendarRegressionCase],
) -> None:
    precedence_conflict_cases: list[CalendarRegressionCase] = [
        regression_case
        for regression_case in calendar_regression_cases
        if _case_has_tag(regression_case, "precedence_conflict")
    ]

    assert precedence_conflict_cases

    for regression_case in precedence_conflict_cases:
        exchange_code: str = cast(str, regression_case["exchange_code"])
        timestamp_utc: datetime = _parse_fixture_timestamp(cast(str, regression_case["timestamp_utc"]))

        assert calendar_registry.resolve_market_state(exchange_code, timestamp_utc) == MarketState.CLOSED


def test_anomaly_boundary_timestamps_are_timezone_aware_rfc3339(
    calendar_regression_cases: list[CalendarRegressionCase],
) -> None:
    anomaly_boundary_cases: list[CalendarRegressionCase] = [
        regression_case
        for regression_case in calendar_regression_cases
        if _case_has_tag(regression_case, "session_anomaly") and _case_has_tag(regression_case, "boundary")
    ]

    assert anomaly_boundary_cases

    for regression_case in anomaly_boundary_cases:
        timestamp_raw: str = cast(str, regression_case["timestamp_utc"])
        parsed_timestamp: datetime = _parse_fixture_timestamp(timestamp_raw)
        assert timestamp_raw.endswith("Z")
        assert parsed_timestamp.tzinfo is not None


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


def _cases_by_fixture_id(
    calendar_regression_cases: list[CalendarRegressionCase],
    fixture_ids: list[str],
) -> list[CalendarRegressionCase]:
    return [
        _find_case_by_fixture_id(calendar_regression_cases, fixture_id)
        for fixture_id in fixture_ids
    ]


def _fixture_id(regression_case: CalendarRegressionCase) -> str:
    return cast(str, regression_case["fixture_id"])


def _parse_fixture_timestamp(timestamp_utc: str) -> datetime:
    return datetime.fromisoformat(timestamp_utc.replace("Z", "+00:00")).astimezone(UTC)
