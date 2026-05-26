from __future__ import annotations

from datetime import UTC, datetime
from math import log
from typing import TYPE_CHECKING, Any, Mapping, cast

from ....._timestamps import NANOS_PER_HOUR, datetime_to_utc_nanos
from .....contracts.base import require_positive_finite_scalar
from .....hourly_factors import (
    HourlyNewsIntensityProviderMaterializedSource,
    provide_hourly_factor_contract,
)
from .....pricing import PricingIntervalSchedule
from ....errors import calibration_error
from ..payloads import resolved_known_news_event_timestamps, surprise_baseline_intensity
from ..seasonality import (
    build_runtime_hourly_seasonality_source_from_model,
    surprise_deterministic_time_multiplier_by_interval_index,
)

if TYPE_CHECKING:
    from .....model import JaniformModel


def build_model_pricing_hourly_factor_contract(
    model: JaniformModel,
    *,
    runtime_factors: Any,
    range_start_utc: datetime,
    range_end_utc: datetime,
) -> Any:
    return provide_hourly_factor_contract(
        calendar_registry=runtime_factors.calendar_registry,
        exchange_code=runtime_factors.exchange_code,
        range_start_utc=range_start_utc,
        range_end_utc=range_end_utc,
        interval="1h",
        seasonality_source=build_runtime_hourly_seasonality_source_from_model(
            model,
            calendar_registry=runtime_factors.calendar_registry,
            exchange_code=runtime_factors.exchange_code,
            range_start_utc=range_start_utc,
            range_end_utc=range_end_utc,
            interval="1h",
        ),
        news_intensity_source=HourlyNewsIntensityProviderMaterializedSource(
            known_scheduled_event_timestamps_utc=resolved_known_news_event_timestamps(model),
            surprise_baseline_intensity=surprise_baseline_intensity(model),
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
    simulation_output_preferences = cast(Mapping[str, Any], model.metadata.get("simulation_output_preferences", {}))
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


def runtime_schedule_hourly_factor_contract_coverage(
    valuation_time_utc: datetime,
    runtime_interval_schedule: PricingIntervalSchedule,
) -> tuple[datetime, datetime]:
    valuation_time_utc_nanos = datetime_to_utc_nanos(valuation_time_utc)
    coverage_start_utc_nanos = valuation_time_utc_nanos - (
        valuation_time_utc_nanos % NANOS_PER_HOUR
    )

    terminal_interval_end_utc_nanos = (
        runtime_interval_schedule.interval_end_timestamps_utc_nanos[-1]
    )

    coverage_end_utc_nanos = (
        (terminal_interval_end_utc_nanos + NANOS_PER_HOUR - 1) // NANOS_PER_HOUR
    ) * NANOS_PER_HOUR

    return (
        _runtime_datetime_from_utc_nanos(coverage_start_utc_nanos),
        _runtime_datetime_from_utc_nanos(coverage_end_utc_nanos),
    )


def hourly_interval_count(range_start_utc: datetime, range_end_utc: datetime) -> int:
    interval_count = (
        datetime_to_utc_nanos(range_end_utc) - datetime_to_utc_nanos(range_start_utc)
    ) // NANOS_PER_HOUR
    if interval_count <= 0:
        raise calibration_error(
            "runtime_interval_schedule",
            "must span at least one canonical hourly source interval",
        )

    return int(interval_count)


def _runtime_datetime_from_utc_nanos(timestamp_utc_nanos: int) -> datetime:
    whole_seconds, remainder_nanos = divmod(timestamp_utc_nanos, 1_000_000_000)
    return datetime.fromtimestamp(whole_seconds, tz=UTC).replace(
        microsecond=remainder_nanos // 1_000
    )
