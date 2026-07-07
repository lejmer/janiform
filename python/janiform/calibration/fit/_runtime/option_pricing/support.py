from __future__ import annotations

from datetime import datetime
from math import log
from typing import TYPE_CHECKING, Any, Mapping, cast

from ....._timestamps import datetime_to_utc_nanos, utc_nanos_to_datetime
from .....contracts.base import require_positive_finite_scalar
from .....interval_factor_grids import (
    IntervalNewsIntensityProviderMaterializedSource,
    provide_interval_factor_grid,
)
from .....pricing import PricingIntervalSchedule
from ....errors import calibration_error
from ..payloads import resolved_known_news_event_timestamps, surprise_baseline_intensities
from ..seasonality import (
    build_runtime_interval_seasonality_source_from_model,
    surprise_deterministic_time_multiplier_by_interval_index,
)

if TYPE_CHECKING:
    from .....model import JaniformModel


def build_model_pricing_interval_factor_grid(
    model: JaniformModel,
    *,
    runtime_factors: Any,
    range_start_utc: datetime,
    range_end_utc: datetime,
) -> Any:
    surprise_open_baseline_intensity, surprise_closed_baseline_intensity = (
        surprise_baseline_intensities(model)
    )

    return provide_interval_factor_grid(
        calendar_registry=runtime_factors.calendar_registry,
        exchange_code=runtime_factors.exchange_code,
        range_start_utc=range_start_utc,
        range_end_utc=range_end_utc,
        interval=f"{runtime_factors.interval_duration_nanos}ns",
        seasonality_source=build_runtime_interval_seasonality_source_from_model(
            model,
            calendar_registry=runtime_factors.calendar_registry,
            exchange_code=runtime_factors.exchange_code,
            range_start_utc=range_start_utc,
            range_end_utc=range_end_utc,
            interval=f"{runtime_factors.interval_duration_nanos}ns",
        ),
        news_intensity_source=IntervalNewsIntensityProviderMaterializedSource(
            known_scheduled_event_timestamps_utc=resolved_known_news_event_timestamps(model),
            surprise_open_baseline_intensity=surprise_open_baseline_intensity,
            surprise_closed_baseline_intensity=surprise_closed_baseline_intensity,
            surprise_deterministic_time_multiplier_by_interval_index=(
                surprise_deterministic_time_multiplier_by_interval_index(
                    model,
                    range_start_utc=range_start_utc,
                    range_end_utc=range_end_utc,
                    interval_duration_nanos=runtime_factors.interval_duration_nanos,
                )
            ),
        ),
    )


def price_observed_log_return_from_spot(model: JaniformModel, *, spot: float) -> float:
    normalized_spot = require_positive_finite_scalar(spot, "spot", calibration_error)
    return log(normalized_spot) - log(model_reference_price(model))


def model_reference_price(model: JaniformModel) -> float:
    simulation_output_preferences = cast(
        Mapping[str, Any],
        model.metadata.get("simulation_output_preferences", {}),
    )

    raw_reference_price = simulation_output_preferences.get("reference_price")
    if raw_reference_price is None:
        raise calibration_error(
            "metadata.simulation_output_preferences.reference_price",
            "must be present so JaniformModel can derive pricing state from spot",
        )

    return require_positive_finite_scalar(
        raw_reference_price,
        "metadata.simulation_output_preferences.reference_price",
        calibration_error,
    )


def runtime_schedule_interval_factor_grid_coverage(
    valuation_time_utc: datetime,
    runtime_interval_schedule: PricingIntervalSchedule,
    *,
    interval_duration_nanos: int,
) -> tuple[datetime, datetime]:
    if interval_duration_nanos <= 0:
        raise calibration_error(
            "interval_factor_grid.interval_duration_nanos",
            "must be strictly positive",
        )

    valuation_time_utc_nanos = datetime_to_utc_nanos(valuation_time_utc)

    coverage_start_utc_nanos = valuation_time_utc_nanos - (
        valuation_time_utc_nanos % interval_duration_nanos
    )

    terminal_interval_end_utc_nanos = (
        runtime_interval_schedule.interval_end_timestamps_utc_nanos[-1]
    )

    coverage_end_utc_nanos = (
        (terminal_interval_end_utc_nanos + interval_duration_nanos - 1)
        // interval_duration_nanos
    ) * interval_duration_nanos

    return (
        utc_nanos_to_datetime(coverage_start_utc_nanos),
        utc_nanos_to_datetime(coverage_end_utc_nanos),
    )


def interval_factor_grid_interval_count(
    range_start_utc: datetime,
    range_end_utc: datetime,
    *,
    interval_duration_nanos: int,
) -> int:
    if interval_duration_nanos <= 0:
        raise calibration_error(
            "interval_factor_grid.interval_duration_nanos",
            "must be strictly positive",
        )

    interval_count = (
        datetime_to_utc_nanos(range_end_utc) - datetime_to_utc_nanos(range_start_utc)
    ) // interval_duration_nanos

    if interval_count <= 0:
        raise calibration_error(
            "runtime_interval_schedule",
            "must span at least one interval factor grid source interval",
        )

    return int(interval_count)
