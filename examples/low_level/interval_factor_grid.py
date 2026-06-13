from datetime import UTC, datetime

from janiform.calendar import CalendarRegistry
from janiform.interval_factor_grids import (
    IntervalFactorGridContract,
    IntervalNewsIntensityMaterialization,
    IntervalNewsIntensityMaterializationSettings,
    IntervalSeasonalityMaterialization,
    IntervalSeasonalityMaterializationSettings,
    build_interval_factor_grid,
    materialize_interval_news_intensity_factors,
    materialize_interval_seasonality_factors,
)

from examples._shared import calendar_registry_json


def main() -> None:
    calendar_registry: CalendarRegistry = CalendarRegistry(calendar_registry_json())

    range_start_utc: datetime = datetime(2026, 2, 2, 13, 0, tzinfo=UTC)
    range_end_utc: datetime = datetime(2026, 2, 2, 18, 0, tzinfo=UTC)

    seasonality_materialization: IntervalSeasonalityMaterialization = (
        materialize_interval_seasonality_factors(
            calendar_registry=calendar_registry,
            exchange_code="XNYS",
            range_start_utc=range_start_utc,
            range_end_utc=range_end_utc,
            interval="1h",
            source_timestamps_utc=[
                datetime(2026, 2, 2, 14, 5, tzinfo=UTC),
                datetime(2026, 2, 2, 14, 20, tzinfo=UTC),
                datetime(2026, 2, 2, 15, 10, tzinfo=UTC),
                datetime(2026, 2, 2, 17, 15, tzinfo=UTC),
            ],
            source_seasonality_factors=[1.2, 1.0, 0.8, 1.4],
            settings=IntervalSeasonalityMaterializationSettings(
                interpolation_method="linear_between_observations.v1",
                aggregation_method="last_observation.v1",
                sparse_fallback_policy="previous_materialized_then_global_mean.v1",
                sparse_interval_minimum_observation_count=2,
            ),
        )
    )

    news_materialization: IntervalNewsIntensityMaterialization = (
        materialize_interval_news_intensity_factors(
            calendar_registry=calendar_registry,
            exchange_code="XNYS",
            range_start_utc=range_start_utc,
            range_end_utc=range_end_utc,
            interval="1h",
            settings=IntervalNewsIntensityMaterializationSettings(
                known_scheduled_event_timestamps_utc=(
                    datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
                    datetime(2026, 2, 2, 16, 15, tzinfo=UTC),
                ),
                surprise_open_baseline_intensity=0.05,
                surprise_closed_baseline_intensity=0.05,
                surprise_deterministic_time_multiplier_by_interval_index={
                    2: 2.0,
                },
            ),
        )
    )

    interval_factor_grid: IntervalFactorGridContract = build_interval_factor_grid(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=range_start_utc,
        range_end_utc=range_end_utc,
        interval="1h",
        seasonality_factors=seasonality_materialization.seasonality_factor,
        news_intensity_factors=news_materialization.news_intensity,
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
        "interval factor grid summary:",
        {
            "exchange_code": interval_factor_grid.exchange_code,
            "interval_count": interval_factor_grid.interval_count,
            "first_interval_start_exchange_local": interval_factor_grid.interval_start_exchange_local[0],
        },
    )


if __name__ == "__main__":
    main()
