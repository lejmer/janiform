from datetime import UTC, datetime

import pytest

from janiform.calendar import CalendarRegistry
from janiform.interval_factor_grids import (
    IntervalFactorGridProviderMissingInputError,
    IntervalFactorGridProviderUnsupportedRangeError,
    IntervalNewsIntensityProviderExplicitSource,
    IntervalNewsIntensityProviderMaterializedSource,
    IntervalSeasonalityProviderExplicitSource,
    IntervalSeasonalityProviderMaterializedSource,
    provide_interval_factor_grid,
)


def test_provider_rejects_missing_seasonality_source(calendar_registry: CalendarRegistry) -> None:
    with pytest.raises(
        IntervalFactorGridProviderMissingInputError,
        match="seasonality_source is required",
    ):
        provide_interval_factor_grid(
            calendar_registry=calendar_registry,
            exchange_code="XNYS",
            range_start_utc=datetime(2026, 2, 2, 14, 0, 0, tzinfo=UTC),
            range_end_utc=datetime(2026, 2, 2, 16, 0, 0, tzinfo=UTC),
            seasonality_source=None,
            news_intensity_source=IntervalNewsIntensityProviderExplicitSource(
                interval_factors=[0.2, 0.0],
                materialization_method="explicit_interval_grid_input.v1",
            ),
        )


def test_provider_rejects_unsupported_range_requests(calendar_registry: CalendarRegistry) -> None:
    with pytest.raises(
        IntervalFactorGridProviderUnsupportedRangeError,
        match="request range is unsupported",
    ):
        provide_interval_factor_grid(
            calendar_registry=calendar_registry,
            exchange_code="XNYS",
            range_start_utc=datetime(2026, 2, 2, 14, 30, 0, tzinfo=UTC),
            range_end_utc=datetime(2026, 2, 2, 16, 0, 0, tzinfo=UTC),
            seasonality_source=IntervalSeasonalityProviderExplicitSource(
                interval_factors=[1.0],
                materialization_method="explicit_interval_grid_input.v1",
            ),
            news_intensity_source=IntervalNewsIntensityProviderExplicitSource(
                interval_factors=[0.0],
                materialization_method="explicit_interval_grid_input.v1",
            ),
        )


def test_provider_consumes_materialized_sources_for_interval_grid(
    calendar_registry: CalendarRegistry,
) -> None:
    contract = provide_interval_factor_grid(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=datetime(2026, 2, 2, 14, 0, 0, tzinfo=UTC),
        range_end_utc=datetime(2026, 2, 2, 17, 0, 0, tzinfo=UTC),
        seasonality_source=IntervalSeasonalityProviderMaterializedSource(
            source_timestamps_utc=[
                datetime(2026, 2, 2, 14, 5, 0, tzinfo=UTC),
                datetime(2026, 2, 2, 14, 20, 0, tzinfo=UTC),
                datetime(2026, 2, 2, 15, 10, 0, tzinfo=UTC),
                datetime(2026, 2, 2, 16, 5, 0, tzinfo=UTC),
            ],
            source_seasonality_factors=[1.0, 1.2, 0.8, 1.4],
            interpolation_method="previous_observation_carry_forward.v1",
            aggregation_method="mean_of_observations.v1",
            sparse_fallback_policy="global_mean.v1",
            sparse_interval_minimum_observation_count=1,
        ),
        news_intensity_source=IntervalNewsIntensityProviderMaterializedSource(
            known_scheduled_event_timestamps_utc=[
                datetime(2026, 2, 2, 14, 5, 0, tzinfo=UTC),
                datetime(2026, 2, 2, 15, 10, 0, tzinfo=UTC),
            ],
            surprise_open_baseline_intensity=0.1,
            surprise_closed_baseline_intensity=0.1,
            surprise_deterministic_time_multiplier_by_interval_index={0: 1.0, 1: 2.0, 2: 1.0},
        ),
    )

    assert contract.seasonality_factor == [1.1, 0.8, 1.4]
    assert contract.news_intensity == [1.1, 1.2, 0.1]
    assert (
        contract.seasonality_materialization_method
        == "seasonality.interval_seasonality_factor.materialization.v1"
    )
    assert (
        contract.news_intensity_materialization_method
        == "seasonality.interval_news_intensity.materialization.v1"
    )


def test_provider_consumes_explicit_sources_for_interval_grid(
    calendar_registry: CalendarRegistry,
) -> None:
    contract = provide_interval_factor_grid(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=datetime(2026, 2, 2, 14, 0, 0, tzinfo=UTC),
        range_end_utc=datetime(2026, 2, 2, 17, 0, 0, tzinfo=UTC),
        seasonality_source=IntervalSeasonalityProviderExplicitSource(
            interval_factors=[1.0, 0.9, 1.1],
            materialization_method="shared_precomputed_interval_seasonality_factor.v1",
        ),
        news_intensity_source=IntervalNewsIntensityProviderExplicitSource(
            interval_factors=[0.2, 0.0, 0.3],
            materialization_method="shared_precomputed_interval_news_intensity.v1",
        ),
    )

    assert contract.seasonality_factor == [1.0, 0.9, 1.1]
    assert contract.news_intensity == [0.2, 0.0, 0.3]
    assert (
        contract.seasonality_materialization_method
        == "shared_precomputed_interval_seasonality_factor.v1"
    )
    assert (
        contract.news_intensity_materialization_method
        == "shared_precomputed_interval_news_intensity.v1"
    )


def test_provider_accepts_explicit_sources_aligned_to_sub_hour_interval_grid(
    calendar_registry: CalendarRegistry,
) -> None:
    contract = provide_interval_factor_grid(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=datetime(2026, 2, 2, 14, 0, 0, tzinfo=UTC),
        range_end_utc=datetime(2026, 2, 2, 16, 0, 0, tzinfo=UTC),
        interval="30min",
        seasonality_source=IntervalSeasonalityProviderExplicitSource(
            interval_factors=[1.2, 1.2, 0.8, 0.8],
            materialization_method="shared_precomputed_interval_seasonality_factor.v1",
        ),
        news_intensity_source=IntervalNewsIntensityProviderExplicitSource(
            interval_factors=[0.3, 0.3, 0.1, 0.1],
            materialization_method="shared_precomputed_interval_news_intensity.v1",
        ),
    )

    assert contract.interval_duration_nanos == 1_800_000_000_000
    assert contract.seasonality_factor == [1.2, 1.2, 0.8, 0.8]
    assert contract.news_intensity == [0.3, 0.3, 0.1, 0.1]
