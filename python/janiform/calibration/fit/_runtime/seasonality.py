from __future__ import annotations

from collections.abc import Mapping, Sequence
from datetime import UTC, datetime, timedelta
from typing import Any, Final, TYPE_CHECKING, cast

from ...._native_api import native_module
from ...._timestamps import NANOS_PER_HOUR, datetime_to_utc_nanos
from ....calendar import CalendarRegistry
from ....hourly_factors import (
    HourlySeasonalityProviderExplicitSource,
    HourlySeasonalityProviderMaterializedSource,
    HourlySeasonalityProviderSource,
)
from ....hourly_factors.workflow import _parse_interval_duration_nanos
from ...errors import calibration_error
from .payloads import (
    raise_invalid_weekly_surprise_multiplier,
    require_payload_mapping,
    return_components_fitted_parameters_payload,
    seasonality_parameters_payload,
)
from .shared import RuntimeStructuralSeasonalityGrid

if TYPE_CHECKING:
    from ....model import JaniformModel


RUNTIME_COMPACT_SEASONALITY_MATERIALIZATION_METHOD: Final = (
    "runtime_compact_seasonality_parameters_open_time_mean_renormalized.v1"
)


def build_runtime_hourly_seasonality_source_from_model(
    model: JaniformModel,
    *,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    range_start_utc: datetime,
    range_end_utc: datetime,
    interval: str | timedelta,
) -> HourlySeasonalityProviderSource:
    seasonality_parameters = seasonality_parameters_payload(model)
    if not contains_structural_seasonality_parameters(seasonality_parameters):
        raise RuntimeError(
            "JaniformModel does not contain structural seasonality parameters required for runtime materialization"
        )

    clock_model = seasonality_clock_model(seasonality_parameters)
    interval_duration_nanos = _parse_interval_duration_nanos(interval, field_name="interval")

    seasonality_grid = materialize_runtime_structural_seasonality_grid(
        calendar_registry=calendar_registry,
        exchange_code=exchange_code,
        range_start_utc=range_start_utc,
        range_end_utc=range_end_utc,
        interval_duration_nanos=interval_duration_nanos,
        clock_model=clock_model,
    )

    if not seasonality_grid.seasonality_factors:
        return HourlySeasonalityProviderExplicitSource(
            hourly_factors=(),
            materialization_method=RUNTIME_COMPACT_SEASONALITY_MATERIALIZATION_METHOD,
        )

    if interval_duration_nanos < NANOS_PER_HOUR or not runtime_interval_grid_is_hour_aligned(
        range_start_utc=range_start_utc,
        range_end_utc=range_end_utc,
    ):
        return HourlySeasonalityProviderMaterializedSource(
            source_timestamps_utc=tuple(
                runtime_datetime_from_utc_nanos(timestamp_utc_nanos)
                for timestamp_utc_nanos in seasonality_grid.interval_start_utc_nanos
            ),
            source_seasonality_factors=seasonality_grid.seasonality_factors,
            interpolation_method="previous_observation_carry_forward.v1",
            aggregation_method="mean_of_observations.v1",
            sparse_fallback_policy="global_mean.v1",
            sparse_hour_minimum_observation_count=1,
        )

    return HourlySeasonalityProviderExplicitSource(
        hourly_factors=seasonality_grid.seasonality_factors,
        materialization_method=RUNTIME_COMPACT_SEASONALITY_MATERIALIZATION_METHOD,
    )


def surprise_deterministic_time_multiplier_by_interval_index(
    model: JaniformModel,
    *,
    range_start_utc: datetime,
    range_end_utc: datetime,
    interval_duration_nanos: int,
) -> dict[int, float]:
    weekly_hour_utc_multipliers = surprise_weekly_hour_utc_multipliers(model)

    normalized_range_start_utc = normalize_runtime_utc_timestamp(range_start_utc, field_name="range_start_utc")
    normalized_range_end_utc = normalize_runtime_utc_timestamp(range_end_utc, field_name="range_end_utc")

    if normalized_range_end_utc <= normalized_range_start_utc:
        return {}

    interval_duration = timedelta(seconds=interval_duration_nanos / 1_000_000_000)

    interval_count = int(
        (normalized_range_end_utc - normalized_range_start_utc).total_seconds()
        / interval_duration.total_seconds()
    )

    multiplier_by_interval: dict[int, float] = {}

    for interval_index in range(interval_count):
        interval_start_utc = normalized_range_start_utc + interval_duration * interval_index
        interval_end_utc = min(interval_start_utc + interval_duration, normalized_range_end_utc)

        interval_multiplier = average_weekly_hour_multiplier_over_interval(
            interval_start_utc=interval_start_utc,
            interval_end_utc=interval_end_utc,
            weekly_hour_utc_multipliers=weekly_hour_utc_multipliers,
        )

        if abs(interval_multiplier - 1.0) > 1.0e-12:
            multiplier_by_interval[interval_index] = interval_multiplier

    return multiplier_by_interval


