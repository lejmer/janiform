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
    require_non_negative_finite_number,
    require_non_negative_finite_scalar,
    require_non_negative_integer,
    require_positive_finite_number,
    require_positive_integer,
    require_sequence,
    require_string,
    serialize_deterministic_json,
)
from ._native_api import native_module
from .calendar import CalendarRegistry
from .hourly_factors import (
    HourlyFactorContract,
    HourlyNewsIntensityProviderSource,
    HourlySeasonalityProviderSource,
    provide_hourly_factor_contract,
)


@dataclass(frozen=True, slots=True)
class SeededTickSimulationValidationFailure:
    field_path: str
    message: str


class SeededTickSimulationValidationError(ValueError):
    def __init__(self, failure: SeededTickSimulationValidationFailure) -> None:
        self.failure: SeededTickSimulationValidationFailure = failure
        super().__init__(f"{failure.field_path}: {failure.message}")


def _simulation_error(field_path: str, message: str) -> SeededTickSimulationValidationError:
    return SeededTickSimulationValidationError(
        SeededTickSimulationValidationFailure(field_path=field_path, message=message)
    )


SIMULATION_ERROR_FACTORY: ValidationErrorFactory = _simulation_error


class SeededTickEventSide(StrEnum):
    BUY = 'buy'
    SELL = 'sell'


class SeededTickEventStateSnapshotCapture(StrEnum):
    DISABLED = 'disabled'
    POST_EVENT_TRANSITION = 'post_event_transition'


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

    def __post_init__(self) -> None:
        if self.seed < 0:
            raise _simulation_error("seed", "must be non-negative")
        if self.start_time_utc.tzinfo is None:
            raise _simulation_error("start_time_utc", "must be timezone-aware")
        if self.horizon_interval_count <= 0:
            raise _simulation_error("horizon_interval_count", "must be strictly positive")
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

    return provide_hourly_factor_contract(
        calendar_registry=calendar_registry,
        exchange_code=exchange_code,
        range_start_utc=range_start_utc,
        range_end_utc=range_end_utc,
        seasonality_source=seasonality_source,
        news_intensity_source=news_intensity_source,
    )


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
) -> SeededTickSimulationResponse:
    request = SeededTickSimulationRequest.from_runtime_inputs(
        seed=seed,
        start_time_utc=start_time_utc,
        horizon_interval_count=horizon_interval_count,
        kernel_parameters=kernel_parameters,
        initial_excitation_state=initial_excitation_state,
        hourly_factor_contract=hourly_factor_contract,
        debug_trace_enabled=debug_trace_enabled,
        sample_every_nth_transition=sample_every_nth_transition,
        max_records_per_chunk=max_records_per_chunk,
        state_snapshot_capture=state_snapshot_capture,
    )

    native: ModuleType = native_module()
    event_tape_json: str = native.run_seeded_tick_event_simulation(
        request.seed,
        request.start_time_utc.astimezone(UTC).isoformat(),
        request.horizon_interval_count,
        request.kernel_parameters.to_payload(),
        request.initial_excitation_state.to_payload(),
        request.hourly_factor_contract._native_result,
        debug_trace_enabled=request.debug_trace_enabled,
        sample_every_nth_transition=request.sample_every_nth_transition,
        max_records_per_chunk=request.max_records_per_chunk,
        state_snapshot_capture=request.state_snapshot_capture.value,
    )

    return deserialize_seeded_tick_simulation_response(event_tape_json)


__all__ = [
    "HawkesExcitationState",
    "HawkesKernelLoadings",
    "HawkesKernelParameters",
    "SeededTickEventRecord",
    "SeededTickEventSide",
    "SeededTickEventStateSnapshot",
    "SeededTickEventStateSnapshotCapture",
    "SeededTickEventTapeMetadata",
    "SeededTickSimulationRequest",
    "SeededTickSimulationResponse",
    "SeededTickSimulationValidationError",
    "SeededTickSimulationValidationFailure",
    "deserialize_seeded_tick_simulation_response",
    "load_hourly_factor_contract_for_simulation",
    "run_seeded_tick_event_simulation",
    "serialize_seeded_tick_simulation_response",
    "validate_seeded_tick_simulation_response",
]
