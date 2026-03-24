from datetime import UTC, datetime, timedelta
from time import perf_counter
from typing import Any

from .contracts import (
    SEEDED_TICK_SIMULATION_NATIVE_CHUNK_RECORD_COUNT,
    SeededTickSimulationRequest,
)
from .response import SeededTickSimulationResponse
from ..shared import SeededTickSimulationValidationError
from ..shared.errors import (
    SIMULATION_ERROR_FACTORY,
    SIMULATION_INTEGRATION_HOURLY_FACTOR_ERROR_ID,
    SIMULATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
    SIMULATION_INTEGRATION_RESPONSE_ERROR_ID,
    _simulation_error,
    _simulation_execution_error,
    _simulation_integration_error,
)
from ...contracts import require_datetime_utc
from ..._native_api import native_exception_matches, native_module
from ...calendar import (
    CalendarInvalidTimestampError,
    CalendarInvalidTimezoneError,
    CalendarMissingExchangeError,
    CalendarRegistry,
)
from ...hourly_factors import (
    _parse_interval_duration_nanos,
    HourlyFactorContract,
    HourlyFactorProviderMissingInputError,
    HourlyFactorProviderUnsupportedRangeError,
    HourlyNewsIntensityProviderSource,
    HourlySeasonalityProviderSource,
    provide_hourly_factor_contract,
)
from ...seasonality import SeasonalityInputValidationError


def load_hourly_factor_contract_for_simulation(
    *,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    range_start_utc: datetime,
    range_end_utc: datetime,
    interval: str | timedelta = "1h",
    seasonality_source: HourlySeasonalityProviderSource,
    news_intensity_source: HourlyNewsIntensityProviderSource,
) -> HourlyFactorContract:
    """Load interval-aligned factors for the simulation path."""
    interval_duration_nanos = _parse_interval_duration_nanos(interval)
    try:
        return provide_hourly_factor_contract(
            calendar_registry=calendar_registry,
            exchange_code=exchange_code,
            range_start_utc=range_start_utc,
            range_end_utc=range_end_utc,
            interval=interval,
            seasonality_source=seasonality_source,
            news_intensity_source=news_intensity_source,
        )
    except (
        CalendarInvalidTimestampError,
        CalendarInvalidTimezoneError,
        CalendarMissingExchangeError,
        HourlyFactorProviderMissingInputError,
        HourlyFactorProviderUnsupportedRangeError,
        SeasonalityInputValidationError,
    ) as e:
        raise _simulation_error(
            "hourly_factor_contract",
            str(e),
            entrypoint="load_hourly_factor_contract_for_simulation",
            context={
                "exchange_code": exchange_code,
                "interval_duration_nanos": interval_duration_nanos,
            },
        ) from e
    except ModuleNotFoundError as e:
        raise _simulation_integration_error(
            "native hourly-factor support is unavailable",
            entrypoint="load_hourly_factor_contract_for_simulation",
            error_id=SIMULATION_INTEGRATION_HOURLY_FACTOR_ERROR_ID,
            field_path="hourly_factor_contract",
            context={
                "exchange_code": exchange_code,
                "interval_duration_nanos": interval_duration_nanos,
            },
        ) from e


def run_seeded_tick_event_simulation(
    request: SeededTickSimulationRequest,
) -> SeededTickSimulationResponse:
    if not isinstance(request, SeededTickSimulationRequest):
        raise _simulation_error("request", "must be a SeededTickSimulationRequest")

    normalized_request = SeededTickSimulationRequest(
        seed=request.seed,
        start_time_utc=require_datetime_utc(
            request.start_time_utc,
            "start_time_utc",
            SIMULATION_ERROR_FACTORY,
        ),
        horizon_interval_count=request.horizon_interval_count,
        kernel_parameters=request.kernel_parameters,
        nig_shared_plus_shift_configuration=request.nig_shared_plus_shift_configuration,
        news_process_configuration=request.news_process_configuration,
        initial_excitation_state=request.initial_excitation_state,
        mean_reversion_fitted_parameters=request.mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors=request.mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state=request.initial_mean_reversion_boundary_state,
        hourly_factor_summary=request.hourly_factor_summary,
        debug_trace_enabled=request.debug_trace_enabled,
        sample_every_nth_transition=request.sample_every_nth_transition,
        max_records_per_chunk=request.max_records_per_chunk,
        state_snapshot_capture=request.state_snapshot_capture,
        _hourly_factor_contract=request.hourly_factor_contract,
    )
    try:
        native = native_module()
    except ModuleNotFoundError as e:
        raise _simulation_integration_error(
            "native simulation module is unavailable",
            entrypoint="run_seeded_tick_event_simulation",
            error_id=SIMULATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
            context={"module_name": "janiform._native"},
        ) from e

    try:
        native_run_started = perf_counter()
        native_result: Any = native.run_seeded_tick_event_simulation(
            normalized_request.seed,
            normalized_request.start_time_utc.astimezone(UTC).isoformat(),
            normalized_request.horizon_interval_count,
            normalized_request.kernel_parameters.to_payload(),
            normalized_request.nig_shared_plus_shift_configuration.to_payload(),
            normalized_request.initial_excitation_state.to_payload(),
            normalized_request.hourly_factor_contract._native_result,
            normalized_request.news_process_configuration.to_payload(),
            normalized_request.mean_reversion_fitted_parameters.to_payload(),
            normalized_request.mean_reversion_runtime_interval_anchors.to_payload(),
            normalized_request.initial_mean_reversion_boundary_state.to_payload(),
            debug_trace_enabled=normalized_request.debug_trace_enabled,
            sample_every_nth_transition=normalized_request.sample_every_nth_transition,
            max_records_per_chunk=normalized_request.max_records_per_chunk,
            state_snapshot_capture=normalized_request.state_snapshot_capture.value,
        )
        native_runtime_seconds = perf_counter() - native_run_started
    except Exception as e:
        if native_exception_matches(native, e, "SeasonalityInputValidationError"):
            raise _simulation_execution_error(
                "native simulation rejected the normalized run request",
                entrypoint="run_seeded_tick_event_simulation",
                context={"source_category": "native_validation"},
            ) from e

        raise _simulation_execution_error(
            "seeded tick simulation failed during native execution",
            entrypoint="run_seeded_tick_event_simulation",
            context={"source_exception_type": type(e).__name__},
        ) from e

    try:
        response_build_started = perf_counter()
        response = SeededTickSimulationResponse.from_native_result(
            native_result=native_result,
            native_runtime_seconds=native_runtime_seconds,
            python_response_construction_seconds=0.0,
            chunk_record_count=SEEDED_TICK_SIMULATION_NATIVE_CHUNK_RECORD_COUNT,
        )
        response.diagnostics.python_response_construction_seconds = (
            perf_counter() - response_build_started
        )
        return response
    except SeededTickSimulationValidationError as e:
        raise _simulation_integration_error(
            "native simulation returned an invalid response payload",
            entrypoint="run_seeded_tick_event_simulation",
            error_id=SIMULATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"source_error_id": e.error_id},
        ) from e
    except Exception as e:
        raise _simulation_integration_error(
            "native simulation returned an unreadable response handle",
            entrypoint="run_seeded_tick_event_simulation",
            error_id=SIMULATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"source_exception_type": type(e).__name__},
        ) from e


__all__ = [
    "load_hourly_factor_contract_for_simulation",
    "run_seeded_tick_event_simulation",
]
