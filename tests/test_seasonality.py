from datetime import UTC, datetime
from typing import Any, Optional, cast

import pytest

from janiform.calendar import CalendarRegistry
from janiform.seasonality import (
    SessionFilter,
    SeasonalityInputValidationError,
    TimestampEventPreprocessingResult,
    preprocess_timestamp_events,
)

OPEN_MARKET_STATE_CODE: int = 1
CLOSED_MARKET_STATE_CODE: int = 0


def test_preprocess_timestamp_events_raises_for_missing_timestamp_field(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, Any]] = [{"event_id": "a"}]

    with pytest.raises(
        SeasonalityInputValidationError,
        match="missing required field 'timestamp'",
    ):
        preprocess_timestamp_events(
            event_records,
            exchange_code="XNYS",
            calendar_registry=calendar_registry,
        )


def test_preprocess_timestamp_events_raises_for_invalid_timestamp_string(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, Any]] = [{"timestamp": "not-a-timestamp"}]

    with pytest.raises(
        SeasonalityInputValidationError,
        match="invalid timestamp",
    ):
        preprocess_timestamp_events(
            event_records,
            exchange_code="XNYS",
            calendar_registry=calendar_registry,
        )


def test_preprocess_timestamp_events_normalizes_timezone_and_local_context(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, Any]] = [
        {"timestamp": "2026-02-02T14:30:00Z"},
        {"timestamp": "2026-02-02T09:30:00-05:00"},
    ]

    preprocessed = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
        session_filter=SessionFilter.ALL,
    )

    expected_timestamp_utc_nanos: int = _utc_nanos(2026, 2, 2, 14, 30, 0)

    assert preprocessed.exchange_timezone_name == "America/New_York"
    assert preprocessed.timestamp_utc_nanos == [
        expected_timestamp_utc_nanos,
        expected_timestamp_utc_nanos,
    ]
    assert preprocessed.exchange_local_minute_of_day == [570, 570]
    assert preprocessed.exchange_local_day_of_week == bytes([1, 1])
    assert preprocessed.market_state_codes == bytes([OPEN_MARKET_STATE_CODE, OPEN_MARKET_STATE_CODE])


def test_preprocess_timestamp_events_filters_by_session_state(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, Any]] = [
        {"timestamp": "2026-02-02T14:30:00Z"},
        {"timestamp": "2026-02-02T03:00:00Z"},
    ]

    open_only = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
        session_filter=SessionFilter.OPEN_ONLY,
    )
    closed_only = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
        session_filter=SessionFilter.CLOSED_ONLY,
    )

    assert open_only.accepted_record_count == 1
    assert open_only.filtered_record_count == 1
    assert open_only.market_state_codes == bytes([OPEN_MARKET_STATE_CODE])
    assert closed_only.accepted_record_count == 1
    assert closed_only.filtered_record_count == 1
    assert closed_only.market_state_codes == bytes([CLOSED_MARKET_STATE_CODE])


def test_preprocess_timestamp_events_reports_missing_and_sparse_hour_buckets(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, Any]] = [
        {"timestamp": "2026-02-02T14:30:00Z"},
        {"timestamp": "2026-02-02T16:30:00Z"},
        {"timestamp": "2026-02-02T16:31:00Z"},
    ]

    preprocessed = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
        session_filter=SessionFilter.ALL,
        sparse_hour_event_threshold=2,
    )

    assert preprocessed.missing_hour_bucket_utc_nanos == [
        _utc_nanos(2026, 2, 2, 15, 0, 0),
    ]
    assert preprocessed.sparse_hour_bucket_utc_nanos == [
        _utc_nanos(2026, 2, 2, 14, 0, 0),
    ]
    assert preprocessed.missing_hour_bucket_count == 1
    assert preprocessed.sparse_hour_bucket_count == 1


def test_preprocess_timestamp_events_is_deterministic_for_identical_input(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, Any]] = [
        {"timestamp": "2026-02-02T16:30:00Z"},
        {"timestamp": "2026-02-02T14:30:00Z"},
        {"timestamp": "2026-02-02T16:31:00Z"},
    ]

    first_result = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
        session_filter=SessionFilter.ALL,
        sparse_hour_event_threshold=2,
    )
    second_result = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
        session_filter=SessionFilter.ALL,
        sparse_hour_event_threshold=2,
    )

    assert _result_snapshot(first_result) == _result_snapshot(second_result)
    assert first_result.source_record_positions == [1, 0, 2]


def test_preprocess_timestamp_events_accepts_datetime_values(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, Any]] = [
        {"timestamp": datetime(2026, 2, 2, 14, 30, 0, tzinfo=UTC)},
    ]

    preprocessed = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
    )

    assert preprocessed.timestamp_utc_nanos == [_utc_nanos(2026, 2, 2, 14, 30, 0)]


def test_preprocess_timestamp_events_raises_for_naive_datetime_values(
    calendar_registry: CalendarRegistry,
) -> None:
    naive_datetime: datetime = cast(datetime, datetime(2026, 2, 2, 14, 30, 0))
    event_records: list[dict[str, Any]] = [{"timestamp": naive_datetime}]

    with pytest.raises(
        SeasonalityInputValidationError,
        match="naive timestamp",
    ):
        preprocess_timestamp_events(
            event_records,
            exchange_code="XNYS",
            calendar_registry=calendar_registry,
        )


def _result_snapshot(result: TimestampEventPreprocessingResult) -> dict[str, Any]:
    return {
        "schema_version": cast(str, result.schema_version),
        "input_schema_version": cast(str, result.input_schema_version),
        "exchange_code": cast(str, result.exchange_code),
        "exchange_timezone_name": cast(str, result.exchange_timezone_name),
        "source_record_positions": list(cast(list[int], result.source_record_positions)),
        "timestamp_utc_nanos": list(cast(list[int], result.timestamp_utc_nanos)),
        "exchange_local_minute_of_day": list(cast(list[int], result.exchange_local_minute_of_day)),
        "exchange_local_day_of_week": bytes(cast(bytes, result.exchange_local_day_of_week)),
        "exchange_local_day_of_year": list(cast(list[int], result.exchange_local_day_of_year)),
        "market_state_codes": bytes(cast(bytes, result.market_state_codes)),
        "missing_hour_bucket_utc_nanos": list(cast(list[int], result.missing_hour_bucket_utc_nanos)),
        "sparse_hour_bucket_utc_nanos": list(cast(list[int], result.sparse_hour_bucket_utc_nanos)),
        "input_record_count": cast(int, result.input_record_count),
        "validated_record_count": cast(int, result.validated_record_count),
        "accepted_record_count": cast(int, result.accepted_record_count),
        "filtered_record_count": cast(int, result.filtered_record_count),
        "first_event_timestamp_utc_nanos": cast(Optional[int], result.first_event_timestamp_utc_nanos),
        "last_event_timestamp_utc_nanos": cast(Optional[int], result.last_event_timestamp_utc_nanos),
        "hour_bucket_count": cast(int, result.hour_bucket_count),
        "missing_hour_bucket_count": cast(int, result.missing_hour_bucket_count),
        "sparse_hour_bucket_count": cast(int, result.sparse_hour_bucket_count),
    }


def _utc_nanos(
    year: int,
    month: int,
    day: int,
    hour: int,
    minute: int,
    second: int,
) -> int:
    timestamp = datetime(year, month, day, hour, minute, second, tzinfo=UTC)
    return int(timestamp.timestamp() * 1_000_000_000)
