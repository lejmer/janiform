from datetime import UTC, datetime

from janiform.calendar import CalendarRegistry
from janiform.hourly_factors import (
    HourlyFactorContract,
    HourlyNewsIntensityMaterialization,
    HourlyNewsIntensityMaterializationSettings,
    HourlySeasonalityMaterialization,
    HourlySeasonalityMaterializationSettings,
    build_hourly_factor_contract,
    materialize_hourly_news_intensity_factors,
    materialize_hourly_seasonality_factors,
)

from examples._shared import calendar_registry_json


def main() -> None:
    calendar_registry: CalendarRegistry = CalendarRegistry(calendar_registry_json())

    range_start_utc: datetime = datetime(2026, 2, 2, 13, 0, tzinfo=UTC)
    range_end_utc: datetime = datetime(2026, 2, 2, 18, 0, tzinfo=UTC)

    seasonality_materialization: HourlySeasonalityMaterialization = materialize_hourly_seasonality_factors(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=range_start_utc,
        range_end_utc=range_end_utc,
        source_timestamps_utc=[
            datetime(2026, 2, 2, 14, 5, tzinfo=UTC),
            datetime(2026, 2, 2, 14, 20, tzinfo=UTC),
            datetime(2026, 2, 2, 15, 10, tzinfo=UTC),
            datetime(2026, 2, 2, 17, 15, tzinfo=UTC),
        ],
        source_seasonality_factors=[1.2, 1.0, 0.8, 1.4],
        settings=HourlySeasonalityMaterializationSettings(
            interpolation_method="linear_between_observations.v1",
            aggregation_method="last_observation.v1",
            sparse_fallback_policy="previous_materialized_then_global_mean.v1",
            sparse_hour_minimum_observation_count=2,
        ),
    )

    news_materialization: HourlyNewsIntensityMaterialization = materialize_hourly_news_intensity_factors(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=range_start_utc,
        range_end_utc=range_end_utc,
        settings=HourlyNewsIntensityMaterializationSettings(
            known_scheduled_event_timestamps_utc=(
                datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
                datetime(2026, 2, 2, 16, 15, tzinfo=UTC),
            ),
            surprise_baseline_intensity=0.05,
            surprise_deterministic_time_multiplier_by_interval_index={
                2: 2.0,
            },
        ),
    )

    contract: HourlyFactorContract = build_hourly_factor_contract(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=range_start_utc,
        range_end_utc=range_end_utc,
        seasonality_hourly_factors=seasonality_materialization.s_h,
        news_intensity_hourly_factors=news_materialization.nu_h,
        seasonality_materialization_method=seasonality_materialization.interpolation_method,
        news_intensity_materialization_method="known_news_count_plus_surprise_baseline.v1",
    )

    print(
        "materialization summary:",
        {
            "seasonality_interval_count": seasonality_materialization.interval_count,
            "news_interval_count": news_materialization.interval_count,
        },
    )
    print(
        "contract summary:",
        {
            "exchange_code": contract.exchange_code,
            "interval_count": contract.interval_count,
            "first_interval_start_exchange_local": contract.interval_start_exchange_local[0],
        },
    )


if __name__ == "__main__":
    main()
