import json
from dataclasses import dataclass, field
from datetime import UTC, datetime
from enum import StrEnum
from types import ModuleType
from typing import Any, Optional, cast

from ._contracts import (
    ContractModel,
    ValidationErrorFactory,
    compute_hawkes_alpha_over_beta_spectral_radius,
    parse_json_object,
    require_datetime_utc,
    require_mapping,
    require_non_empty_string,
    require_non_negative_finite_number,
    require_non_negative_finite_scalar,
    require_non_negative_integer,
    require_positive_finite_number,
    require_positive_finite_scalar,
    require_positive_integer,
    require_sequence,
    require_string,
    serialize_deterministic_json,
    validate_exact_integer,
    validate_exact_number,
    validate_exact_string,
    validate_required_field_list,
)
from ._timestamps import datetime_to_utc_nanos
from ._native_api import native_module
from .calendar import (
    CalendarInvalidTimestampError,
    CalendarInvalidTimezoneError,
    CalendarMissingExchangeError,
    CalendarRegistry,
    CalendarRegistryLoadError,
)
from .errors import (
    ContractValidationFailure,
    JaniformErrorDetails,
    JaniformErrorCategory,
    JaniformExecutionError,
    JaniformIntegrationError,
    JaniformValidationError,
)
from .hourly_factors import (
    HourlyFactorContract,
    HourlyFactorProviderMissingInputError,
    HourlyFactorProviderUnsupportedRangeError,
    HourlyNewsIntensityProviderSource,
    HourlySeasonalityProviderSource,
    provide_hourly_factor_contract,
)
from .seasonality import SeasonalityInputValidationError

SIMULATION_VALIDATION_ERROR_ID = "simulation.validation.contract"
SIMULATION_EXECUTION_ERROR_ID = "simulation.execution.run_failed"
SIMULATION_INTEGRATION_NATIVE_MODULE_ERROR_ID = "simulation.integration.native_module_unavailable"
SIMULATION_INTEGRATION_RESPONSE_ERROR_ID = "simulation.integration.invalid_native_response"
SIMULATION_INTEGRATION_HOURLY_FACTOR_ERROR_ID = "simulation.integration.hourly_factor_contract_unavailable"

SeededTickSimulationValidationFailure = ContractValidationFailure


class SeededTickSimulationValidationError(JaniformValidationError):
    pass


def _simulation_failure(
    field_path: str,
    message: str,
    *,
    entrypoint: str = "contract",
    error_id: str = SIMULATION_VALIDATION_ERROR_ID,
    context: Optional[dict[str, str | int | float | bool | None]] = None,
) -> SeededTickSimulationValidationFailure:
    return SeededTickSimulationValidationFailure.from_context(
        error_id=error_id,
        workflow="simulation",
        entrypoint=entrypoint,
        field_path=field_path,
        message=message,
        context=context,
    )


def _simulation_error(
    field_path: str,
    message: str,
    *,
    entrypoint: str = "contract",
    error_id: str = SIMULATION_VALIDATION_ERROR_ID,
    context: Optional[dict[str, str | int | float | bool | None]] = None,
) -> SeededTickSimulationValidationError:
    return SeededTickSimulationValidationError(
        _simulation_failure(
            field_path,
            message,
            entrypoint=entrypoint,
            error_id=error_id,
            context=context,
        )
    )


def _simulation_execution_error(
    message: str,
    *,
    entrypoint: str,
    context: Optional[dict[str, str | int | float | bool | None]] = None,
) -> JaniformExecutionError:
    return JaniformExecutionError(
        JaniformErrorDetails.from_context(
            error_id=SIMULATION_EXECUTION_ERROR_ID,
            category=JaniformErrorCategory.EXECUTION,
            workflow="simulation",
            entrypoint=entrypoint,
            message=message,
            context=context,
        )
    )


def _simulation_integration_error(
    message: str,
    *,
    entrypoint: str,
    error_id: str,
    field_path: Optional[str] = None,
    context: Optional[dict[str, str | int | float | bool | None]] = None,
) -> JaniformIntegrationError:
    return JaniformIntegrationError(
        JaniformErrorDetails.from_context(
            error_id=error_id,
            category=JaniformErrorCategory.INTEGRATION,
            workflow="simulation",
            entrypoint=entrypoint,
            message=message,
            field_path=field_path,
            context=context,
        )
    )


SIMULATION_ERROR_FACTORY: ValidationErrorFactory = _simulation_error


class SeededTickEventSide(StrEnum):
    BUY = 'buy'
    SELL = 'sell'


class SeededTickEventStateSnapshotCapture(StrEnum):
    DISABLED = 'disabled'
    POST_EVENT_TRANSITION = 'post_event_transition'


def _parse_seeded_tick_state_snapshot_capture(
    raw_value: object,
    *,
    field_path: str,
) -> SeededTickEventStateSnapshotCapture:
    if not isinstance(raw_value, str):
        raise _simulation_error(field_path, "must be a string")

    try:
        return SeededTickEventStateSnapshotCapture(raw_value)
    except ValueError as e:
        raise _simulation_error(
            field_path,
            f"must be one of: disabled, post_event_transition, received '{raw_value}'",
        ) from e


