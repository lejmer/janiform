from __future__ import annotations

from datetime import datetime
from math import log
from typing import TYPE_CHECKING, Any, Mapping, cast

from ....contracts import require_positive_finite_scalar
from ....hourly_factors import (
    HourlyNewsIntensityProviderMaterializedSource,
    provide_hourly_factor_contract,
)
from ...errors import calibration_error

if TYPE_CHECKING:
    from ..model import JaniformModel


def build_model_pricing_hourly_factor_contract(
    model: JaniformModel,
    *,
    runtime_factors: Any,
    valuation_time_utc: datetime,
) -> Any:
    return provide_hourly_factor_contract(
        calendar_registry=runtime_factors.calendar_registry,
        exchange_code=runtime_factors.exchange_code,
        range_start_utc=valuation_time_utc,
        range_end_utc=runtime_factors.range_end_utc,
        interval="1h",
        seasonality_source=model.build_runtime_hourly_seasonality_source_from_model(
            calendar_registry=runtime_factors.calendar_registry,
            exchange_code=runtime_factors.exchange_code,
            range_start_utc=valuation_time_utc,
            range_end_utc=runtime_factors.range_end_utc,
            interval="1h",
        ),
        news_intensity_source=HourlyNewsIntensityProviderMaterializedSource(
            known_scheduled_event_timestamps_utc=model.resolved_known_news_event_timestamps(),
            surprise_baseline_intensity=model.surprise_baseline_intensity(),
            surprise_deterministic_time_multiplier_by_interval_index=(
                model.surprise_deterministic_time_multiplier_by_interval_index(
                    range_start_utc=valuation_time_utc,
                    range_end_utc=runtime_factors.range_end_utc,
                    interval_duration_nanos=runtime_factors.interval_duration_nanos,
                )
            ),
        ),
    )


def price_observed_log_price_from_spot(model: JaniformModel, *, spot: float) -> float:
    normalized_spot = require_positive_finite_scalar(spot, "spot", calibration_error)
    return log(normalized_spot / model_reference_price(model))


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


def pricing_interval_count_from_maturity_interval(
    maturity_interval: float,
    *,
    field_path: str,
) -> int:
    horizon_interval_count_float = maturity_interval / 1.0
    horizon_interval_count = int(horizon_interval_count_float)
    if horizon_interval_count <= 0 or float(horizon_interval_count) != horizon_interval_count_float:
        raise calibration_error(
            field_path,
            "must be a positive exact multiple of the pricing runtime interval 1.0",
        )
    return horizon_interval_count


def native_pricing_interval_factors(
    hourly_factor_contract: Any,
    *,
    horizon_interval_count: int,
) -> list[dict[str, float]]:
    return [
        {
            "s_h": float(hourly_factor_contract.s_h[interval_index]),
            "nu_h_known": float(hourly_factor_contract.known_component_nu_h[interval_index]),
            "nu_h_surprise": float(hourly_factor_contract.surprise_component_nu_h[interval_index]),
        }
        for interval_index in range(horizon_interval_count)
    ]
