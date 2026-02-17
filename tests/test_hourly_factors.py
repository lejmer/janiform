from datetime import UTC, datetime

import pytest

from janiform.calendar import CalendarMissingExchangeError, CalendarRegistry
from janiform.hourly_factors import (
    DEFAULT_SEASONALITY_AGGREGATION_METHOD,
    DEFAULT_SEASONALITY_INTERPOLATION_METHOD,
    DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY,
    DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
    HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
    HOURLY_FACTOR_OUTPUT_SCHEMA,
    HOURLY_SEASONALITY_MATERIALIZATION_OUTPUT_SCHEMA,
    HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION,
    HourlyFactorContract,
    HourlySeasonalityMaterialization,
    HourlySeasonalityMaterializationSettings,
    build_hourly_factor_contract,
    materialize_hourly_seasonality_factors,
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


def test_materialize_hourly_seasonality_handles_sparse_and_missing_source_segments(
    calendar_registry: CalendarRegistry,
) -> None:
    materialization = materialize_hourly_seasonality_factors(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=datetime(2026, 2, 2, 13, 0, 0, tzinfo=UTC),
        range_end_utc=datetime(2026, 2, 2, 18, 0, 0, tzinfo=UTC),
        source_timestamps_utc=[
            datetime(2026, 2, 2, 14, 5, 0, tzinfo=UTC),
            datetime(2026, 2, 2, 14, 20, 0, tzinfo=UTC),
            datetime(2026, 2, 2, 15, 10, 0, tzinfo=UTC),
            datetime(2026, 2, 2, 17, 15, 0, tzinfo=UTC),
        ],
        source_seasonality_factors=[1.2, 1.0, 0.8, 1.4],
        settings=HourlySeasonalityMaterializationSettings(
            interpolation_method=DEFAULT_SEASONALITY_INTERPOLATION_METHOD,
            aggregation_method=DEFAULT_SEASONALITY_AGGREGATION_METHOD,
            sparse_fallback_policy="previous_materialized_then_global_mean.v1",
            sparse_hour_minimum_observation_count=2,
        ),
    )

    assert materialization.schema_version == HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION
    assert materialization.s_h == [1.1, 1.1, 0.8, 0.8, 1.4]
    assert materialization.source_point_count == 4
    assert materialization.interval_count == 5
    assert materialization.sparse_interval_count == 4
    assert materialization.interpolated_interval_count == 3
    assert materialization.fallback_interval_count == 1
    assert (
        materialization.interval_boundary_policy
        == HOURLY_SEASONALITY_MATERIALIZATION_OUTPUT_SCHEMA.interval_boundary_policy
    )
    assert (
        materialization.interval_alignment_policy
        == HOURLY_SEASONALITY_MATERIALIZATION_OUTPUT_SCHEMA.interval_alignment_policy
    )


def test_materialize_hourly_seasonality_is_deterministic_for_controlled_inputs(
    calendar_registry: CalendarRegistry,
) -> None:
    settings = HourlySeasonalityMaterializationSettings(
        interpolation_method="linear_between_observations.v1",
        aggregation_method="last_observation.v1",
        sparse_fallback_policy=DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY,
        sparse_hour_minimum_observation_count=2,
    )
    source_timestamps_utc = [
        datetime(2026, 2, 2, 15, 5, 0, tzinfo=UTC),
        datetime(2026, 2, 2, 14, 20, 0, tzinfo=UTC),
        datetime(2026, 2, 2, 16, 10, 0, tzinfo=UTC),
        datetime(2026, 2, 2, 15, 30, 0, tzinfo=UTC),
        datetime(2026, 2, 2, 14, 5, 0, tzinfo=UTC),
        datetime(2026, 2, 2, 16, 50, 0, tzinfo=UTC),
    ]
    source_seasonality_factors = [0.9, 1.2, 1.4, 1.1, 1.0, 1.6]

    first_materialization = materialize_hourly_seasonality_factors(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=datetime(2026, 2, 2, 14, 0, 0, tzinfo=UTC),
        range_end_utc=datetime(2026, 2, 2, 17, 0, 0, tzinfo=UTC),
        source_timestamps_utc=source_timestamps_utc,
        source_seasonality_factors=source_seasonality_factors,
        settings=settings,
    )
    second_materialization = materialize_hourly_seasonality_factors(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=datetime(2026, 2, 2, 14, 0, 0, tzinfo=UTC),
        range_end_utc=datetime(2026, 2, 2, 17, 0, 0, tzinfo=UTC),
        source_timestamps_utc=source_timestamps_utc,
        source_seasonality_factors=source_seasonality_factors,
        settings=settings,
    )
    first_snapshot = _seasonality_materialization_snapshot(first_materialization)
    second_snapshot = _seasonality_materialization_snapshot(second_materialization)

    assert first_materialization.s_h == [1.2, 1.1, 1.6]
    assert first_snapshot == second_snapshot


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


def _seasonality_materialization_snapshot(
    materialization: HourlySeasonalityMaterialization,
) -> dict[str, object]:
    return {
        "schema_version": materialization.schema_version,
        "exchange_code": materialization.exchange_code,
        "exchange_timezone_name": materialization.exchange_timezone_name,
        "request_start_utc_nanos": materialization.request_start_utc_nanos,
        "request_end_utc_nanos": materialization.request_end_utc_nanos,
        "interval_indices": list(materialization.interval_indices),
        "interval_start_utc_nanos": list(materialization.interval_start_utc_nanos),
        "interval_end_utc_nanos": list(materialization.interval_end_utc_nanos),
        "interval_start_exchange_local": list(materialization.interval_start_exchange_local),
        "interval_end_exchange_local": list(materialization.interval_end_exchange_local),
        "s_h": list(materialization.s_h),
        "source_point_count": materialization.source_point_count,
        "interval_count": materialization.interval_count,
        "sparse_interval_count": materialization.sparse_interval_count,
        "interpolated_interval_count": materialization.interpolated_interval_count,
        "fallback_interval_count": materialization.fallback_interval_count,
        "interpolation_method": materialization.interpolation_method,
        "aggregation_method": materialization.aggregation_method,
        "sparse_fallback_policy": materialization.sparse_fallback_policy,
        "sparse_hour_minimum_observation_count": materialization.sparse_hour_minimum_observation_count,
        "interval_boundary_policy": materialization.interval_boundary_policy,
        "interval_alignment_policy": materialization.interval_alignment_policy,
    }
