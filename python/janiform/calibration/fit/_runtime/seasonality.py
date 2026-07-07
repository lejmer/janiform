from __future__ import annotations

from collections.abc import Mapping, Sequence
from datetime import UTC, datetime, timedelta
from typing import Any, TYPE_CHECKING, cast

from ...._native_api import native_module
from ....calendar import CalendarRegistry
from ....interval_factor_grids import (
    IntervalSeasonalityProviderExplicitSource,
    IntervalSeasonalityProviderSource,
)
from ....interval_factor_grids.workflow import _parse_interval_duration_nanos
from ...errors import calibration_error
from .payloads import (
    require_payload_mapping,
    return_components_fitted_parameters_payload,
    seasonality_parameters_payload,
)
from .shared import RuntimeStructuralSeasonalityGrid

if TYPE_CHECKING:
    from ....model import JaniformModel


def build_runtime_interval_seasonality_source_from_model(
    model: JaniformModel,
    *,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    range_start_utc: datetime,
    range_end_utc: datetime,
    interval: str | timedelta,
) -> IntervalSeasonalityProviderSource:
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

    return IntervalSeasonalityProviderExplicitSource(
        interval_factors=seasonality_grid.seasonality_factors,
        materialization_method=seasonality_grid.materialization_method,
    )


def surprise_deterministic_time_multiplier_by_interval_index(
    model: JaniformModel,
    *,
    range_start_utc: datetime,
    range_end_utc: datetime,
    interval_duration_nanos: int,
) -> dict[int, float]:
    normalized_range_start_utc = normalize_runtime_utc_timestamp(
        range_start_utc,
        field_name="range_start_utc",
    )

    normalized_range_end_utc = normalize_runtime_utc_timestamp(
        range_end_utc,
        field_name="range_end_utc",
    )

    native = native_module()

    native_payload = native.materialize_surprise_news_weekly_profile_multipliers_for_interval_range(
        normalized_range_start_utc.isoformat(),
        normalized_range_end_utc.isoformat(),
        interval_duration_nanos,
        dict(surprise_deterministic_intensity_profile_payload(model)),
    )

    return {
        int(interval_index): float(multiplier)
        for interval_index, multiplier in cast(Mapping[Any, Any], native_payload).items()
    }


def surprise_deterministic_intensity_profile_payload(model: JaniformModel) -> Mapping[str, Any]:
    fitted_parameters = return_components_fitted_parameters_payload(model)

    return require_payload_mapping(
        fitted_parameters,
        "surprise_deterministic_intensity_profile",
        "return_components.fitted_parameters.surprise_deterministic_intensity_profile",
    )


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
        materialization_method=str(payload_mapping["materialization_method"]),
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


def normalize_runtime_utc_timestamp(timestamp_utc: datetime, *, field_name: str) -> datetime:
    if timestamp_utc.tzinfo is None:
        raise calibration_error(field_name, "must be timezone-aware")

    return timestamp_utc.astimezone(UTC)
