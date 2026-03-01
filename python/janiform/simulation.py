import json
from datetime import UTC, datetime
from types import ModuleType
from typing import Any, TypedDict, cast

from .api import native_module
from .calendar import CalendarRegistry
from .hourly_factors import (
    HourlyFactorContract,
    HourlyNewsIntensityProviderSource,
    HourlySeasonalityProviderSource,
    provide_hourly_factor_contract,
)
from .seasonality import SeasonalityInputValidationError


class HawkesKernelLoadings(TypedDict):
    plus_plus: float
    plus_minus: float
    minus_plus: float
    minus_minus: float


class HawkesKernelParameters(TypedDict):
    lambda_bar: float
    beta: float
    alpha: HawkesKernelLoadings
    rho: HawkesKernelLoadings


class HawkesExcitationState(TypedDict):
    y_plus_plus: float
    y_plus_minus: float
    y_minus_plus: float
    y_minus_minus: float


type SeededTickEventTape = dict[str, Any]


def load_hourly_factor_contract_for_simulation(
    *,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    range_start_utc: datetime,
    range_end_utc: datetime,
    seasonality_source: HourlySeasonalityProviderSource,
    news_intensity_source: HourlyNewsIntensityProviderSource,
) -> HourlyFactorContract:
    """Load interval-aligned hourly factors for the simulation path."""

    return provide_hourly_factor_contract(
        calendar_registry=calendar_registry,
        exchange_code=exchange_code,
        range_start_utc=range_start_utc,
        range_end_utc=range_end_utc,
        seasonality_source=seasonality_source,
        news_intensity_source=news_intensity_source,
    )


def run_seeded_tick_event_simulation(
    *,
    seed: int,
    start_time_utc: datetime,
    horizon_interval_count: int,
    kernel_parameters: HawkesKernelParameters,
    initial_excitation_state: HawkesExcitationState,
    hourly_factor_contract: HourlyFactorContract,
    debug_trace_enabled: bool = False,
    sample_every_nth_transition: int = 1,
    max_records_per_chunk: int = 1024,
    state_snapshot_capture: str = "disabled",
) -> SeededTickEventTape:
    native: ModuleType = native_module()
    normalized_start_time_utc = _normalize_utc_timestamp(
        start_time_utc,
        field_name="start_time_utc",
    )
    event_tape_json: str = native.run_seeded_tick_event_simulation(
        seed,
        normalized_start_time_utc.isoformat(),
        horizon_interval_count,
        kernel_parameters,
        initial_excitation_state,
        hourly_factor_contract._native_result,
        debug_trace_enabled=debug_trace_enabled,
        sample_every_nth_transition=sample_every_nth_transition,
        max_records_per_chunk=max_records_per_chunk,
        state_snapshot_capture=state_snapshot_capture,
    )

    try:
        parsed_payload: object = json.loads(event_tape_json)
    except json.JSONDecodeError as e:
        raise SeasonalityInputValidationError(
            f"seeded tick simulation payload must be valid JSON: {e.msg}",
        ) from e

    if not isinstance(parsed_payload, dict):
        raise SeasonalityInputValidationError(
            "seeded tick simulation payload must decode to an object",
        )

    return cast(SeededTickEventTape, parsed_payload)


def _normalize_utc_timestamp(timestamp_utc: datetime, *, field_name: str) -> datetime:
    if not isinstance(timestamp_utc, datetime):
        raise SeasonalityInputValidationError(
            f"{field_name} must be a datetime value with timezone information",
        )

    if timestamp_utc.tzinfo is None:
        raise SeasonalityInputValidationError(
            f"{field_name} must be timezone-aware",
        )

    return timestamp_utc.astimezone(UTC)


__all__ = [
    "HawkesExcitationState",
    "HawkesKernelLoadings",
    "HawkesKernelParameters",
    "SeededTickEventTape",
    "load_hourly_factor_contract_for_simulation",
    "run_seeded_tick_event_simulation",
]
