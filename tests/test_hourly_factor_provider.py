from datetime import UTC, datetime

import pytest

from janiform.calendar import CalendarRegistry
from janiform.hourly_factors import (
    HourlyFactorProviderMissingInputError,
    HourlyFactorProviderUnsupportedRangeError,
    HourlyNewsIntensityProviderExplicitSource,
    HourlyNewsIntensityProviderMaterializedSource,
    HourlySeasonalityProviderExplicitSource,
    HourlySeasonalityProviderMaterializedSource,
    provide_hourly_factor_contract,
)


def test_provider_rejects_missing_seasonality_source(calendar_registry: CalendarRegistry) -> None:
    with pytest.raises(HourlyFactorProviderMissingInputError, match="seasonality_source is required"):
        provide_hourly_factor_contract(
            calendar_registry=calendar_registry,
            exchange_code="XNYS",
            range_start_utc=datetime(2026, 2, 2, 14, 0, 0, tzinfo=UTC),
            range_end_utc=datetime(2026, 2, 2, 16, 0, 0, tzinfo=UTC),
            seasonality_source=None,
            news_intensity_source=HourlyNewsIntensityProviderExplicitSource(
                hourly_factors=[0.2, 0.0],
                materialization_method="explicit_hourly_input.v1",
            ),
        )


def test_provider_rejects_unsupported_range_requests(calendar_registry: CalendarRegistry) -> None:
    with pytest.raises(
        HourlyFactorProviderUnsupportedRangeError,
        match="request range is unsupported",
    ):
        provide_hourly_factor_contract(
            calendar_registry=calendar_registry,
            exchange_code="XNYS",
            range_start_utc=datetime(2026, 2, 2, 14, 30, 0, tzinfo=UTC),
            range_end_utc=datetime(2026, 2, 2, 16, 0, 0, tzinfo=UTC),
            seasonality_source=HourlySeasonalityProviderExplicitSource(
                hourly_factors=[1.0],
                materialization_method="explicit_hourly_input.v1",
            ),
            news_intensity_source=HourlyNewsIntensityProviderExplicitSource(
                hourly_factors=[0.0],
                materialization_method="explicit_hourly_input.v1",
            ),
        )


def test_provider_consumes_materialized_sources_for_hourly_grid(
    calendar_registry: CalendarRegistry,
) -> None:
    contract = provide_hourly_factor_contract(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=datetime(2026, 2, 2, 14, 0, 0, tzinfo=UTC),
        range_end_utc=datetime(2026, 2, 2, 17, 0, 0, tzinfo=UTC),
        seasonality_source=HourlySeasonalityProviderMaterializedSource(
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
            sparse_hour_minimum_observation_count=1,
        ),
        news_intensity_source=HourlyNewsIntensityProviderMaterializedSource(
            known_scheduled_event_timestamps_utc=[
                datetime(2026, 2, 2, 14, 5, 0, tzinfo=UTC),
                datetime(2026, 2, 2, 15, 10, 0, tzinfo=UTC),
            ],
            surprise_baseline_intensity=0.1,
            surprise_deterministic_time_multiplier_by_interval_index={0: 1.0, 1: 2.0, 2: 1.0},
        ),
    )

    assert contract.s_h == [1.1, 0.8, 1.4]
    assert contract.nu_h == [1.1, 1.2, 0.1]
    assert contract.seasonality_materialization_method == "seasonality.hourly_s_h.materialization.v1"
    assert contract.news_intensity_materialization_method == "seasonality.hourly_nu_h.materialization.v1"


def test_provider_consumes_explicit_sources_for_hourly_grid(
    calendar_registry: CalendarRegistry,
) -> None:
    contract = provide_hourly_factor_contract(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=datetime(2026, 2, 2, 14, 0, 0, tzinfo=UTC),
        range_end_utc=datetime(2026, 2, 2, 17, 0, 0, tzinfo=UTC),
        seasonality_source=HourlySeasonalityProviderExplicitSource(
            hourly_factors=[1.0, 0.9, 1.1],
            materialization_method="shared_precomputed_hourly_s_h.v1",
        ),
        news_intensity_source=HourlyNewsIntensityProviderExplicitSource(
            hourly_factors=[0.2, 0.0, 0.3],
            materialization_method="shared_precomputed_hourly_nu_h.v1",
        ),
    )

    assert contract.s_h == [1.0, 0.9, 1.1]
    assert contract.nu_h == [0.2, 0.0, 0.3]
    assert contract.seasonality_materialization_method == "shared_precomputed_hourly_s_h.v1"
    assert contract.news_intensity_materialization_method == "shared_precomputed_hourly_nu_h.v1"


def test_provider_resamples_hourly_sources_to_sub_hour_intervals(
    calendar_registry: CalendarRegistry,
) -> None:
    contract = provide_hourly_factor_contract(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=datetime(2026, 2, 2, 14, 0, 0, tzinfo=UTC),
        range_end_utc=datetime(2026, 2, 2, 16, 0, 0, tzinfo=UTC),
        interval="30min",
        seasonality_source=HourlySeasonalityProviderExplicitSource(
            hourly_factors=[1.2, 0.8],
            materialization_method="shared_precomputed_hourly_s_h.v1",
        ),
        news_intensity_source=HourlyNewsIntensityProviderExplicitSource(
            hourly_factors=[0.3, 0.1],
            materialization_method="shared_precomputed_hourly_nu_h.v1",
        ),
    )

    assert contract.interval_duration_nanos == 1_800_000_000_000
    assert contract.s_h == [1.2, 1.2, 0.8, 0.8]
    assert contract.nu_h == [0.3, 0.3, 0.1, 0.1]