def surprise_weekly_hour_utc_multipliers(model: JaniformModel) -> tuple[float, ...]:
    fitted_parameters = return_components_fitted_parameters_payload(model)

    profile = require_payload_mapping(
        fitted_parameters,
        "surprise_deterministic_intensity_profile",
        "return_components.fitted_parameters.surprise_deterministic_intensity_profile",
    )

    raw_weekly_hour_utc_multipliers = profile.get("weekly_hour_utc_multipliers")
    if not isinstance(raw_weekly_hour_utc_multipliers, list):
        raise RuntimeError(
            "JaniformModel is missing weekly surprise intensity multipliers in the fitted model payload"
        )

    weekly_hour_utc_multipliers = tuple(
        float(raw_multiplier)
        if isinstance(raw_multiplier, int | float) and not isinstance(raw_multiplier, bool)
        else raise_invalid_weekly_surprise_multiplier(bucket_index)
        for bucket_index, raw_multiplier in enumerate(raw_weekly_hour_utc_multipliers)
    )

    if len(weekly_hour_utc_multipliers) != 168:
        raise RuntimeError(
            "JaniformModel weekly surprise intensity profile must contain exactly 168 hourly multipliers"
        )

    return weekly_hour_utc_multipliers


def runtime_interval_grid_is_hour_aligned(*, range_start_utc: datetime, range_end_utc: datetime) -> bool:
    return (
        datetime_to_utc_nanos(range_start_utc) % NANOS_PER_HOUR == 0
        and datetime_to_utc_nanos(range_end_utc) % NANOS_PER_HOUR == 0
    )


def runtime_datetime_from_utc_nanos(timestamp_utc_nanos: int) -> datetime:
    seconds, nanos_remainder = divmod(timestamp_utc_nanos, 1_000_000_000)
    return datetime.fromtimestamp(seconds, tz=UTC) + timedelta(microseconds=nanos_remainder / 1000.0)


def materialize_runtime_structural_seasonality_grid(
    *,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    range_start_utc: datetime,
    range_end_utc: datetime,
    interval_duration_nanos: int,
    clock_model: Mapping[str, Any],
) -> RuntimeStructuralSeasonalityGrid:
    native_payload = native_module().materialize_runtime_structural_seasonality(
        calendar_registry._native_registry,
        exchange_code,
        range_start_utc.isoformat(),
        range_end_utc.isoformat(),
        interval_duration_nanos,
        dict(clock_model),
    )

    payload_mapping = cast(Mapping[str, Any], native_payload)

    return RuntimeStructuralSeasonalityGrid(
        interval_start_utc_nanos=tuple(
            cast(Sequence[int], payload_mapping["interval_start_utc_nanos"])
        ),
        seasonality_factors=tuple(
            cast(Sequence[float], payload_mapping["seasonality_factors"])
        ),
    )


def contains_structural_seasonality_parameters(seasonality_parameters: Mapping[str, Any]) -> bool:
    return isinstance(seasonality_parameters.get("clock_model"), Mapping)


def seasonality_clock_model(seasonality_parameters: Mapping[str, Any]) -> Mapping[str, Any]:
    return require_payload_mapping(
        seasonality_parameters,
        "clock_model",
        "seasonality.clock_model",
    )


def average_weekly_hour_multiplier_over_interval(
    *,
    interval_start_utc: datetime,
    interval_end_utc: datetime,
    weekly_hour_utc_multipliers: tuple[float, ...],
) -> float:
    if interval_end_utc <= interval_start_utc:
        return 1.0

    weighted_multiplier_sum = 0.0
    weighted_duration_seconds = 0.0

    current = interval_start_utc
    while current < interval_end_utc:
        hour_start_utc = current.replace(minute=0, second=0, microsecond=0)

        next_hour_boundary_utc = (
            current + timedelta(hours=1)
            if current == hour_start_utc
            else hour_start_utc + timedelta(hours=1)
        )

        segment_end_utc = min(next_hour_boundary_utc, interval_end_utc)
        segment_duration_seconds = (segment_end_utc - current).total_seconds()

        weighted_multiplier_sum += (
            weekly_hour_utc_multipliers[weekly_hour_bucket_index(current)] * segment_duration_seconds
        )

        weighted_duration_seconds += segment_duration_seconds

        current = segment_end_utc

    if weighted_duration_seconds <= 0.0:
        return 1.0

    return weighted_multiplier_sum / weighted_duration_seconds


def weekly_hour_bucket_index(timestamp_utc: datetime) -> int:
    normalized_timestamp_utc = normalize_runtime_utc_timestamp(timestamp_utc, field_name="timestamp_utc")
    return normalized_timestamp_utc.weekday() * 24 + normalized_timestamp_utc.hour


def normalize_runtime_utc_timestamp(timestamp_utc: datetime, *, field_name: str) -> datetime:
    if timestamp_utc.tzinfo is None:
        raise calibration_error(field_name, "must be timezone-aware")

    return timestamp_utc.astimezone(UTC)
