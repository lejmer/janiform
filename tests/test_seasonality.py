from datetime import UTC, datetime
from typing import Optional, cast

import pytest

from janiform.calendar import CalendarRegistry
from janiform.seasonality import (
    INTRADAY_SEASONALITY_ESTIMATE_SCHEMA_VERSION,
    IntradaySeasonalityEstimatorSettings,
    IntradaySmoothingKernel,
    SessionFilter,
    SeasonalityInputValidationError,
    estimate_intraday_seasonality,
    preprocess_timestamp_events,
)

OPEN_MARKET_STATE_CODE: int = 1
CLOSED_MARKET_STATE_CODE: int = 0


def test_preprocess_timestamp_events_raises_for_missing_timestamp_field(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, object]] = [{"event_id": "a"}]

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
    event_records: list[dict[str, object]] = [{"timestamp": "not-a-timestamp"}]

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
    event_records: list[dict[str, object]] = [
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
    event_records: list[dict[str, object]] = [
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
    event_records: list[dict[str, object]] = [
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
    event_records: list[dict[str, object]] = [
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
    event_records: list[dict[str, object]] = [
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
    event_records: list[dict[str, object]] = [{"timestamp": naive_datetime}]

    with pytest.raises(
        SeasonalityInputValidationError,
        match="naive timestamp",
    ):
        preprocess_timestamp_events(
            event_records,
            exchange_code="XNYS",
            calendar_registry=calendar_registry,
        )


def test_estimate_intraday_seasonality_recovers_synthetic_bimodal_pattern(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, object]] = _build_bimodal_intraday_event_records()
    preprocessed = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
        session_filter=SessionFilter.OPEN_ONLY,
    )
    estimate = estimate_intraday_seasonality(
        preprocessed,
        settings=IntradaySeasonalityEstimatorSettings(
            minutes_per_bin=30,
            smoothing_kernel=IntradaySmoothingKernel.GAUSSIAN,
            smoothing_bandwidth_bins=1.2,
            minimum_bin_events=1,
            pseudocount=1e-6,
        ),
    )
    open_bin_index: int = 570 // 30
    midday_bin_index: int = 720 // 30
    close_bin_index: int = 930 // 30

    assert estimate.schema_version == INTRADAY_SEASONALITY_ESTIMATE_SCHEMA_VERSION
    assert estimate.intraday_factor_by_bin[open_bin_index] > estimate.intraday_factor_by_bin[midday_bin_index]
    assert estimate.intraday_factor_by_bin[close_bin_index] > estimate.intraday_factor_by_bin[midday_bin_index]
    assert sum(estimate.intraday_factor_by_bin) / len(estimate.intraday_factor_by_bin) == pytest.approx(1.0)


def test_estimate_intraday_seasonality_handles_sparse_bins_and_day_boundary_wrap(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, object]] = [
        {"timestamp": "2026-02-02T00:00:00-05:00"},
        {"timestamp": "2026-02-02T00:00:30-05:00"},
        {"timestamp": "2026-02-02T23:59:00-05:00"},
        {"timestamp": "2026-02-02T23:59:30-05:00"},
        {"timestamp": "2026-02-03T00:00:00-05:00"},
        {"timestamp": "2026-02-03T23:59:00-05:00"},
    ]
    preprocessed = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
        session_filter=SessionFilter.ALL,
    )
    estimate = estimate_intraday_seasonality(
        preprocessed,
        settings=IntradaySeasonalityEstimatorSettings(
            minutes_per_bin=1,
            smoothing_kernel=IntradaySmoothingKernel.GAUSSIAN,
            smoothing_bandwidth_bins=2.0,
            minimum_bin_events=2,
            pseudocount=1e-6,
        ),
    )

    assert estimate.bin_count == 1_440
    assert estimate.sparse_bin_count > 1_000
    assert all(intraday_factor > 0.0 for intraday_factor in estimate.intraday_factor_by_bin)
    assert abs(estimate.intraday_factor_by_bin[0] - estimate.intraday_factor_by_bin[1_439]) < 0.4
    assert estimate.intraday_factor_by_bin[0] > estimate.intraday_factor_by_bin[720]


def test_estimate_intraday_seasonality_is_reproducible_for_fixed_input(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, object]] = _build_bimodal_intraday_event_records()
    preprocessed = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
        session_filter=SessionFilter.OPEN_ONLY,
    )
    settings = IntradaySeasonalityEstimatorSettings(
        minutes_per_bin=15,
        smoothing_kernel=IntradaySmoothingKernel.UNIFORM,
        smoothing_bandwidth_bins=2.0,
        minimum_bin_events=1,
        pseudocount=1e-6,
    )
    first_estimate = estimate_intraday_seasonality(preprocessed, settings=settings)
    second_estimate = estimate_intraday_seasonality(preprocessed, settings=settings)

    assert _intraday_estimate_snapshot(first_estimate) == _intraday_estimate_snapshot(second_estimate)


def _result_snapshot(result: object) -> dict[str, object]:
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


def _intraday_estimate_snapshot(estimate: object) -> dict[str, object]:
    return {
        "schema_version": cast(str, estimate.schema_version),
        "minutes_per_bin": cast(int, estimate.minutes_per_bin),
        "smoothing_kernel": cast(str, estimate.smoothing_kernel),
        "smoothing_bandwidth_bins": cast(float, estimate.smoothing_bandwidth_bins),
        "minimum_bin_events": cast(int, estimate.minimum_bin_events),
        "pseudocount": cast(float, estimate.pseudocount),
        "bin_start_minute_of_day": list(cast(list[int], estimate.bin_start_minute_of_day)),
        "event_count_by_bin": list(cast(list[int], estimate.event_count_by_bin)),
        "sparse_bin_flags": bytes(cast(bytes, estimate.sparse_bin_flags)),
        "smoothed_density_by_bin": list(cast(list[float], estimate.smoothed_density_by_bin)),
        "intraday_log_component_by_bin": list(cast(list[float], estimate.intraday_log_component_by_bin)),
        "intraday_factor_by_bin": list(cast(list[float], estimate.intraday_factor_by_bin)),
        "input_event_count": cast(int, estimate.input_event_count),
        "bin_count": cast(int, estimate.bin_count),
        "zero_event_bin_count": cast(int, estimate.zero_event_bin_count),
        "sparse_bin_count": cast(int, estimate.sparse_bin_count),
    }


def _build_bimodal_intraday_event_records() -> list[dict[str, object]]:
    event_records: list[dict[str, object]] = []
    for day in [2, 3, 4, 5, 6]:
        for second in range(20):
            event_records.append({"timestamp": f"2026-02-{day:02d}T09:30:{second:02d}-05:00"})
        for second in range(4):
            event_records.append({"timestamp": f"2026-02-{day:02d}T12:00:{second:02d}-05:00"})
        for second in range(18):
            event_records.append({"timestamp": f"2026-02-{day:02d}T15:30:{second:02d}-05:00"})
    return event_records


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
