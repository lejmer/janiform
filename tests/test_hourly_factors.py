from datetime import UTC, datetime

import pytest

from janiform.calendar import CalendarMissingExchangeError, CalendarRegistry
from janiform.hourly_factors import (
    DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
    HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
    HOURLY_FACTOR_OUTPUT_SCHEMA,
    HourlyFactorContract,
    build_hourly_factor_contract,
)
from janiform.seasonality import SeasonalityInputValidationError

ONE_HOUR_NANOS: int = 3_600_000_000_000


def test_build_hourly_factor_contract_exposes_required_schema_fields_and_types(
    calendar_registry: CalendarRegistry,
) -> None:
    contract = build_hourly_factor_contract(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=datetime(2026, 2, 2, 14, 0, 0, tzinfo=UTC),
        range_end_utc=datetime(2026, 2, 2, 17, 0, 0, tzinfo=UTC),
        seasonality_hourly_factors=[1.0, 1.1, 0.9],
        news_intensity_hourly_factors=[0.2, 0.0, 0.3],
        seasonality_materialization_method=DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
        news_intensity_materialization_method="calendar_closed_window_baseline.v1",
    )
    snapshot = _contract_snapshot(contract)

    assert contract.schema_version == HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION

    for required_field in HOURLY_FACTOR_OUTPUT_SCHEMA.required_fields:
        assert required_field in snapshot

    assert isinstance(contract.exchange_code, str)
    assert isinstance(contract.exchange_timezone_name, str)
    assert isinstance(contract.interval_indices, list)
    assert isinstance(contract.interval_start_utc_nanos[0], int)
    assert isinstance(contract.interval_end_utc_nanos[0], int)
    assert isinstance(contract.interval_start_exchange_local[0], str)
    assert isinstance(contract.s_h[0], float)
    assert isinstance(contract.nu_h[0], float)
    assert contract.interval_boundary_policy == HOURLY_FACTOR_OUTPUT_SCHEMA.interval_boundary_policy
    assert contract.interval_alignment_policy == HOURLY_FACTOR_OUTPUT_SCHEMA.interval_alignment_policy


def test_build_hourly_factor_contract_indexes_consecutive_non_overlapping_hours(
    calendar_registry: CalendarRegistry,
) -> None:
    contract = build_hourly_factor_contract(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=datetime(2026, 2, 2, 14, 0, 0, tzinfo=UTC),
        range_end_utc=datetime(2026, 2, 2, 18, 0, 0, tzinfo=UTC),
        seasonality_hourly_factors=[1.0, 1.0, 1.0, 1.0],
        news_intensity_hourly_factors=[0.0, 0.0, 0.0, 0.0],
    )

    assert contract.interval_indices == [0, 1, 2, 3]
    assert contract.interval_count == 4

    for interval_position in range(1, contract.interval_count):
        assert (
            contract.interval_start_utc_nanos[interval_position]
            == contract.interval_end_utc_nanos[interval_position - 1]
        )

    for interval_position in range(contract.interval_count):
        interval_start = contract.interval_start_utc_nanos[interval_position]
        interval_end = contract.interval_end_utc_nanos[interval_position]
        assert interval_start < interval_end
        assert interval_end - interval_start == ONE_HOUR_NANOS


def test_build_hourly_factor_contract_rejects_invalid_date_range(
    calendar_registry: CalendarRegistry,
) -> None:
    with pytest.raises(
        SeasonalityInputValidationError,
        match="range_start_utc must be earlier than range_end_utc",
    ):
        build_hourly_factor_contract(
            calendar_registry=calendar_registry,
            exchange_code="XNYS",
            range_start_utc=datetime(2026, 2, 2, 14, 0, 0, tzinfo=UTC),
            range_end_utc=datetime(2026, 2, 2, 14, 0, 0, tzinfo=UTC),
            seasonality_hourly_factors=[],
            news_intensity_hourly_factors=[],
        )


def test_build_hourly_factor_contract_rejects_unknown_exchange_identifier(
    calendar_registry: CalendarRegistry,
) -> None:
    with pytest.raises(
        CalendarMissingExchangeError,
        match="missing exchange id `MISSING`",
    ):
        build_hourly_factor_contract(
            calendar_registry=calendar_registry,
            exchange_code="MISSING",
            range_start_utc=datetime(2026, 2, 2, 14, 0, 0, tzinfo=UTC),
            range_end_utc=datetime(2026, 2, 2, 16, 0, 0, tzinfo=UTC),
            seasonality_hourly_factors=[1.0, 1.0],
            news_intensity_hourly_factors=[0.0, 0.0],
        )


def _contract_snapshot(contract: HourlyFactorContract) -> dict[str, object]:
    return {
        "schema_version": contract.schema_version,
        "exchange_code": contract.exchange_code,
        "exchange_timezone_name": contract.exchange_timezone_name,
        "request_start_utc_nanos": contract.request_start_utc_nanos,
        "request_end_utc_nanos": contract.request_end_utc_nanos,
        "interval_indices": list(contract.interval_indices),
        "interval_start_utc_nanos": list(contract.interval_start_utc_nanos),
        "interval_end_utc_nanos": list(contract.interval_end_utc_nanos),
        "interval_start_exchange_local": list(contract.interval_start_exchange_local),
        "interval_end_exchange_local": list(contract.interval_end_exchange_local),
        "s_h": list(contract.s_h),
        "nu_h": list(contract.nu_h),
        "interval_count": contract.interval_count,
        "first_interval_start_utc_nanos": contract.first_interval_start_utc_nanos,
        "last_interval_end_utc_nanos": contract.last_interval_end_utc_nanos,
        "interval_boundary_policy": contract.interval_boundary_policy,
        "interval_alignment_policy": contract.interval_alignment_policy,
        "seasonality_materialization_method": contract.seasonality_materialization_method,
        "news_intensity_materialization_method": contract.news_intensity_materialization_method,
    }