@dataclass(frozen=True, slots=True)
class HawkesKernelLoadings(ContractModel):
    """Directional Hawkes loadings for one 2x2 excitation block."""

    plus_plus: float
    plus_minus: float
    minus_plus: float
    minus_minus: float

    def __post_init__(self) -> None:
        for field_name in ("plus_plus", "plus_minus", "minus_plus", "minus_minus"):
            value = cast(float, getattr(self, field_name))
            require_non_negative_finite_scalar(value, field_name, SIMULATION_ERROR_FACTORY)

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str = "kernel_loadings") -> HawkesKernelLoadings:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        return HawkesKernelLoadings(
            plus_plus=require_non_negative_finite_number(
                raw_payload,
                "plus_plus",
                f"{field_path}.plus_plus",
                SIMULATION_ERROR_FACTORY,
            ),
            plus_minus=require_non_negative_finite_number(
                raw_payload,
                "plus_minus",
                f"{field_path}.plus_minus",
                SIMULATION_ERROR_FACTORY,
            ),
            minus_plus=require_non_negative_finite_number(
                raw_payload,
                "minus_plus",
                f"{field_path}.minus_plus",
                SIMULATION_ERROR_FACTORY,
            ),
            minus_minus=require_non_negative_finite_number(
                raw_payload,
                "minus_minus",
                f"{field_path}.minus_minus",
                SIMULATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class HawkesKernelParameters(ContractModel):
    """Canonical Hawkes kernel parameters with stability validation."""

    lambda_bar: float
    beta: float
    alpha: HawkesKernelLoadings
    rho: HawkesKernelLoadings

    def __post_init__(self) -> None:
        require_positive_finite_number(
            {"lambda_bar": self.lambda_bar},
            "lambda_bar",
            "lambda_bar",
            SIMULATION_ERROR_FACTORY,
        )
        require_positive_finite_number(
            {"beta": self.beta},
            "beta",
            "beta",
            SIMULATION_ERROR_FACTORY,
        )

        spectral_radius = compute_hawkes_alpha_over_beta_spectral_radius(
            self.alpha.plus_plus,
            self.alpha.plus_minus,
            self.alpha.minus_plus,
            self.alpha.minus_minus,
            self.beta,
        )
        if spectral_radius >= 1.0:
            raise _simulation_error(
                "alpha",
                f"spectral radius(alpha / beta) must be < 1.0, found {spectral_radius}",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "kernel_parameters",
    ) -> HawkesKernelParameters:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        return HawkesKernelParameters(
            lambda_bar=require_positive_finite_number(
                raw_payload,
                "lambda_bar",
                f"{field_path}.lambda_bar",
                SIMULATION_ERROR_FACTORY,
            ),
            beta=require_positive_finite_number(
                raw_payload,
                "beta",
                f"{field_path}.beta",
                SIMULATION_ERROR_FACTORY,
            ),
            alpha=HawkesKernelLoadings.from_mapping(
                require_mapping(raw_payload, "alpha", f"{field_path}.alpha", SIMULATION_ERROR_FACTORY),
                field_path=f"{field_path}.alpha",
            ),
            rho=HawkesKernelLoadings.from_mapping(
                require_mapping(raw_payload, "rho", f"{field_path}.rho", SIMULATION_ERROR_FACTORY),
                field_path=f"{field_path}.rho",
            ),
        )


@dataclass(frozen=True, slots=True)
class HawkesExcitationState(ContractModel):
    """4D Hawkes excitation state at one simulation boundary."""

    y_plus_plus: float
    y_plus_minus: float
    y_minus_plus: float
    y_minus_minus: float

    def __post_init__(self) -> None:
        for field_name in ("y_plus_plus", "y_plus_minus", "y_minus_plus", "y_minus_minus"):
            value = cast(float, getattr(self, field_name))
            require_non_negative_finite_scalar(value, field_name, SIMULATION_ERROR_FACTORY)

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "initial_excitation_state",
    ) -> HawkesExcitationState:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        return HawkesExcitationState(
            y_plus_plus=require_non_negative_finite_number(
                raw_payload,
                "y_plus_plus",
                f"{field_path}.y_plus_plus",
                SIMULATION_ERROR_FACTORY,
            ),
            y_plus_minus=require_non_negative_finite_number(
                raw_payload,
                "y_plus_minus",
                f"{field_path}.y_plus_minus",
                SIMULATION_ERROR_FACTORY,
            ),
            y_minus_plus=require_non_negative_finite_number(
                raw_payload,
                "y_minus_plus",
                f"{field_path}.y_minus_plus",
                SIMULATION_ERROR_FACTORY,
            ),
            y_minus_minus=require_non_negative_finite_number(
                raw_payload,
                "y_minus_minus",
                f"{field_path}.y_minus_minus",
                SIMULATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class SeededTickSimulationHourlyFactorSummary(ContractModel):
    """Serializable snapshot of the hourly factor contract used by one run."""

    schema_version: str
    exchange_code: str
    interval_count: int
    interval_start_utc_nanos: tuple[int, ...]
    interval_end_utc_nanos: tuple[int, ...]
    s_h: tuple[float, ...]
    nu_h: tuple[float, ...]

    def __post_init__(self) -> None:
        if self.schema_version != "seasonality.hourly_factor.contract.v1":
            raise _simulation_error(
                "hourly_factor_summary.schema_version",
                "must be `seasonality.hourly_factor.contract.v1`",
            )

        if not self.exchange_code.strip():
            raise _simulation_error("hourly_factor_summary.exchange_code", "must be a non-empty string")
        if self.interval_count <= 0:
            raise _simulation_error("hourly_factor_summary.interval_count", "must be strictly positive")

        if len(self.interval_start_utc_nanos) != self.interval_count:
            raise _simulation_error(
                "hourly_factor_summary.interval_start_utc_nanos",
                "must contain one entry per interval",
            )
        if len(self.interval_end_utc_nanos) != self.interval_count:
            raise _simulation_error(
                "hourly_factor_summary.interval_end_utc_nanos",
                "must contain one entry per interval",
            )

        if len(self.s_h) != self.interval_count:
            raise _simulation_error("hourly_factor_summary.s_h", "must contain one entry per interval")
        if len(self.nu_h) != self.interval_count:
            raise _simulation_error("hourly_factor_summary.nu_h", "must contain one entry per interval")

        for interval_index, interval_start_utc_nanos in enumerate(self.interval_start_utc_nanos):
            if interval_start_utc_nanos < 0:
                raise _simulation_error(
                    f"hourly_factor_summary.interval_start_utc_nanos[{interval_index}]",
                    "must be non-negative",
                )

        for interval_index, interval_end_utc_nanos in enumerate(self.interval_end_utc_nanos):
            if interval_end_utc_nanos < 0:
                raise _simulation_error(
                    f"hourly_factor_summary.interval_end_utc_nanos[{interval_index}]",
                    "must be non-negative",
                )
            if interval_end_utc_nanos <= self.interval_start_utc_nanos[interval_index]:
                raise _simulation_error(
                    f"hourly_factor_summary.interval_end_utc_nanos[{interval_index}]",
                    "must be greater than the corresponding interval start timestamp",
                )

        for interval_index, seasonality_factor in enumerate(self.s_h):
            require_non_negative_finite_scalar(
                seasonality_factor,
                f"hourly_factor_summary.s_h[{interval_index}]",
                SIMULATION_ERROR_FACTORY,
            )

        for interval_index, news_intensity_factor in enumerate(self.nu_h):
            require_non_negative_finite_scalar(
                news_intensity_factor,
                f"hourly_factor_summary.nu_h[{interval_index}]",
                SIMULATION_ERROR_FACTORY,
            )

    @staticmethod
    def from_contract(hourly_factor_contract: HourlyFactorContract) -> SeededTickSimulationHourlyFactorSummary:
        return SeededTickSimulationHourlyFactorSummary(
            schema_version=hourly_factor_contract.schema_version,
            exchange_code=hourly_factor_contract.exchange_code,
            interval_count=hourly_factor_contract.interval_count,
            interval_start_utc_nanos=tuple(hourly_factor_contract.interval_start_utc_nanos),
            interval_end_utc_nanos=tuple(hourly_factor_contract.interval_end_utc_nanos),
            s_h=tuple(hourly_factor_contract.s_h),
            nu_h=tuple(hourly_factor_contract.nu_h),
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "hourly_factor_summary",
    ) -> SeededTickSimulationHourlyFactorSummary:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        raw_interval_start_utc_nanos = require_sequence(
            raw_payload,
            "interval_start_utc_nanos",
            f"{field_path}.interval_start_utc_nanos",
            SIMULATION_ERROR_FACTORY,
        )
        raw_interval_end_utc_nanos = require_sequence(
            raw_payload,
            "interval_end_utc_nanos",
            f"{field_path}.interval_end_utc_nanos",
            SIMULATION_ERROR_FACTORY,
        )
        raw_seasonality_factors = require_sequence(
            raw_payload,
            "s_h",
            f"{field_path}.s_h",
            SIMULATION_ERROR_FACTORY,
        )
        raw_news_intensity_factors = require_sequence(
            raw_payload,
            "nu_h",
            f"{field_path}.nu_h",
            SIMULATION_ERROR_FACTORY,
        )

        return SeededTickSimulationHourlyFactorSummary(
            schema_version=require_string(
                raw_payload,
                "schema_version",
                f"{field_path}.schema_version",
                SIMULATION_ERROR_FACTORY,
            ),
            exchange_code=require_string(
                raw_payload,
                "exchange_code",
                f"{field_path}.exchange_code",
                SIMULATION_ERROR_FACTORY,
            ),
            interval_count=require_positive_integer(
                raw_payload,
                "interval_count",
                f"{field_path}.interval_count",
                SIMULATION_ERROR_FACTORY,
            ),
            interval_start_utc_nanos=tuple(
                require_non_negative_integer(
                    {"value": raw_value},
                    "value",
                    f"{field_path}.interval_start_utc_nanos[{interval_index}]",
                    SIMULATION_ERROR_FACTORY,
                )
                for interval_index, raw_value in enumerate(raw_interval_start_utc_nanos)
            ),
            interval_end_utc_nanos=tuple(
                require_non_negative_integer(
                    {"value": raw_value},
                    "value",
                    f"{field_path}.interval_end_utc_nanos[{interval_index}]",
                    SIMULATION_ERROR_FACTORY,
                )
                for interval_index, raw_value in enumerate(raw_interval_end_utc_nanos)
            ),
            s_h=tuple(
                require_non_negative_finite_number(
                    {"value": raw_value},
                    "value",
                    f"{field_path}.s_h[{interval_index}]",
                    SIMULATION_ERROR_FACTORY,
                )
                for interval_index, raw_value in enumerate(raw_seasonality_factors)
            ),
            nu_h=tuple(
                require_non_negative_finite_number(
                    {"value": raw_value},
                    "value",
                    f"{field_path}.nu_h[{interval_index}]",
                    SIMULATION_ERROR_FACTORY,
                )
                for interval_index, raw_value in enumerate(raw_news_intensity_factors)
            ),
        )


@dataclass(frozen=True, slots=True)
class SeededTickSimulationRequest(ContractModel):
    """Typed seeded-tick simulation request for the importable Python surface."""

    seed: int
    start_time_utc: datetime
    horizon_interval_count: int
    kernel_parameters: HawkesKernelParameters
    initial_excitation_state: HawkesExcitationState
    hourly_factor_summary: SeededTickSimulationHourlyFactorSummary
    debug_trace_enabled: bool = False
    sample_every_nth_transition: int = 1
    max_records_per_chunk: int = 1024
    state_snapshot_capture: SeededTickEventStateSnapshotCapture = SeededTickEventStateSnapshotCapture.DISABLED
    _hourly_factor_contract: Optional[HourlyFactorContract] = field(
        default=None,
        repr=False,
        compare=False,
    )

    def to_payload(self) -> dict[str, Any]:
        payload = super().to_payload()
        payload.pop("_hourly_factor_contract", None)
        return payload

    def __post_init__(self) -> None:
        if self.seed < 0:
            raise _simulation_error("seed", "must be non-negative")
        if self.start_time_utc.tzinfo is None:
            raise _simulation_error("start_time_utc", "must be timezone-aware")
        if self.horizon_interval_count <= 0:
            raise _simulation_error("horizon_interval_count", "must be strictly positive")
        if self.hourly_factor_summary.interval_count != self.horizon_interval_count:
            raise _simulation_error(
                "hourly_factor_summary.interval_count",
                "must match horizon_interval_count",
            )
        if self.sample_every_nth_transition <= 0:
            raise _simulation_error("sample_every_nth_transition", "must be strictly positive")
        if self.max_records_per_chunk <= 0:
            raise _simulation_error("max_records_per_chunk", "must be strictly positive")

    @property
    def hourly_factor_contract(self) -> HourlyFactorContract:
        if self._hourly_factor_contract is None:
            raise _simulation_error(
                "hourly_factor_summary",
                "runtime hourly factor contract handle is unavailable on deserialized requests",
            )

        return self._hourly_factor_contract

    @staticmethod
    def from_runtime_inputs(
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
        state_snapshot_capture: SeededTickEventStateSnapshotCapture = SeededTickEventStateSnapshotCapture.DISABLED,
    ) -> SeededTickSimulationRequest:
        normalized_start_time_utc = require_datetime_utc(
            start_time_utc,
            "start_time_utc",
            SIMULATION_ERROR_FACTORY,
        )
        return SeededTickSimulationRequest(
            seed=seed,
            start_time_utc=normalized_start_time_utc,
            horizon_interval_count=horizon_interval_count,
            kernel_parameters=kernel_parameters,
            initial_excitation_state=initial_excitation_state,
            hourly_factor_summary=SeededTickSimulationHourlyFactorSummary.from_contract(hourly_factor_contract),
            debug_trace_enabled=debug_trace_enabled,
            sample_every_nth_transition=sample_every_nth_transition,
            max_records_per_chunk=max_records_per_chunk,
            state_snapshot_capture=state_snapshot_capture,
            _hourly_factor_contract=hourly_factor_contract,
        )

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any) -> SeededTickSimulationRequest:
        if not isinstance(raw_payload, dict):
            raise _simulation_error("seeded_tick_simulation_request", "must be an object")
        if "start_time_utc" not in raw_payload:
            raise _simulation_error("start_time_utc", "is required")

        raw_state_snapshot_capture = raw_payload.get("state_snapshot_capture")
        state_snapshot_capture = (
            _parse_seeded_tick_state_snapshot_capture(
                raw_state_snapshot_capture,
                field_path="state_snapshot_capture",
            )
            if raw_state_snapshot_capture is not None
            else SeededTickEventStateSnapshotCapture.DISABLED
        )

        raw_debug_trace_enabled = raw_payload.get("debug_trace_enabled", False)
        if not isinstance(raw_debug_trace_enabled, bool):
            raise _simulation_error("debug_trace_enabled", "must be a boolean")

        sample_every_nth_transition = (
            require_positive_integer(
                raw_payload,
                "sample_every_nth_transition",
                "sample_every_nth_transition",
                SIMULATION_ERROR_FACTORY,
            )
            if "sample_every_nth_transition" in raw_payload
            else 1
        )
        max_records_per_chunk = (
            require_positive_integer(
                raw_payload,
                "max_records_per_chunk",
                "max_records_per_chunk",
                SIMULATION_ERROR_FACTORY,
            )
            if "max_records_per_chunk" in raw_payload
            else 1024
        )

        return SeededTickSimulationRequest(
            seed=require_non_negative_integer(
                raw_payload,
                "seed",
                "seed",
                SIMULATION_ERROR_FACTORY,
            ),
            start_time_utc=require_datetime_utc(
                raw_payload["start_time_utc"],
                "start_time_utc",
                SIMULATION_ERROR_FACTORY,
            ),
            horizon_interval_count=require_positive_integer(
                raw_payload,
                "horizon_interval_count",
                "horizon_interval_count",
                SIMULATION_ERROR_FACTORY,
            ),
            kernel_parameters=HawkesKernelParameters.from_mapping(
                require_mapping(
                    raw_payload,
                    "kernel_parameters",
                    "kernel_parameters",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path="kernel_parameters",
            ),
            initial_excitation_state=HawkesExcitationState.from_mapping(
                require_mapping(
                    raw_payload,
                    "initial_excitation_state",
                    "initial_excitation_state",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path="initial_excitation_state",
            ),
            hourly_factor_summary=SeededTickSimulationHourlyFactorSummary.from_mapping(
                require_mapping(
                    raw_payload,
                    "hourly_factor_summary",
                    "hourly_factor_summary",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path="hourly_factor_summary",
            ),
            debug_trace_enabled=raw_debug_trace_enabled,
            sample_every_nth_transition=sample_every_nth_transition,
            max_records_per_chunk=max_records_per_chunk,
            state_snapshot_capture=state_snapshot_capture,
        )


@dataclass(frozen=True, slots=True)
class SeededTickEventStateSnapshot(ContractModel):
    """Optional post-transition state snapshot attached to one event record."""

    y_plus_plus: float
    y_plus_minus: float
    y_minus_plus: float
    y_minus_minus: float
    deseasonalized_lambda_plus: float
    deseasonalized_lambda_minus: float

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> SeededTickEventStateSnapshot:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        return SeededTickEventStateSnapshot(
            y_plus_plus=require_non_negative_finite_number(
                raw_payload,
                "y_plus_plus",
                f"{field_path}.y_plus_plus",
                SIMULATION_ERROR_FACTORY,
            ),
            y_plus_minus=require_non_negative_finite_number(
                raw_payload,
                "y_plus_minus",
                f"{field_path}.y_plus_minus",
                SIMULATION_ERROR_FACTORY,
            ),
            y_minus_plus=require_non_negative_finite_number(
                raw_payload,
                "y_minus_plus",
                f"{field_path}.y_minus_plus",
                SIMULATION_ERROR_FACTORY,
            ),
            y_minus_minus=require_non_negative_finite_number(
                raw_payload,
                "y_minus_minus",
                f"{field_path}.y_minus_minus",
                SIMULATION_ERROR_FACTORY,
            ),
            deseasonalized_lambda_plus=require_non_negative_finite_number(
                raw_payload,
                "deseasonalized_lambda_plus",
                f"{field_path}.deseasonalized_lambda_plus",
                SIMULATION_ERROR_FACTORY,
            ),
            deseasonalized_lambda_minus=require_non_negative_finite_number(
                raw_payload,
                "deseasonalized_lambda_minus",
                f"{field_path}.deseasonalized_lambda_minus",
                SIMULATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class SeededTickEventRecord(ContractModel):
    """One ordered simulated event."""

    event_index: int
    timestamp_utc_nanos: int
    side: SeededTickEventSide
    state_snapshot: Optional[SeededTickEventStateSnapshot] = None

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str) -> SeededTickEventRecord:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        raw_side = require_string(raw_payload, "side", f"{field_path}.side", SIMULATION_ERROR_FACTORY)
        try:
            side = SeededTickEventSide(raw_side)
        except ValueError as e:
            raise _simulation_error(
                f"{field_path}.side",
                f"must be one of: buy, sell, received '{raw_side}'",
            ) from e

        raw_state_snapshot = raw_payload.get("state_snapshot")
        state_snapshot = (
            SeededTickEventStateSnapshot.from_mapping(
                raw_state_snapshot,
                field_path=f"{field_path}.state_snapshot",
            )
            if raw_state_snapshot is not None
            else None
        )

        return SeededTickEventRecord(
            event_index=require_non_negative_integer(
                raw_payload,
                "event_index",
                f"{field_path}.event_index",
                SIMULATION_ERROR_FACTORY,
            ),
            timestamp_utc_nanos=require_non_negative_integer(
                raw_payload,
                "timestamp_utc_nanos",
                f"{field_path}.timestamp_utc_nanos",
                SIMULATION_ERROR_FACTORY,
            ),
            side=side,
            state_snapshot=state_snapshot,
        )


@dataclass(frozen=True, slots=True)
class SeededTickEventTapeMetadata(ContractModel):
    """Deterministic metadata attached to one serialized event tape."""

    schema_version: str
    event_record_schema_version: str
    state_snapshot_schema_version: str
    event_ordering_policy: str
    timestamp_representation_policy: str
    side_encoding_policy: str
    state_snapshot_capture: SeededTickEventStateSnapshotCapture
    state_snapshot_capture_point_policy: Optional[str] = None

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str) -> SeededTickEventTapeMetadata:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        raw_capture = require_string(
            raw_payload,
            "state_snapshot_capture",
            f"{field_path}.state_snapshot_capture",
            SIMULATION_ERROR_FACTORY,
        )
        try:
            capture = SeededTickEventStateSnapshotCapture(raw_capture)
        except ValueError as e:
            raise _simulation_error(
                f"{field_path}.state_snapshot_capture",
                "must be either 'disabled' or 'post_event_transition'",
            ) from e

        raw_capture_policy = raw_payload.get("state_snapshot_capture_point_policy")
        if raw_capture_policy is not None and not isinstance(raw_capture_policy, str):
            raise _simulation_error(
                f"{field_path}.state_snapshot_capture_point_policy",
                "must be a string when provided",
            )

        return SeededTickEventTapeMetadata(
            schema_version=require_string(raw_payload, "schema_version", f"{field_path}.schema_version", SIMULATION_ERROR_FACTORY),
            event_record_schema_version=require_string(
                raw_payload,
                "event_record_schema_version",
                f"{field_path}.event_record_schema_version",
                SIMULATION_ERROR_FACTORY,
            ),
            state_snapshot_schema_version=require_string(
                raw_payload,
                "state_snapshot_schema_version",
                f"{field_path}.state_snapshot_schema_version",
                SIMULATION_ERROR_FACTORY,
            ),
            event_ordering_policy=require_string(
                raw_payload,
                "event_ordering_policy",
                f"{field_path}.event_ordering_policy",
                SIMULATION_ERROR_FACTORY,
            ),
            timestamp_representation_policy=require_string(
                raw_payload,
                "timestamp_representation_policy",
                f"{field_path}.timestamp_representation_policy",
                SIMULATION_ERROR_FACTORY,
            ),
            side_encoding_policy=require_string(
                raw_payload,
                "side_encoding_policy",
                f"{field_path}.side_encoding_policy",
                SIMULATION_ERROR_FACTORY,
            ),
            state_snapshot_capture=capture,
            state_snapshot_capture_point_policy=cast(Optional[str], raw_capture_policy),
        )


@dataclass(frozen=True, slots=True)
class SeededTickSimulationResponse(ContractModel):
    """Typed seeded-tick simulation response containing the canonical event tape."""

    schema_version: str
    metadata: SeededTickEventTapeMetadata
    event_tape: tuple[SeededTickEventRecord, ...]

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "seeded_tick_simulation_response",
    ) -> SeededTickSimulationResponse:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        event_tape_items = require_sequence(raw_payload, "event_tape", f"{field_path}.event_tape", SIMULATION_ERROR_FACTORY)
        event_tape = tuple(
            SeededTickEventRecord.from_mapping(
                raw_item,
                field_path=f"{field_path}.event_tape[{event_index}]",
            )
            for event_index, raw_item in enumerate(event_tape_items)
        )

        return SeededTickSimulationResponse(
            schema_version=require_string(
                raw_payload,
                "schema_version",
                f"{field_path}.schema_version",
                SIMULATION_ERROR_FACTORY,
            ),
            metadata=SeededTickEventTapeMetadata.from_mapping(
                require_mapping(raw_payload, "metadata", f"{field_path}.metadata", SIMULATION_ERROR_FACTORY),
                field_path=f"{field_path}.metadata",
            ),
            event_tape=event_tape,
        )


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
    try:
        return provide_hourly_factor_contract(
            calendar_registry=calendar_registry,
            exchange_code=exchange_code,
            range_start_utc=range_start_utc,
            range_end_utc=range_end_utc,
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
            context={"exchange_code": exchange_code},
        ) from e
    except ModuleNotFoundError as e:
        raise _simulation_integration_error(
            "native hourly-factor support is unavailable",
            entrypoint="load_hourly_factor_contract_for_simulation",
            error_id=SIMULATION_INTEGRATION_HOURLY_FACTOR_ERROR_ID,
            field_path="hourly_factor_contract",
            context={"exchange_code": exchange_code},
        ) from e


def validate_seeded_tick_simulation_response(
    raw_response: dict[str, Any] | Any,
) -> SeededTickSimulationResponse:
    return SeededTickSimulationResponse.from_mapping(raw_response)


def serialize_seeded_tick_simulation_response(response: SeededTickSimulationResponse) -> str:
    return serialize_deterministic_json(response)


def deserialize_seeded_tick_simulation_response(payload_json: str) -> SeededTickSimulationResponse:
    raw_payload = parse_json_object(payload_json, "seeded tick simulation response", SIMULATION_ERROR_FACTORY)
    return validate_seeded_tick_simulation_response(dict(raw_payload))


def run_seeded_tick_event_simulation(
    request: SeededTickSimulationRequest,
) -> SeededTickSimulationResponse:
    """Run the seeded tick simulator from a typed request.

    Example:
        request = SeededTickSimulationRequest.from_runtime_inputs(
            seed=17,
            start_time_utc=start_time_utc,
            horizon_interval_count=2,
            kernel_parameters=kernel_parameters,
            initial_excitation_state=initial_excitation_state,
            hourly_factor_contract=hourly_factor_contract,
        )
        response = run_seeded_tick_event_simulation(request)
    """

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
        initial_excitation_state=request.initial_excitation_state,
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
        event_tape_json: str = native.run_seeded_tick_event_simulation(
            normalized_request.seed,
            normalized_request.start_time_utc.astimezone(UTC).isoformat(),
            normalized_request.horizon_interval_count,
            normalized_request.kernel_parameters.to_payload(),
            normalized_request.initial_excitation_state.to_payload(),
            normalized_request.hourly_factor_contract._native_result,
            debug_trace_enabled=normalized_request.debug_trace_enabled,
            sample_every_nth_transition=normalized_request.sample_every_nth_transition,
            max_records_per_chunk=normalized_request.max_records_per_chunk,
            state_snapshot_capture=normalized_request.state_snapshot_capture.value,
        )
    except native.SeasonalityInputValidationError as e:
        raise _simulation_execution_error(
            "native simulation rejected the normalized run request",
            entrypoint="run_seeded_tick_event_simulation",
            context={"source_category": "native_validation"},
        ) from e
    except Exception as e:
        raise _simulation_execution_error(
            "seeded tick simulation failed during native execution",
            entrypoint="run_seeded_tick_event_simulation",
            context={"source_exception_type": type(e).__name__},
        ) from e

    try:
        return deserialize_seeded_tick_simulation_response(event_tape_json)
    except SeededTickSimulationValidationError as e:
        raise _simulation_integration_error(
            "native simulation returned an invalid response payload",
            entrypoint="run_seeded_tick_event_simulation",
            error_id=SIMULATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"source_error_id": e.error_id},
        ) from e


NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION = "news.process.configuration.v1"
COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION = "simulation.coarse_interval.run_request.v1"
COARSE_SIMULATION_PATH_SCHEMA_VERSION = "simulation.coarse_interval.path.v1"
COARSE_SIMULATION_PATH_RECORD_SCHEMA_VERSION = "simulation.coarse_interval.path_record.v1"
COARSE_SIMULATION_PATH_OUTPUT_SCHEMA_METADATA_VERSION = "simulation.coarse_interval.path_output_schema_metadata.v1"
COARSE_SIMULATION_PATH_PROVENANCE_SCHEMA_VERSION = "simulation.coarse_interval.path_provenance.v1"
COARSE_SIMULATION_PATH_DIAGNOSTICS_SCHEMA_VERSION = "simulation.coarse_interval.path_diagnostics.v1"
COARSE_SIMULATION_ARTIFACT_WORKFLOW_POLICY = (
    "time_indexed_coarse_interval_simulation_path_for_validation_and_analytics.v1"
)
COARSE_SIMULATION_ENVIRONMENT_METADATA_POLICY = "exclude_environment_specific_metadata.v1"
COARSE_SIMULATION_INTERVAL_TIMING_POLICY = (
    "start_time_utc_anchor_plus_fixed_interval_delta_half_open_utc_boundaries.v1"
)
COARSE_SIMULATION_INTERVAL_IDENTIFIER_POLICY = "interval_index_and_explicit_utc_boundary_timestamps.v1"
COARSE_SIMULATION_INTERVAL_FACTOR_ALIGNMENT_POLICY = (
    "factor_row_zero_anchors_to_start_time_utc_and_rows_progress_in_request_interval_order_without_implicit_resampling.v1"
)
COARSE_SIMULATION_COMPATIBLE_INTERVAL_GRID_POLICY = (
    "interval_delta_must_map_to_an_exact_fixed_width_utc_nanosecond_grid.v1"
)
COARSE_SIMULATION_REQUIRED_PATH_FIELDS: tuple[str, ...] = (
    "schema_version",
    "run_request_schema_version",
    "output_schema_metadata",
    "provenance",
    "diagnostics",
    "seed",
    "start_time_utc_nanos",
    "end_time_utc_nanos",
    "interval_delta",
    "simulated_interval_count",
    "initial_excitation_state",
    "terminal_excitation_state",
    "records",
)
COARSE_SIMULATION_REQUIRED_RECORD_FIELDS: tuple[str, ...] = (
    "schema_version",
    "interval_index",
    "interval_start_utc_nanos",
    "interval_end_utc_nanos",
    "sufficient_statistics",
    "return_components",
)
COARSE_SIMULATION_REQUIRED_PROVENANCE_FIELDS: tuple[str, ...] = (
    "schema_version",
    "artifact_workflow_policy",
    "environment_metadata_policy",
    "interval_timing_policy",
    "interval_identifier_policy",
    "interval_factor_alignment_policy",
    "compatible_interval_grid_policy",
    "interval_factor_policy",
    "transform_slice_sampling_policy",
    "boundary_state_update_policy",
    "interval_return_generation_policy",
    "interval_return_component_routing_policy",
    "batch_seed_partition_policy",
)
COARSE_SIMULATION_REQUIRED_DIAGNOSTICS_FIELDS: tuple[str, ...] = (
    "schema_version",
    "simulated_interval_count",
    "first_interval_index",
    "last_interval_index",
    "first_interval_start_utc_nanos",
    "last_interval_end_utc_nanos",
    "cumulative_a_plus",
    "cumulative_a_minus",
    "cumulative_n_plus",
    "cumulative_n_minus",
    "cumulative_trading_time_return",
    "cumulative_open_jump_return",
    "cumulative_closed_gap_return",
    "cumulative_total_return",
    "max_abs_total_return",
)
_ONE_HOUR_NANOS = 3_600_000_000_000
_INTERVAL_DELTA_NANOS_TOLERANCE = 0.5
_SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY = (
    "explicit_interval_aligned_s_h_and_news_intensity_inputs_without_implicit_resampling.v1"
)
_SUFFICIENT_STATISTICS_TRANSFORM_SLICE_SAMPLING_POLICY = (
    "counts_via_fractional_rounding_and_integrated_intensity_via_exponential_with_slice_implied_mean.v1"
)
_SUFFICIENT_STATISTICS_BOUNDARY_STATE_UPDATE_POLICY = (
    "interval_boundary_drift_then_count_mapped_endogenous_jump_composition.v1"
)
_SUFFICIENT_STATISTICS_INTERVAL_RETURN_GENERATION_POLICY = (
    "sample_trading_time_increment_from_sampled_statistics_via_shared_return_decomposition.v1"
)
_SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_ROUTING_POLICY = (
    "open_market_state_routes_to_mt_closed_market_state_routes_to_gt.v1"
)
_SUFFICIENT_STATISTICS_BATCH_SEED_PARTITION_POLICY = (
    "path_seed_partitioned_from_root_seed_and_path_index_with_shared_substream_mixer.v1"
)


def _prefix_nested_field_path(
    field_path: str,
    *,
    prefix: str,
    strip_prefix: Optional[str] = None,
) -> str:
    normalized_field_path = field_path
    if strip_prefix is not None and normalized_field_path.startswith(strip_prefix):
        normalized_field_path = normalized_field_path.removeprefix(strip_prefix)

    if normalized_field_path == "":
        return prefix

    return f"{prefix}.{normalized_field_path}"


def _interval_delta_to_nanos(interval_delta: float, *, field_path: str) -> int:
    require_positive_finite_scalar(interval_delta, field_path, SIMULATION_ERROR_FACTORY)
    interval_delta_nanos = interval_delta * _ONE_HOUR_NANOS
    rounded_interval_delta_nanos = round(interval_delta_nanos)
    if (
        abs(interval_delta_nanos - rounded_interval_delta_nanos) > _INTERVAL_DELTA_NANOS_TOLERANCE
        or rounded_interval_delta_nanos <= 0
    ):
        raise _simulation_error(
            field_path,
            "must map to an exact fixed-width UTC nanosecond grid",
        )

    return int(rounded_interval_delta_nanos)


def _validate_calendar_registry_artifact_payload(
    calendar_registry_artifact: dict[str, Any] | Any,
    *,
    field_path: str,
) -> dict[str, Any]:
    if not isinstance(calendar_registry_artifact, dict):
        raise _simulation_error(field_path, "must be an object")

    try:
        CalendarRegistry(serialize_deterministic_json(calendar_registry_artifact))
    except CalendarRegistryLoadError as e:
        raise _simulation_error(field_path, str(e)) from e
    except ModuleNotFoundError as e:
        raise _simulation_integration_error(
            "native calendar module is unavailable",
            entrypoint="validate_coarse_simulation_run_request",
            error_id=SIMULATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
            field_path=field_path,
            context={"module_name": "janiform._native"},
        ) from e

    return cast(dict[str, Any], calendar_registry_artifact)


@dataclass(frozen=True, slots=True)
class DoubleExponentialBaseShockConfiguration(ContractModel):
    probability_positive: float
    eta_plus: float
    eta_minus: float

    def __post_init__(self) -> None:
        require_positive_finite_scalar(self.eta_plus, "eta_plus", SIMULATION_ERROR_FACTORY)
        require_positive_finite_scalar(self.eta_minus, "eta_minus", SIMULATION_ERROR_FACTORY)
        require_positive_finite_scalar(
            self.probability_positive,
            "probability_positive",
            SIMULATION_ERROR_FACTORY,
        )
        if self.probability_positive >= 1.0:
            raise _simulation_error("probability_positive", "must be strictly less than 1.0")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> DoubleExponentialBaseShockConfiguration:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        return DoubleExponentialBaseShockConfiguration(
            probability_positive=require_positive_finite_number(
                raw_payload,
                "probability_positive",
                f"{field_path}.probability_positive",
                SIMULATION_ERROR_FACTORY,
            ),
            eta_plus=require_positive_finite_number(
                raw_payload,
                "eta_plus",
                f"{field_path}.eta_plus",
                SIMULATION_ERROR_FACTORY,
            ),
            eta_minus=require_positive_finite_number(
                raw_payload,
                "eta_minus",
                f"{field_path}.eta_minus",
                SIMULATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CoupledGapMarkConfiguration(ContractModel):
    sigma_g: float

    def __post_init__(self) -> None:
        require_non_negative_finite_scalar(self.sigma_g, "sigma_g", SIMULATION_ERROR_FACTORY)

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CoupledGapMarkConfiguration:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        return CoupledGapMarkConfiguration(
            sigma_g=require_non_negative_finite_number(
                raw_payload,
                "sigma_g",
                f"{field_path}.sigma_g",
                SIMULATION_ERROR_FACTORY,
            )
        )


@dataclass(frozen=True, slots=True)
class SharedNewsMarkLawConfiguration(ContractModel):
    gamma_severity_marks: Any
    double_exponential_base_shock: DoubleExponentialBaseShockConfiguration
    coupled_gap_marks: CoupledGapMarkConfiguration

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> SharedNewsMarkLawConfiguration:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        try:
            from .pricing import GammaSeverityMarkConfiguration, VanillaOptionPricingValidationError

            return SharedNewsMarkLawConfiguration(
                gamma_severity_marks=GammaSeverityMarkConfiguration.from_mapping(
                    require_mapping(
                        raw_payload,
                        "gamma_severity_marks",
                        f"{field_path}.gamma_severity_marks",
                        SIMULATION_ERROR_FACTORY,
                    ),
                    field_path=f"{field_path}.gamma_severity_marks",
                ),
                double_exponential_base_shock=DoubleExponentialBaseShockConfiguration.from_mapping(
                    require_mapping(
                        raw_payload,
                        "double_exponential_base_shock",
                        f"{field_path}.double_exponential_base_shock",
                        SIMULATION_ERROR_FACTORY,
                    ),
                    field_path=f"{field_path}.double_exponential_base_shock",
                ),
                coupled_gap_marks=CoupledGapMarkConfiguration.from_mapping(
                    require_mapping(
                        raw_payload,
                        "coupled_gap_marks",
                        f"{field_path}.coupled_gap_marks",
                        SIMULATION_ERROR_FACTORY,
                    ),
                    field_path=f"{field_path}.coupled_gap_marks",
                ),
            )
        except VanillaOptionPricingValidationError as e:
            raise _simulation_error(e.failure.field_path, e.failure.message) from e


@dataclass(frozen=True, slots=True)
class KnownNewsArrivalConfiguration(ContractModel):
    scheduled_event_timestamps_utc_nanos: tuple[int, ...]

    def __post_init__(self) -> None:
        previous_timestamp_utc_nanos: Optional[int] = None
        for timestamp_index, timestamp_utc_nanos in enumerate(self.scheduled_event_timestamps_utc_nanos):
            if previous_timestamp_utc_nanos is not None and timestamp_utc_nanos <= previous_timestamp_utc_nanos:
                raise _simulation_error(
                    f"scheduled_event_timestamps_utc_nanos[{timestamp_index}]",
                    "must be strictly greater than the previous timestamp",
                )

            previous_timestamp_utc_nanos = timestamp_utc_nanos

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> KnownNewsArrivalConfiguration:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        scheduled_event_timestamps_utc_nanos = require_sequence(
            raw_payload,
            "scheduled_event_timestamps_utc_nanos",
            f"{field_path}.scheduled_event_timestamps_utc_nanos",
            SIMULATION_ERROR_FACTORY,
        )

        return KnownNewsArrivalConfiguration(
            scheduled_event_timestamps_utc_nanos=tuple(
                require_non_negative_integer(
                    {"value": raw_timestamp_utc_nanos},
                    "value",
                    f"{field_path}.scheduled_event_timestamps_utc_nanos[{timestamp_index}]",
                    SIMULATION_ERROR_FACTORY,
                )
                for timestamp_index, raw_timestamp_utc_nanos in enumerate(scheduled_event_timestamps_utc_nanos)
            )
        )


@dataclass(frozen=True, slots=True)
class SurpriseNewsArrivalConfiguration(ContractModel):
    baseline_intensity: float

    def __post_init__(self) -> None:
        require_non_negative_finite_scalar(
            self.baseline_intensity,
            "baseline_intensity",
            SIMULATION_ERROR_FACTORY,
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> SurpriseNewsArrivalConfiguration:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        return SurpriseNewsArrivalConfiguration(
            baseline_intensity=require_non_negative_finite_number(
                raw_payload,
                "baseline_intensity",
                f"{field_path}.baseline_intensity",
                SIMULATION_ERROR_FACTORY,
            )
        )


@dataclass(frozen=True, slots=True)
class NewsArrivalConfiguration(ContractModel):
    known: KnownNewsArrivalConfiguration
    surprise: SurpriseNewsArrivalConfiguration

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> NewsArrivalConfiguration:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        return NewsArrivalConfiguration(
            known=KnownNewsArrivalConfiguration.from_mapping(
                require_mapping(raw_payload, "known", f"{field_path}.known", SIMULATION_ERROR_FACTORY),
                field_path=f"{field_path}.known",
            ),
            surprise=SurpriseNewsArrivalConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "surprise",
                    f"{field_path}.surprise",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.surprise",
            ),
        )


@dataclass(frozen=True, slots=True)
class NewsComponentMarkScaleConfiguration(ContractModel):
    activity_coupling_scale: float
    return_jump_scale: float

    def __post_init__(self) -> None:
        require_non_negative_finite_scalar(
            self.activity_coupling_scale,
            "activity_coupling_scale",
            SIMULATION_ERROR_FACTORY,
        )
        require_non_negative_finite_scalar(
            self.return_jump_scale,
            "return_jump_scale",
            SIMULATION_ERROR_FACTORY,
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> NewsComponentMarkScaleConfiguration:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        return NewsComponentMarkScaleConfiguration(
            activity_coupling_scale=require_non_negative_finite_number(
                raw_payload,
                "activity_coupling_scale",
                f"{field_path}.activity_coupling_scale",
                SIMULATION_ERROR_FACTORY,
            ),
            return_jump_scale=require_non_negative_finite_number(
                raw_payload,
                "return_jump_scale",
                f"{field_path}.return_jump_scale",
                SIMULATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class NewsProcessConfiguration(ContractModel):
    schema_version: str
    arrivals: NewsArrivalConfiguration
    shared_mark_law: SharedNewsMarkLawConfiguration
    known_component_mark_scales: NewsComponentMarkScaleConfiguration
    surprise_component_mark_scales: NewsComponentMarkScaleConfiguration

    def __post_init__(self) -> None:
        if self.schema_version != NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION:
            raise _simulation_error(
                "schema_version",
                f"must be {NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION}, found {self.schema_version}",
            )

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any) -> NewsProcessConfiguration:
        if not isinstance(raw_payload, dict):
            raise _simulation_error("news_process_configuration", "must be an object")

        return NewsProcessConfiguration(
            schema_version=require_string(
                raw_payload,
                "schema_version",
                "news_process_configuration.schema_version",
                SIMULATION_ERROR_FACTORY,
            ),
            arrivals=NewsArrivalConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "arrivals",
                    "news_process_configuration.arrivals",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path="news_process_configuration.arrivals",
            ),
            shared_mark_law=SharedNewsMarkLawConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "shared_mark_law",
                    "news_process_configuration.shared_mark_law",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path="news_process_configuration.shared_mark_law",
            ),
            known_component_mark_scales=NewsComponentMarkScaleConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "known_component_mark_scales",
                    "news_process_configuration.known_component_mark_scales",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path="news_process_configuration.known_component_mark_scales",
            ),
            surprise_component_mark_scales=NewsComponentMarkScaleConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "surprise_component_mark_scales",
                    "news_process_configuration.surprise_component_mark_scales",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path="news_process_configuration.surprise_component_mark_scales",
            ),
        )


@dataclass(frozen=True, slots=True)
class CoarseSimulationModelInputs(ContractModel):
    risk_model_inputs: Any
    news_process_configuration: NewsProcessConfiguration
    calendar_registry_artifact: dict[str, Any]
    exchange_code: str

    def __post_init__(self) -> None:
        if not self.exchange_code.strip():
            raise _simulation_error("exchange_code", "must be a non-empty string")

        _validate_calendar_registry_artifact_payload(
            self.calendar_registry_artifact,
            field_path="calendar_registry_artifact",
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "model_inputs",
    ) -> CoarseSimulationModelInputs:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        from .risk import SufficientStatisticsRiskModelInputs, SufficientStatisticsRiskValidationError

        try:

            risk_model_inputs = SufficientStatisticsRiskModelInputs.from_mapping(
                require_mapping(
                    raw_payload,
                    "risk_model_inputs",
                    f"{field_path}.risk_model_inputs",
                    SIMULATION_ERROR_FACTORY,
                )
            )
        except SufficientStatisticsRiskValidationError as e:
            raise _simulation_error(
                _prefix_nested_field_path(
                    e.failure.field_path,
                    prefix=f"{field_path}.risk_model_inputs",
                    strip_prefix="model_inputs.",
                ),
                e.failure.message,
            ) from e

        return CoarseSimulationModelInputs(
            risk_model_inputs=risk_model_inputs,
            news_process_configuration=NewsProcessConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "news_process_configuration",
                    f"{field_path}.news_process_configuration",
                    SIMULATION_ERROR_FACTORY,
                )
            ),
            calendar_registry_artifact=_validate_calendar_registry_artifact_payload(
                require_mapping(
                    raw_payload,
                    "calendar_registry_artifact",
                    f"{field_path}.calendar_registry_artifact",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.calendar_registry_artifact",
            ),
            exchange_code=require_non_empty_string(
                raw_payload,
                "exchange_code",
                f"{field_path}.exchange_code",
                SIMULATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CoarseSimulationRunRequest(ContractModel):
    schema_version: str
    seed: int
    start_time_utc_nanos: int
    horizon_interval_count: int
    interval_delta: float
    model_inputs: CoarseSimulationModelInputs
    interval_factors: Any

    def __post_init__(self) -> None:
        if self.schema_version != COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION:
            raise _simulation_error(
                "schema_version",
                (
                    f"must be {COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION}, "
                    f"found {self.schema_version}"
                ),
            )

        if self.seed < 0:
            raise _simulation_error("seed", "must be non-negative")

        if self.horizon_interval_count <= 0:
            raise _simulation_error("horizon_interval_count", "must be strictly positive")

        _interval_delta_to_nanos(self.interval_delta, field_path="interval_delta")

        if self.interval_factors.interval_delta != self.interval_delta:
            raise _simulation_error(
                "interval_factors.interval_delta",
                (
                    "must match interval_delta exactly under explicit factor alignment without "
                    f"implicit resampling (interval_delta={self.interval_delta}, "
                    f"interval_factors.interval_delta={self.interval_factors.interval_delta})"
                ),
            )

        if self.horizon_interval_count > len(self.interval_factors.s_h):
            raise _simulation_error(
                "horizon_interval_count",
                f"must not exceed available interval factors count ({len(self.interval_factors.s_h)})",
            )

    @staticmethod
    def from_runtime_inputs(
        *,
        seed: int,
        start_time_utc: datetime,
        horizon_interval_count: int,
        interval_delta: float,
        risk_model_inputs: Any,
        interval_factors: Any,
        news_process_configuration: NewsProcessConfiguration,
        calendar_registry: CalendarRegistry,
        exchange_code: str,
    ) -> CoarseSimulationRunRequest:
        normalized_start_time_utc = require_datetime_utc(
            start_time_utc,
            "start_time_utc",
            SIMULATION_ERROR_FACTORY,
        )

        return CoarseSimulationRunRequest(
            schema_version=COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION,
            seed=seed,
            start_time_utc_nanos=datetime_to_utc_nanos(normalized_start_time_utc),
            horizon_interval_count=horizon_interval_count,
            interval_delta=interval_delta,
            model_inputs=CoarseSimulationModelInputs(
                risk_model_inputs=risk_model_inputs,
                news_process_configuration=news_process_configuration,
                calendar_registry_artifact=dict(calendar_registry._registry_payload),
                exchange_code=exchange_code,
            ),
            interval_factors=interval_factors,
        )

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any) -> CoarseSimulationRunRequest:
        if not isinstance(raw_payload, dict):
            raise _simulation_error("coarse_simulation_run_request", "must be an object")

        interval_delta = require_positive_finite_number(
            raw_payload,
            "interval_delta",
            "interval_delta",
            SIMULATION_ERROR_FACTORY,
        )

        from .risk import SufficientStatisticsRiskIntervalFactors, SufficientStatisticsRiskValidationError

        try:

            interval_factors = SufficientStatisticsRiskIntervalFactors.from_mapping(
                require_mapping(raw_payload, "interval_factors", "interval_factors", SIMULATION_ERROR_FACTORY),
                field_path="interval_factors",
                requested_interval_delta=interval_delta,
            )
        except SufficientStatisticsRiskValidationError as e:
            raise _simulation_error(e.failure.field_path, e.failure.message) from e

        return CoarseSimulationRunRequest(
            schema_version=require_string(
                raw_payload,
                "schema_version",
                "schema_version",
                SIMULATION_ERROR_FACTORY,
            ),
            seed=require_non_negative_integer(raw_payload, "seed", "seed", SIMULATION_ERROR_FACTORY),
            start_time_utc_nanos=require_non_negative_integer(
                raw_payload,
                "start_time_utc_nanos",
                "start_time_utc_nanos",
                SIMULATION_ERROR_FACTORY,
            ),
            horizon_interval_count=require_positive_integer(
                raw_payload,
                "horizon_interval_count",
                "horizon_interval_count",
                SIMULATION_ERROR_FACTORY,
            ),
            interval_delta=interval_delta,
            model_inputs=CoarseSimulationModelInputs.from_mapping(
                require_mapping(raw_payload, "model_inputs", "model_inputs", SIMULATION_ERROR_FACTORY)
            ),
            interval_factors=interval_factors,
        )


@dataclass(frozen=True, slots=True)
class CoarseSimulationPathRecord(ContractModel):
    schema_version: str
    interval_index: int
    interval_start_utc_nanos: int
    interval_end_utc_nanos: int
    sufficient_statistics: Any
    return_components: Any

    def __post_init__(self) -> None:
        if self.schema_version != COARSE_SIMULATION_PATH_RECORD_SCHEMA_VERSION:
            raise _simulation_error(
                "schema_version",
                f"must be {COARSE_SIMULATION_PATH_RECORD_SCHEMA_VERSION}, found {self.schema_version}",
            )

        if self.interval_end_utc_nanos <= self.interval_start_utc_nanos:
            raise _simulation_error(
                "interval_end_utc_nanos",
                "must be strictly greater than interval_start_utc_nanos",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
        expected_interval_index: int,
    ) -> CoarseSimulationPathRecord:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        interval_index = require_non_negative_integer(
            raw_payload,
            "interval_index",
            f"{field_path}.interval_index",
            SIMULATION_ERROR_FACTORY,
        )
        if interval_index != expected_interval_index:
            raise _simulation_error(
                f"{field_path}.interval_index",
                f"must equal {expected_interval_index}",
            )

        from .risk import (
            SufficientStatisticsIntervalStatistics,
            SufficientStatisticsRiskReturnComponents,
            SufficientStatisticsRiskValidationError,
        )

        try:

            sufficient_statistics = SufficientStatisticsIntervalStatistics.from_mapping(
                require_mapping(
                    raw_payload,
                    "sufficient_statistics",
                    f"{field_path}.sufficient_statistics",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.sufficient_statistics",
            )
            return_components = SufficientStatisticsRiskReturnComponents.from_mapping(
                require_mapping(
                    raw_payload,
                    "return_components",
                    f"{field_path}.return_components",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.return_components",
            )
        except SufficientStatisticsRiskValidationError as e:
            raise _simulation_error(e.failure.field_path, e.failure.message) from e

        return CoarseSimulationPathRecord(
            schema_version=require_string(
                raw_payload,
                "schema_version",
                f"{field_path}.schema_version",
                SIMULATION_ERROR_FACTORY,
            ),
            interval_index=interval_index,
            interval_start_utc_nanos=require_non_negative_integer(
                raw_payload,
                "interval_start_utc_nanos",
                f"{field_path}.interval_start_utc_nanos",
                SIMULATION_ERROR_FACTORY,
            ),
            interval_end_utc_nanos=require_non_negative_integer(
                raw_payload,
                "interval_end_utc_nanos",
                f"{field_path}.interval_end_utc_nanos",
                SIMULATION_ERROR_FACTORY,
            ),
            sufficient_statistics=sufficient_statistics,
            return_components=return_components,
        )


@dataclass(frozen=True, slots=True)
class CoarseSimulationPathOutputSchemaMetadata(ContractModel):
    schema_version: str
    provenance_schema_version: str
    diagnostics_schema_version: str
    artifact_workflow_policy: str
    environment_metadata_policy: str
    required_path_fields: tuple[str, ...]
    required_record_fields: tuple[str, ...]
    required_provenance_fields: tuple[str, ...]
    required_diagnostics_fields: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class CoarseSimulationPathProvenance(ContractModel):
    schema_version: str
    artifact_workflow_policy: str
    environment_metadata_policy: str
    interval_timing_policy: str
    interval_identifier_policy: str
    interval_factor_alignment_policy: str
    compatible_interval_grid_policy: str
    interval_factor_policy: str
    transform_slice_sampling_policy: str
    boundary_state_update_policy: str
    interval_return_generation_policy: str
    interval_return_component_routing_policy: str
    batch_seed_partition_policy: str


@dataclass(frozen=True, slots=True)
class CoarseSimulationPathDiagnostics(ContractModel):
    schema_version: str
    simulated_interval_count: int
    first_interval_index: int
    last_interval_index: int
    first_interval_start_utc_nanos: int
    last_interval_end_utc_nanos: int
    cumulative_a_plus: float
    cumulative_a_minus: float
    cumulative_n_plus: int
    cumulative_n_minus: int
    cumulative_trading_time_return: float
    cumulative_open_jump_return: float
    cumulative_closed_gap_return: float
    cumulative_total_return: float
    max_abs_total_return: float


@dataclass(frozen=True, slots=True)
class CoarseSimulationPath(ContractModel):
    schema_version: str
    run_request_schema_version: str
    output_schema_metadata: CoarseSimulationPathOutputSchemaMetadata
    provenance: CoarseSimulationPathProvenance
    diagnostics: CoarseSimulationPathDiagnostics
    seed: int
    start_time_utc_nanos: int
    end_time_utc_nanos: int
    interval_delta: float
    simulated_interval_count: int
    initial_excitation_state: HawkesExcitationState
    terminal_excitation_state: HawkesExcitationState
    records: tuple[CoarseSimulationPathRecord, ...]

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any) -> CoarseSimulationPath:
        if not isinstance(raw_payload, dict):
            raise _simulation_error("coarse_simulation_path", "must be an object")

        seed = require_non_negative_integer(raw_payload, "seed", "seed", SIMULATION_ERROR_FACTORY)
        start_time_utc_nanos = require_non_negative_integer(
            raw_payload,
            "start_time_utc_nanos",
            "start_time_utc_nanos",
            SIMULATION_ERROR_FACTORY,
        )
        interval_delta = require_positive_finite_number(
            raw_payload,
            "interval_delta",
            "interval_delta",
            SIMULATION_ERROR_FACTORY,
        )
        interval_delta_nanos = _interval_delta_to_nanos(interval_delta, field_path="interval_delta")
        simulated_interval_count = require_positive_integer(
            raw_payload,
            "simulated_interval_count",
            "simulated_interval_count",
            SIMULATION_ERROR_FACTORY,
        )

        records_payload = require_sequence(raw_payload, "records", "records", SIMULATION_ERROR_FACTORY)
        if len(records_payload) != simulated_interval_count:
            raise _simulation_error("records", f"must contain exactly {simulated_interval_count} records")

        records = tuple(
            CoarseSimulationPathRecord.from_mapping(
                raw_record,
                field_path=f"records[{record_position}]",
                expected_interval_index=record_position,
            )
            for record_position, raw_record in enumerate(records_payload)
        )

        if records[0].interval_start_utc_nanos != start_time_utc_nanos:
            raise _simulation_error(
                "start_time_utc_nanos",
                f"must equal {records[0].interval_start_utc_nanos}, found {start_time_utc_nanos}",
            )

        expected_end_time_utc_nanos = start_time_utc_nanos + simulated_interval_count * interval_delta_nanos
        provided_end_time_utc_nanos = require_non_negative_integer(
            raw_payload,
            "end_time_utc_nanos",
            "end_time_utc_nanos",
            SIMULATION_ERROR_FACTORY,
        )
        if provided_end_time_utc_nanos != expected_end_time_utc_nanos:
            raise _simulation_error(
                "end_time_utc_nanos",
                f"must be {expected_end_time_utc_nanos}, found {provided_end_time_utc_nanos}",
            )
        if records[-1].interval_end_utc_nanos != expected_end_time_utc_nanos:
            raise _simulation_error(
                "records[-1].interval_end_utc_nanos",
                f"must equal {expected_end_time_utc_nanos}",
            )

        output_schema_metadata = _build_coarse_simulation_path_output_schema_metadata()
        provenance = _build_coarse_simulation_path_provenance()
        diagnostics = _build_coarse_simulation_path_diagnostics(
            records=records,
            simulated_interval_count=simulated_interval_count,
        )

        validate_exact_string(
            raw_payload,
            "schema_version",
            "schema_version",
            COARSE_SIMULATION_PATH_SCHEMA_VERSION,
            SIMULATION_ERROR_FACTORY,
        )
        validate_exact_string(
            raw_payload,
            "run_request_schema_version",
            "run_request_schema_version",
            COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION,
            SIMULATION_ERROR_FACTORY,
        )
        _validate_coarse_path_output_schema_metadata(
            require_mapping(
                raw_payload,
                "output_schema_metadata",
                "output_schema_metadata",
                SIMULATION_ERROR_FACTORY,
            ),
            expected_metadata=output_schema_metadata,
        )
        _validate_coarse_path_provenance(
            require_mapping(raw_payload, "provenance", "provenance", SIMULATION_ERROR_FACTORY),
            expected_provenance=provenance,
        )
        _validate_coarse_path_diagnostics(
            require_mapping(raw_payload, "diagnostics", "diagnostics", SIMULATION_ERROR_FACTORY),
            expected_diagnostics=diagnostics,
            simulated_interval_count=simulated_interval_count,
        )

        try:
            return CoarseSimulationPath(
                schema_version=COARSE_SIMULATION_PATH_SCHEMA_VERSION,
                run_request_schema_version=COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION,
                output_schema_metadata=output_schema_metadata,
                provenance=provenance,
                diagnostics=diagnostics,
                seed=seed,
                start_time_utc_nanos=start_time_utc_nanos,
                end_time_utc_nanos=provided_end_time_utc_nanos,
                interval_delta=interval_delta,
                simulated_interval_count=simulated_interval_count,
                initial_excitation_state=HawkesExcitationState.from_mapping(
                    require_mapping(
                        raw_payload,
                        "initial_excitation_state",
                        "initial_excitation_state",
                        SIMULATION_ERROR_FACTORY,
                    ),
                    field_path="initial_excitation_state",
                ),
                terminal_excitation_state=HawkesExcitationState.from_mapping(
                    require_mapping(
                        raw_payload,
                        "terminal_excitation_state",
                        "terminal_excitation_state",
                        SIMULATION_ERROR_FACTORY,
                    ),
                    field_path="terminal_excitation_state",
                ),
                records=records,
            )
        except SeededTickSimulationValidationError as e:
            raise _simulation_error(e.failure.field_path, e.failure.message) from e


def _build_coarse_simulation_path_output_schema_metadata() -> CoarseSimulationPathOutputSchemaMetadata:
    return CoarseSimulationPathOutputSchemaMetadata(
        schema_version=COARSE_SIMULATION_PATH_OUTPUT_SCHEMA_METADATA_VERSION,
        provenance_schema_version=COARSE_SIMULATION_PATH_PROVENANCE_SCHEMA_VERSION,
        diagnostics_schema_version=COARSE_SIMULATION_PATH_DIAGNOSTICS_SCHEMA_VERSION,
        artifact_workflow_policy=COARSE_SIMULATION_ARTIFACT_WORKFLOW_POLICY,
        environment_metadata_policy=COARSE_SIMULATION_ENVIRONMENT_METADATA_POLICY,
        required_path_fields=COARSE_SIMULATION_REQUIRED_PATH_FIELDS,
        required_record_fields=COARSE_SIMULATION_REQUIRED_RECORD_FIELDS,
        required_provenance_fields=COARSE_SIMULATION_REQUIRED_PROVENANCE_FIELDS,
        required_diagnostics_fields=COARSE_SIMULATION_REQUIRED_DIAGNOSTICS_FIELDS,
    )


def _build_coarse_simulation_path_provenance() -> CoarseSimulationPathProvenance:
    return CoarseSimulationPathProvenance(
        schema_version=COARSE_SIMULATION_PATH_PROVENANCE_SCHEMA_VERSION,
        artifact_workflow_policy=COARSE_SIMULATION_ARTIFACT_WORKFLOW_POLICY,
        environment_metadata_policy=COARSE_SIMULATION_ENVIRONMENT_METADATA_POLICY,
        interval_timing_policy=COARSE_SIMULATION_INTERVAL_TIMING_POLICY,
        interval_identifier_policy=COARSE_SIMULATION_INTERVAL_IDENTIFIER_POLICY,
        interval_factor_alignment_policy=COARSE_SIMULATION_INTERVAL_FACTOR_ALIGNMENT_POLICY,
        compatible_interval_grid_policy=COARSE_SIMULATION_COMPATIBLE_INTERVAL_GRID_POLICY,
        interval_factor_policy=_SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY,
        transform_slice_sampling_policy=_SUFFICIENT_STATISTICS_TRANSFORM_SLICE_SAMPLING_POLICY,
        boundary_state_update_policy=_SUFFICIENT_STATISTICS_BOUNDARY_STATE_UPDATE_POLICY,
        interval_return_generation_policy=_SUFFICIENT_STATISTICS_INTERVAL_RETURN_GENERATION_POLICY,
        interval_return_component_routing_policy=_SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_ROUTING_POLICY,
        batch_seed_partition_policy=_SUFFICIENT_STATISTICS_BATCH_SEED_PARTITION_POLICY,
    )


def _build_coarse_simulation_path_diagnostics(
    *,
    records: tuple[CoarseSimulationPathRecord, ...],
    simulated_interval_count: int,
) -> CoarseSimulationPathDiagnostics:
    cumulative_a_plus = 0.0
    cumulative_a_minus = 0.0
    cumulative_n_plus = 0
    cumulative_n_minus = 0
    cumulative_trading_time_return = 0.0
    cumulative_open_jump_return = 0.0
    cumulative_closed_gap_return = 0.0
    cumulative_total_return = 0.0
    max_abs_total_return = 0.0

    for record in records:
        cumulative_a_plus += record.sufficient_statistics.a_plus
        cumulative_a_minus += record.sufficient_statistics.a_minus
        cumulative_n_plus += record.sufficient_statistics.n_plus
        cumulative_n_minus += record.sufficient_statistics.n_minus
        cumulative_trading_time_return += record.return_components.trading_time_return
        cumulative_open_jump_return += record.return_components.open_jump_return
        cumulative_closed_gap_return += record.return_components.closed_gap_return
        cumulative_total_return += record.return_components.total_return
        max_abs_total_return = max(max_abs_total_return, abs(record.return_components.total_return))

    return CoarseSimulationPathDiagnostics(
        schema_version=COARSE_SIMULATION_PATH_DIAGNOSTICS_SCHEMA_VERSION,
        simulated_interval_count=simulated_interval_count,
        first_interval_index=records[0].interval_index,
        last_interval_index=records[-1].interval_index,
        first_interval_start_utc_nanos=records[0].interval_start_utc_nanos,
        last_interval_end_utc_nanos=records[-1].interval_end_utc_nanos,
        cumulative_a_plus=cumulative_a_plus,
        cumulative_a_minus=cumulative_a_minus,
        cumulative_n_plus=cumulative_n_plus,
        cumulative_n_minus=cumulative_n_minus,
        cumulative_trading_time_return=cumulative_trading_time_return,
        cumulative_open_jump_return=cumulative_open_jump_return,
        cumulative_closed_gap_return=cumulative_closed_gap_return,
        cumulative_total_return=cumulative_total_return,
        max_abs_total_return=max_abs_total_return,
    )


def _validate_coarse_path_output_schema_metadata(
    raw_payload: dict[str, Any] | Any,
    *,
    expected_metadata: CoarseSimulationPathOutputSchemaMetadata,
) -> None:
    validate_exact_string(
        raw_payload,
        "schema_version",
        "output_schema_metadata.schema_version",
        expected_metadata.schema_version,
        SIMULATION_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "provenance_schema_version",
        "output_schema_metadata.provenance_schema_version",
        expected_metadata.provenance_schema_version,
        SIMULATION_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "diagnostics_schema_version",
        "output_schema_metadata.diagnostics_schema_version",
        expected_metadata.diagnostics_schema_version,
        SIMULATION_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "artifact_workflow_policy",
        "output_schema_metadata.artifact_workflow_policy",
        expected_metadata.artifact_workflow_policy,
        SIMULATION_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "environment_metadata_policy",
        "output_schema_metadata.environment_metadata_policy",
        expected_metadata.environment_metadata_policy,
        SIMULATION_ERROR_FACTORY,
    )
    validate_required_field_list(
        raw_payload,
        "required_path_fields",
        "output_schema_metadata.required_path_fields",
        expected_metadata.required_path_fields,
        SIMULATION_ERROR_FACTORY,
    )
    validate_required_field_list(
        raw_payload,
        "required_record_fields",
        "output_schema_metadata.required_record_fields",
        expected_metadata.required_record_fields,
        SIMULATION_ERROR_FACTORY,
    )
    validate_required_field_list(
        raw_payload,
        "required_provenance_fields",
        "output_schema_metadata.required_provenance_fields",
        expected_metadata.required_provenance_fields,
        SIMULATION_ERROR_FACTORY,
    )
    validate_required_field_list(
        raw_payload,
        "required_diagnostics_fields",
        "output_schema_metadata.required_diagnostics_fields",
        expected_metadata.required_diagnostics_fields,
        SIMULATION_ERROR_FACTORY,
    )


def _validate_coarse_path_provenance(
    raw_payload: dict[str, Any] | Any,
    *,
    expected_provenance: CoarseSimulationPathProvenance,
) -> None:
    validate_exact_string(
        raw_payload,
        "schema_version",
        "provenance.schema_version",
        expected_provenance.schema_version,
        SIMULATION_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "artifact_workflow_policy",
        "provenance.artifact_workflow_policy",
        expected_provenance.artifact_workflow_policy,
        SIMULATION_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "environment_metadata_policy",
        "provenance.environment_metadata_policy",
        expected_provenance.environment_metadata_policy,
        SIMULATION_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "interval_timing_policy",
        "provenance.interval_timing_policy",
        expected_provenance.interval_timing_policy,
        SIMULATION_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "interval_identifier_policy",
        "provenance.interval_identifier_policy",
        expected_provenance.interval_identifier_policy,
        SIMULATION_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "interval_factor_alignment_policy",
        "provenance.interval_factor_alignment_policy",
        expected_provenance.interval_factor_alignment_policy,
        SIMULATION_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "compatible_interval_grid_policy",
        "provenance.compatible_interval_grid_policy",
        expected_provenance.compatible_interval_grid_policy,
        SIMULATION_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "interval_factor_policy",
        "provenance.interval_factor_policy",
        expected_provenance.interval_factor_policy,
        SIMULATION_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "transform_slice_sampling_policy",
        "provenance.transform_slice_sampling_policy",
        expected_provenance.transform_slice_sampling_policy,
        SIMULATION_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "boundary_state_update_policy",
        "provenance.boundary_state_update_policy",
        expected_provenance.boundary_state_update_policy,
        SIMULATION_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "interval_return_generation_policy",
        "provenance.interval_return_generation_policy",
        expected_provenance.interval_return_generation_policy,
        SIMULATION_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "interval_return_component_routing_policy",
        "provenance.interval_return_component_routing_policy",
        expected_provenance.interval_return_component_routing_policy,
        SIMULATION_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "batch_seed_partition_policy",
        "provenance.batch_seed_partition_policy",
        expected_provenance.batch_seed_partition_policy,
        SIMULATION_ERROR_FACTORY,
    )


def _validate_coarse_path_diagnostics(
    raw_payload: dict[str, Any] | Any,
    *,
    expected_diagnostics: CoarseSimulationPathDiagnostics,
    simulated_interval_count: int,
) -> None:
    validate_exact_string(
        raw_payload,
        "schema_version",
        "diagnostics.schema_version",
        expected_diagnostics.schema_version,
        SIMULATION_ERROR_FACTORY,
    )
    validate_exact_integer(
        raw_payload,
        "simulated_interval_count",
        "diagnostics.simulated_interval_count",
        expected_diagnostics.simulated_interval_count,
        SIMULATION_ERROR_FACTORY,
    )
    validate_exact_integer(
        raw_payload,
        "first_interval_index",
        "diagnostics.first_interval_index",
        expected_diagnostics.first_interval_index,
        SIMULATION_ERROR_FACTORY,
    )
    validate_exact_integer(
        raw_payload,
        "last_interval_index",
        "diagnostics.last_interval_index",
        expected_diagnostics.last_interval_index,
        SIMULATION_ERROR_FACTORY,
    )
    validate_exact_integer(
        raw_payload,
        "first_interval_start_utc_nanos",
        "diagnostics.first_interval_start_utc_nanos",
        expected_diagnostics.first_interval_start_utc_nanos,
        SIMULATION_ERROR_FACTORY,
    )
    validate_exact_integer(
        raw_payload,
        "last_interval_end_utc_nanos",
        "diagnostics.last_interval_end_utc_nanos",
        expected_diagnostics.last_interval_end_utc_nanos,
        SIMULATION_ERROR_FACTORY,
    )
    validate_exact_number(
        raw_payload,
        "cumulative_a_plus",
        "diagnostics.cumulative_a_plus",
        expected_diagnostics.cumulative_a_plus,
        SIMULATION_ERROR_FACTORY,
        context_suffix=f"simulated_interval_count={simulated_interval_count}",
    )
    validate_exact_number(
        raw_payload,
        "cumulative_a_minus",
        "diagnostics.cumulative_a_minus",
        expected_diagnostics.cumulative_a_minus,
        SIMULATION_ERROR_FACTORY,
        context_suffix=f"simulated_interval_count={simulated_interval_count}",
    )
    validate_exact_integer(
        raw_payload,
        "cumulative_n_plus",
        "diagnostics.cumulative_n_plus",
        expected_diagnostics.cumulative_n_plus,
        SIMULATION_ERROR_FACTORY,
        context_suffix=f"simulated_interval_count={simulated_interval_count}",
    )
    validate_exact_integer(
        raw_payload,
        "cumulative_n_minus",
        "diagnostics.cumulative_n_minus",
        expected_diagnostics.cumulative_n_minus,
        SIMULATION_ERROR_FACTORY,
        context_suffix=f"simulated_interval_count={simulated_interval_count}",
    )
    validate_exact_number(
        raw_payload,
        "cumulative_trading_time_return",
        "diagnostics.cumulative_trading_time_return",
        expected_diagnostics.cumulative_trading_time_return,
        SIMULATION_ERROR_FACTORY,
        context_suffix=f"simulated_interval_count={simulated_interval_count}",
    )
    validate_exact_number(
        raw_payload,
        "cumulative_open_jump_return",
        "diagnostics.cumulative_open_jump_return",
        expected_diagnostics.cumulative_open_jump_return,
        SIMULATION_ERROR_FACTORY,
        context_suffix=f"simulated_interval_count={simulated_interval_count}",
    )
    validate_exact_number(
        raw_payload,
        "cumulative_closed_gap_return",
        "diagnostics.cumulative_closed_gap_return",
        expected_diagnostics.cumulative_closed_gap_return,
        SIMULATION_ERROR_FACTORY,
        context_suffix=f"simulated_interval_count={simulated_interval_count}",
    )
    validate_exact_number(
        raw_payload,
        "cumulative_total_return",
        "diagnostics.cumulative_total_return",
        expected_diagnostics.cumulative_total_return,
        SIMULATION_ERROR_FACTORY,
        context_suffix=f"simulated_interval_count={simulated_interval_count}",
    )
    validate_exact_number(
        raw_payload,
        "max_abs_total_return",
        "diagnostics.max_abs_total_return",
        expected_diagnostics.max_abs_total_return,
        SIMULATION_ERROR_FACTORY,
        context_suffix=f"simulated_interval_count={simulated_interval_count}",
    )


def validate_coarse_simulation_run_request(
    raw_request: dict[str, Any] | Any,
) -> CoarseSimulationRunRequest:
    return CoarseSimulationRunRequest.from_mapping(raw_request)


def validate_coarse_simulation_path(
    raw_path: dict[str, Any] | Any,
) -> CoarseSimulationPath:
    return CoarseSimulationPath.from_mapping(raw_path)


def serialize_coarse_simulation_run_request(request: CoarseSimulationRunRequest) -> str:
    return serialize_deterministic_json(request)


def deserialize_coarse_simulation_run_request(payload_json: str) -> CoarseSimulationRunRequest:
    raw_payload = parse_json_object(payload_json, "coarse simulation run request", SIMULATION_ERROR_FACTORY)
    return validate_coarse_simulation_run_request(dict(raw_payload))


def serialize_coarse_simulation_path(path: CoarseSimulationPath) -> str:
    return serialize_deterministic_json(path)


def deserialize_coarse_simulation_path(payload_json: str) -> CoarseSimulationPath:
    raw_payload = parse_json_object(payload_json, "coarse simulation path", SIMULATION_ERROR_FACTORY)
    return validate_coarse_simulation_path(dict(raw_payload))


def run_coarse_simulation(request: CoarseSimulationRunRequest) -> CoarseSimulationPath:
    if not isinstance(request, CoarseSimulationRunRequest):
        raise _simulation_error("request", "must be a CoarseSimulationRunRequest")

    normalized_request = validate_coarse_simulation_run_request(request.to_payload())

    try:
        native = native_module()
    except ModuleNotFoundError as e:
        raise _simulation_integration_error(
            "native simulation module is unavailable",
            entrypoint="run_coarse_simulation",
            error_id=SIMULATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
            context={"module_name": "janiform._native"},
        ) from e

    try:
        path_json: str = native.run_coarse_simulation(
            serialize_coarse_simulation_run_request(normalized_request)
        )
    except native.SimulationExecutionError as e:
        raise _simulation_execution_error(
            "coarse simulation failed during native execution",
            entrypoint="run_coarse_simulation",
            context={"source_exception_type": type(e).__name__},
        ) from e
    except Exception as e:
        raise _simulation_execution_error(
            "coarse simulation failed during native execution",
            entrypoint="run_coarse_simulation",
            context={"source_exception_type": type(e).__name__},
        ) from e

    try:
        return deserialize_coarse_simulation_path(path_json)
    except SeededTickSimulationValidationError as e:
        raise _simulation_integration_error(
            "native coarse simulation returned an invalid response payload",
            entrypoint="run_coarse_simulation",
            error_id=SIMULATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"source_error_id": e.error_id},
        ) from e


__all__ = [
    "COARSE_SIMULATION_ARTIFACT_WORKFLOW_POLICY",
    "COARSE_SIMULATION_COMPATIBLE_INTERVAL_GRID_POLICY",
    "COARSE_SIMULATION_ENVIRONMENT_METADATA_POLICY",
    "COARSE_SIMULATION_INTERVAL_FACTOR_ALIGNMENT_POLICY",
    "COARSE_SIMULATION_INTERVAL_IDENTIFIER_POLICY",
    "COARSE_SIMULATION_INTERVAL_TIMING_POLICY",
    "COARSE_SIMULATION_PATH_DIAGNOSTICS_SCHEMA_VERSION",
    "COARSE_SIMULATION_PATH_OUTPUT_SCHEMA_METADATA_VERSION",
    "COARSE_SIMULATION_PATH_PROVENANCE_SCHEMA_VERSION",
    "COARSE_SIMULATION_PATH_RECORD_SCHEMA_VERSION",
    "COARSE_SIMULATION_PATH_SCHEMA_VERSION",
    "COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION",
    "CoarseSimulationModelInputs",
    "CoarseSimulationPath",
    "CoarseSimulationPathDiagnostics",
    "CoarseSimulationPathOutputSchemaMetadata",
    "CoarseSimulationPathProvenance",
    "CoarseSimulationPathRecord",
    "CoarseSimulationRunRequest",
    "CoupledGapMarkConfiguration",
    "DoubleExponentialBaseShockConfiguration",
    "HawkesExcitationState",
    "HawkesKernelLoadings",
    "HawkesKernelParameters",
    "KnownNewsArrivalConfiguration",
    "NewsArrivalConfiguration",
    "NewsComponentMarkScaleConfiguration",
    "NewsProcessConfiguration",
    "SeededTickEventRecord",
    "SeededTickEventSide",
    "SeededTickEventStateSnapshot",
    "SeededTickEventStateSnapshotCapture",
    "SeededTickEventTapeMetadata",
    "SeededTickSimulationHourlyFactorSummary",
    "SeededTickSimulationRequest",
    "SeededTickSimulationResponse",
    "SeededTickSimulationValidationError",
    "SeededTickSimulationValidationFailure",
    "SharedNewsMarkLawConfiguration",
    "SurpriseNewsArrivalConfiguration",
    "deserialize_coarse_simulation_path",
    "deserialize_coarse_simulation_run_request",
    "deserialize_seeded_tick_simulation_response",
    "load_hourly_factor_contract_for_simulation",
    "run_coarse_simulation",
    "run_seeded_tick_event_simulation",
    "serialize_coarse_simulation_path",
    "serialize_coarse_simulation_run_request",
    "serialize_seeded_tick_simulation_response",
    "validate_coarse_simulation_path",
    "validate_coarse_simulation_run_request",
    "validate_seeded_tick_simulation_response",
]
