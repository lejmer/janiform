import json
from collections.abc import Callable, Iterator, Sequence
from dataclasses import dataclass, field
from datetime import UTC, datetime, timedelta
from enum import StrEnum
from time import perf_counter
from typing import Any, Optional, cast

from .._contracts import (
    ContractModel,
    parse_json_object,
    require_datetime_utc,
    require_finite_number,
    require_mapping,
    require_non_negative_finite_number,
    require_non_negative_finite_scalar,
    require_non_negative_integer,
    require_positive_integer,
    require_sequence,
    require_string,
    serialize_deterministic_json,
)
from .._native_api import native_module
from ..calendar import (
    CalendarInvalidTimestampError,
    CalendarInvalidTimezoneError,
    CalendarMissingExchangeError,
    CalendarRegistry,
    MarketState,
)
from ..hourly_factors import (
    HourlyFactorContract,
    HourlyFactorProviderMissingInputError,
    HourlyFactorProviderUnsupportedRangeError,
    HourlyNewsIntensityProviderSource,
    HourlySeasonalityProviderSource,
    _parse_interval_duration_nanos,
    provide_hourly_factor_contract,
)
from ..pricing import NigSharedPlusShiftConfiguration
from ..risk import SufficientStatisticsRiskReturnComponents
from ..seasonality import SeasonalityInputValidationError
from ._coarse import NewsProcessConfiguration
from ._shared import (
    HawkesExcitationState,
    HawkesKernelParameters,
    SIMULATION_ERROR_FACTORY,
    SIMULATION_INTEGRATION_HOURLY_FACTOR_ERROR_ID,
    SIMULATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
    SIMULATION_INTEGRATION_RESPONSE_ERROR_ID,
    SeededTickEventSide,
    SeededTickEventStateSnapshotCapture,
    SeededTickSimulationValidationError,
    _parse_seeded_tick_state_snapshot_capture,
    _simulation_error,
    _simulation_execution_error,
    _simulation_integration_error,
)

def _parse_nig_shared_plus_shift_configuration(
    raw_payload: dict[str, Any] | Any,
    *,
    field_path: str,
) -> Any:
    from ..pricing import NigSharedPlusShiftConfiguration

    return NigSharedPlusShiftConfiguration.from_mapping(raw_payload, field_path=field_path)




@dataclass(frozen=True, slots=True)
class SeededTickSimulationHourlyFactorSummary(ContractModel):
    """Serializable snapshot of the hourly factor contract used by one run."""

    schema_version: str
    exchange_code: str
    interval_count: int
    interval_duration_nanos: int
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
        if self.interval_duration_nanos <= 0:
            raise _simulation_error(
                "hourly_factor_summary.interval_duration_nanos",
                "must be strictly positive",
            )

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
            interval_duration_nanos=hourly_factor_contract.interval_duration_nanos,
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
            interval_duration_nanos=require_positive_integer(
                raw_payload,
                "interval_duration_nanos",
                f"{field_path}.interval_duration_nanos",
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
    nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration
    news_process_configuration: NewsProcessConfiguration
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
        nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
        news_process_configuration: NewsProcessConfiguration,
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
            nig_shared_plus_shift_configuration=nig_shared_plus_shift_configuration,
            news_process_configuration=news_process_configuration,
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
            nig_shared_plus_shift_configuration=_parse_nig_shared_plus_shift_configuration(
                require_mapping(
                    raw_payload,
                    "nig_shared_plus_shift_configuration",
                    "nig_shared_plus_shift_configuration",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path="nig_shared_plus_shift_configuration",
            ),
            news_process_configuration=NewsProcessConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "news_process_configuration",
                    "news_process_configuration",
                    SIMULATION_ERROR_FACTORY,
                )
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


class SeededFinePathTransitionKind(StrEnum):
    BUY_TRADE = 'buy_trade'
    SELL_TRADE = 'sell_trade'
    EXOGENOUS_NEWS = 'exogenous_news'
    INTERVAL_BOUNDARY_DRIFT = 'interval_boundary_drift'


class SeededFinePathNewsArrivalComponent(StrEnum):
    KNOWN = 'known'
    SURPRISE = 'surprise'


@dataclass(frozen=True, slots=True)
class SeededFinePathMetadata(ContractModel):
    schema_version: str
    path_record_schema_version: str
    event_projection_record_schema_version: str
    transition_ordering_policy: str
    timestamp_representation_policy: str
    side_encoding_policy: str
    state_snapshot_capture: SeededTickEventStateSnapshotCapture
    state_snapshot_capture_point_policy: Optional[str] = None

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str) -> SeededFinePathMetadata:
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

        return SeededFinePathMetadata(
            schema_version=require_string(raw_payload, "schema_version", f"{field_path}.schema_version", SIMULATION_ERROR_FACTORY),
            path_record_schema_version=require_string(
                raw_payload,
                "path_record_schema_version",
                f"{field_path}.path_record_schema_version",
                SIMULATION_ERROR_FACTORY,
            ),
            event_projection_record_schema_version=require_string(
                raw_payload,
                "event_projection_record_schema_version",
                f"{field_path}.event_projection_record_schema_version",
                SIMULATION_ERROR_FACTORY,
            ),
            transition_ordering_policy=require_string(
                raw_payload,
                "transition_ordering_policy",
                f"{field_path}.transition_ordering_policy",
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
class SeededFinePathRecord(ContractModel):
    transition_index: int
    timestamp_utc_nanos: int
    transition_kind: SeededFinePathTransitionKind
    side: Optional[SeededTickEventSide]
    news_arrival_component: Optional[SeededFinePathNewsArrivalComponent]
    market_state: MarketState
    return_components: SufficientStatisticsRiskReturnComponents
    cumulative_log_return: float
    state_snapshot: Optional[SeededTickEventStateSnapshot] = None

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str) -> SeededFinePathRecord:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        raw_transition_kind = require_string(
            raw_payload,
            "transition_kind",
            f"{field_path}.transition_kind",
            SIMULATION_ERROR_FACTORY,
        )
        try:
            transition_kind = SeededFinePathTransitionKind(raw_transition_kind)
        except ValueError as e:
            raise _simulation_error(
                f"{field_path}.transition_kind",
                (
                    "must be one of: buy_trade, sell_trade, exogenous_news, interval_boundary_drift, "
                    f"received '{raw_transition_kind}'"
                ),
            ) from e

        raw_side = raw_payload.get("side")
        side: Optional[SeededTickEventSide]
        if raw_side is None:
            side = None
        elif not isinstance(raw_side, str):
            raise _simulation_error(f"{field_path}.side", "must be a string when provided")
        else:
            try:
                side = SeededTickEventSide(raw_side)
            except ValueError as e:
                raise _simulation_error(
                    f"{field_path}.side",
                    f"must be one of: buy, sell, received '{raw_side}'",
                ) from e

        raw_news_arrival_component = raw_payload.get("news_arrival_component")
        news_arrival_component: Optional[SeededFinePathNewsArrivalComponent]
        if raw_news_arrival_component is None:
            news_arrival_component = None
        elif not isinstance(raw_news_arrival_component, str):
            raise _simulation_error(
                f"{field_path}.news_arrival_component",
                "must be a string when provided",
            )
        else:
            try:
                news_arrival_component = SeededFinePathNewsArrivalComponent(raw_news_arrival_component)
            except ValueError as e:
                raise _simulation_error(
                    f"{field_path}.news_arrival_component",
                    f"must be one of: known, surprise, received '{raw_news_arrival_component}'",
                ) from e

        raw_market_state = require_string(
            raw_payload,
            "market_state",
            f"{field_path}.market_state",
            SIMULATION_ERROR_FACTORY,
        )
        try:
            market_state = MarketState(raw_market_state)
        except ValueError as e:
            raise _simulation_error(
                f"{field_path}.market_state",
                f"must be one of: open, closed, received '{raw_market_state}'",
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

        return SeededFinePathRecord(
            transition_index=require_non_negative_integer(
                raw_payload,
                "transition_index",
                f"{field_path}.transition_index",
                SIMULATION_ERROR_FACTORY,
            ),
            timestamp_utc_nanos=require_non_negative_integer(
                raw_payload,
                "timestamp_utc_nanos",
                f"{field_path}.timestamp_utc_nanos",
                SIMULATION_ERROR_FACTORY,
            ),
            transition_kind=transition_kind,
            side=side,
            news_arrival_component=news_arrival_component,
            market_state=market_state,
            return_components=SufficientStatisticsRiskReturnComponents.from_mapping(
                require_mapping(
                    raw_payload,
                    "return_components",
                    f"{field_path}.return_components",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.return_components",
            ),
            cumulative_log_return=require_finite_number(
                raw_payload,
                "cumulative_log_return",
                f"{field_path}.cumulative_log_return",
                SIMULATION_ERROR_FACTORY,
            ),
            state_snapshot=state_snapshot,
        )


SEEDED_TICK_SIMULATION_DIAGNOSTICS_SCHEMA_VERSION: str = "simulation.seeded_tick.path_diagnostics.v1"
SEEDED_TICK_SIMULATION_BOUNDARY_TRANSPORT_POLICY: str = "native_handle_chunked_json_projection.v1"
SEEDED_TICK_SIMULATION_PAYLOAD_TRANSPORT_POLICY: str = "materialized_payload_projection.v1"
SEEDED_TICK_SIMULATION_NATIVE_CHUNK_RECORD_COUNT: int = 256


def _project_event_tape_from_fine_path(
    fine_path: Sequence[SeededFinePathRecord],
) -> tuple[SeededTickEventRecord, ...]:
    event_records: list[SeededTickEventRecord] = []
    for fine_path_record in fine_path:
        if fine_path_record.side is None:
            continue

        event_records.append(
            SeededTickEventRecord(
                event_index=len(event_records),
                timestamp_utc_nanos=fine_path_record.timestamp_utc_nanos,
                side=fine_path_record.side,
                state_snapshot=fine_path_record.state_snapshot,
            )
        )

    return tuple(event_records)


@dataclass(slots=True)
class SeededTickSimulationDiagnostics(ContractModel):
    schema_version: str
    transition_count: int
    event_count: int
    native_runtime_seconds: float = field(compare=False)
    python_response_construction_seconds: float = field(compare=False)
    python_materialization_seconds: float = field(compare=False)
    boundary_transport_bytes: int = field(compare=False)
    native_estimated_retained_bytes: int
    materialized_transition_count: int = field(compare=False)
    materialized_event_count: int = field(compare=False)
    boundary_transport_policy: str
    boundary_chunk_record_count: int

    def __post_init__(self) -> None:
        if self.schema_version != SEEDED_TICK_SIMULATION_DIAGNOSTICS_SCHEMA_VERSION:
            raise _simulation_error(
                "diagnostics.schema_version",
                f"must be `{SEEDED_TICK_SIMULATION_DIAGNOSTICS_SCHEMA_VERSION}`",
            )

        for field_name in (
            "transition_count",
            "event_count",
            "boundary_transport_bytes",
            "native_estimated_retained_bytes",
            "materialized_transition_count",
            "materialized_event_count",
            "boundary_chunk_record_count",
        ):
            require_non_negative_integer(
                {field_name: getattr(self, field_name)},
                field_name,
                f"diagnostics.{field_name}",
                SIMULATION_ERROR_FACTORY,
            )

        for field_name in (
            "native_runtime_seconds",
            "python_response_construction_seconds",
            "python_materialization_seconds",
        ):
            require_non_negative_finite_scalar(
                getattr(self, field_name),
                f"diagnostics.{field_name}",
                SIMULATION_ERROR_FACTORY,
            )

        if not self.boundary_transport_policy.strip():
            raise _simulation_error(
                "diagnostics.boundary_transport_policy",
                "must be a non-empty string",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> SeededTickSimulationDiagnostics:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        return SeededTickSimulationDiagnostics(
            schema_version=require_string(
                raw_payload,
                "schema_version",
                f"{field_path}.schema_version",
                SIMULATION_ERROR_FACTORY,
            ),
            transition_count=require_non_negative_integer(
                raw_payload,
                "transition_count",
                f"{field_path}.transition_count",
                SIMULATION_ERROR_FACTORY,
            ),
            event_count=require_non_negative_integer(
                raw_payload,
                "event_count",
                f"{field_path}.event_count",
                SIMULATION_ERROR_FACTORY,
            ),
            native_runtime_seconds=require_non_negative_finite_number(
                raw_payload,
                "native_runtime_seconds",
                f"{field_path}.native_runtime_seconds",
                SIMULATION_ERROR_FACTORY,
            ),
            python_response_construction_seconds=require_non_negative_finite_number(
                raw_payload,
                "python_response_construction_seconds",
                f"{field_path}.python_response_construction_seconds",
                SIMULATION_ERROR_FACTORY,
            ),
            python_materialization_seconds=require_non_negative_finite_number(
                raw_payload,
                "python_materialization_seconds",
                f"{field_path}.python_materialization_seconds",
                SIMULATION_ERROR_FACTORY,
            ),
            boundary_transport_bytes=require_non_negative_integer(
                raw_payload,
                "boundary_transport_bytes",
                f"{field_path}.boundary_transport_bytes",
                SIMULATION_ERROR_FACTORY,
            ),
            native_estimated_retained_bytes=require_non_negative_integer(
                raw_payload,
                "native_estimated_retained_bytes",
                f"{field_path}.native_estimated_retained_bytes",
                SIMULATION_ERROR_FACTORY,
            ),
            materialized_transition_count=require_non_negative_integer(
                raw_payload,
                "materialized_transition_count",
                f"{field_path}.materialized_transition_count",
                SIMULATION_ERROR_FACTORY,
            ),
            materialized_event_count=require_non_negative_integer(
                raw_payload,
                "materialized_event_count",
                f"{field_path}.materialized_event_count",
                SIMULATION_ERROR_FACTORY,
            ),
            boundary_transport_policy=require_string(
                raw_payload,
                "boundary_transport_policy",
                f"{field_path}.boundary_transport_policy",
                SIMULATION_ERROR_FACTORY,
            ),
            boundary_chunk_record_count=require_positive_integer(
                raw_payload,
                "boundary_chunk_record_count",
                f"{field_path}.boundary_chunk_record_count",
                SIMULATION_ERROR_FACTORY,
            ),
        )


class _SeededTickSimulationMaterializationTracker:
    __slots__ = ("_diagnostics",)

    def __init__(self, diagnostics: SeededTickSimulationDiagnostics) -> None:
        self._diagnostics = diagnostics

    def record_fine_path_chunk(
        self,
        *,
        record_count: int,
        boundary_transport_bytes: int,
        python_materialization_seconds: float,
    ) -> None:
        self._diagnostics.boundary_transport_bytes += boundary_transport_bytes
        self._diagnostics.python_materialization_seconds += python_materialization_seconds
        self._diagnostics.materialized_transition_count += record_count

    def record_event_tape_chunk(
        self,
        *,
        record_count: int,
        boundary_transport_bytes: int,
        python_materialization_seconds: float,
    ) -> None:
        self._diagnostics.boundary_transport_bytes += boundary_transport_bytes
        self._diagnostics.python_materialization_seconds += python_materialization_seconds
        self._diagnostics.materialized_event_count += record_count


type _NativeChunkLoader = Callable[[int, int], str]
type _RecordParser[T] = Callable[[dict[str, Any], int], T]
type _ChunkTrackerCallback = Callable[[int, int, float], None]


class _NativeBackedSequenceView[T](Sequence[T]):
    __slots__ = (
        "_cached_chunk_records",
        "_cached_chunk_start",
        "_chunk_record_count",
        "_chunk_tracker",
        "_field_path",
        "_index_error_message",
        "_materialized_records",
        "_native_chunk_loader",
        "_record_count",
        "_record_parser",
    )

    def __init__(
        self,
        *,
        field_path: str,
        index_error_message: str,
        record_parser: _RecordParser[T],
        materialized_records: Optional[tuple[T, ...]] = None,
        native_chunk_loader: Optional[_NativeChunkLoader] = None,
        record_count: Optional[int] = None,
        chunk_record_count: int = SEEDED_TICK_SIMULATION_NATIVE_CHUNK_RECORD_COUNT,
        chunk_tracker: Optional[_ChunkTrackerCallback] = None,
    ) -> None:
        if materialized_records is None and native_chunk_loader is None:
            raise _simulation_error(
                field_path,
                "must be backed by materialized records or a native result handle",
            )

        self._materialized_records = materialized_records
        self._native_chunk_loader = native_chunk_loader
        self._record_count = len(materialized_records) if materialized_records is not None else record_count
        self._chunk_record_count = chunk_record_count
        self._field_path = field_path
        self._index_error_message = index_error_message
        self._record_parser = record_parser
        self._chunk_tracker = chunk_tracker
        self._cached_chunk_start: Optional[int] = None
        self._cached_chunk_records: tuple[T, ...] = ()

    def __eq__(self, other: object) -> bool:
        if isinstance(other, Sequence) and not isinstance(other, str):
            return tuple(self) == tuple(other)

        return False

    def __len__(self) -> int:
        if self._materialized_records is not None:
            return len(self._materialized_records)

        return cast(int, self._record_count)

    def __getitem__(self, index: int | slice) -> T | tuple[T, ...]:
        if isinstance(index, slice):
            start_index, stop_index, step = index.indices(len(self))
            return tuple(self[position] for position in range(start_index, stop_index, step))

        normalized_index = self._normalize_index(index)
        if self._materialized_records is not None:
            return self._materialized_records[normalized_index]

        if (
            self._cached_chunk_start is not None
            and normalized_index >= self._cached_chunk_start
            and normalized_index < self._cached_chunk_start + len(self._cached_chunk_records)
        ):
            return self._cached_chunk_records[normalized_index - self._cached_chunk_start]

        chunk_start = (normalized_index // self._chunk_record_count) * self._chunk_record_count
        self._cached_chunk_records = self._load_native_chunk(chunk_start)
        self._cached_chunk_start = chunk_start
        return self._cached_chunk_records[normalized_index - chunk_start]

    def __iter__(self) -> Iterator[T]:
        if self._materialized_records is not None:
            return iter(self._materialized_records)

        return self._iter_native_records()

    def _iter_native_records(self) -> Iterator[T]:
        record_count = len(self)
        next_record_position = 0
        while next_record_position < record_count:
            for record in self._load_native_chunk(next_record_position):
                yield record

            next_record_position += self._chunk_record_count

    def _load_native_chunk(self, start_index: int) -> tuple[T, ...]:
        try:
            chunk_json = cast(_NativeChunkLoader, self._native_chunk_loader)(
                start_index,
                self._chunk_record_count,
            )
        except Exception as e:
            raise _simulation_integration_error(
                f"native {self._field_path} chunk projection failed",
                entrypoint="run_seeded_tick_event_simulation",
                error_id=SIMULATION_INTEGRATION_RESPONSE_ERROR_ID,
                field_path=self._field_path,
                context={"chunk_start_index": start_index},
            ) from e

        parse_started = perf_counter()
        try:
            parsed_chunk = json.loads(chunk_json)
        except json.JSONDecodeError as e:
            raise _simulation_integration_error(
                f"native {self._field_path} chunk payload was not valid JSON",
                entrypoint="run_seeded_tick_event_simulation",
                error_id=SIMULATION_INTEGRATION_RESPONSE_ERROR_ID,
                field_path=self._field_path,
                context={"chunk_start_index": start_index},
            ) from e
        if not isinstance(parsed_chunk, list):
            raise _simulation_integration_error(
                f"native {self._field_path} chunk payload was not a list",
                entrypoint="run_seeded_tick_event_simulation",
                error_id=SIMULATION_INTEGRATION_RESPONSE_ERROR_ID,
                field_path=self._field_path,
                context={"chunk_start_index": start_index},
            )

        chunk_records = tuple(
            self._record_parser(raw_record, start_index + chunk_offset)
            for chunk_offset, raw_record in enumerate(parsed_chunk)
        )
        parse_seconds = perf_counter() - parse_started

        if self._chunk_tracker is not None:
            self._chunk_tracker(
                record_count=len(chunk_records),
                boundary_transport_bytes=len(chunk_json.encode("utf-8")),
                python_materialization_seconds=parse_seconds,
            )

        return chunk_records

    def _normalize_index(self, index: int) -> int:
        normalized_index = index
        if normalized_index < 0:
            normalized_index += len(self)
        if normalized_index < 0 or normalized_index >= len(self):
            raise IndexError(self._index_error_message)

        return normalized_index


def _parse_seeded_fine_path_view_record(
    raw_record: dict[str, Any],
    record_index: int,
) -> SeededFinePathRecord:
    return SeededFinePathRecord.from_mapping(
        raw_record,
        field_path=f"seeded_tick_simulation_response.fine_path[{record_index}]",
    )


def _parse_seeded_event_tape_view_record(
    raw_record: dict[str, Any],
    record_index: int,
) -> SeededTickEventRecord:
    return SeededTickEventRecord.from_mapping(
        raw_record,
        field_path=f"seeded_tick_simulation_response.event_tape[{record_index}]",
    )


def _require_native_result_attribute(
    native_result: Any,
    attribute_name: str,
    *,
    field_path: str,
) -> object:
    try:
        return getattr(native_result, attribute_name)
    except AttributeError as e:
        raise _simulation_error(
            field_path,
            f"native result handle is missing required attribute `{attribute_name}`",
        ) from e


def _require_native_result_json_method(
    native_result: Any,
    method_name: str,
    *,
    field_path: str,
) -> str:
    raw_method = _require_native_result_attribute(
        native_result,
        method_name,
        field_path=field_path,
    )
    if not callable(raw_method):
        raise _simulation_error(
            field_path,
            f"native result attribute `{method_name}` must be callable",
        )

    try:
        payload = raw_method()
    except Exception as e:
        raise _simulation_error(
            field_path,
            f"native result method `{method_name}` failed while building the response",
        ) from e

    if not isinstance(payload, str):
        raise _simulation_error(
            field_path,
            f"native result method `{method_name}` must return a JSON string",
        )

    return payload


class SeededFinePathView(_NativeBackedSequenceView[SeededFinePathRecord]):
    __slots__ = ()

    def __init__(
        self,
        *,
        materialized_records: Optional[tuple[SeededFinePathRecord, ...]] = None,
        native_result: Optional[Any] = None,
        chunk_record_count: int = SEEDED_TICK_SIMULATION_NATIVE_CHUNK_RECORD_COUNT,
        tracker: Optional[_SeededTickSimulationMaterializationTracker] = None,
    ) -> None:
        super().__init__(
            field_path="fine_path",
            index_error_message="seeded fine path index out of range",
            record_parser=_parse_seeded_fine_path_view_record,
            materialized_records=materialized_records,
            native_chunk_loader=(
                native_result.get_fine_path_chunk_json if native_result is not None else None
            ),
            record_count=(
                int(native_result.transition_count) if native_result is not None else None
            ),
            chunk_record_count=chunk_record_count,
            chunk_tracker=(
                tracker.record_fine_path_chunk if tracker is not None else None
            ),
        )


class SeededTickEventTapeView(_NativeBackedSequenceView[SeededTickEventRecord]):
    __slots__ = ()

    def __init__(
        self,
        *,
        materialized_records: Optional[tuple[SeededTickEventRecord, ...]] = None,
        native_result: Optional[Any] = None,
        chunk_record_count: int = SEEDED_TICK_SIMULATION_NATIVE_CHUNK_RECORD_COUNT,
        tracker: Optional[_SeededTickSimulationMaterializationTracker] = None,
    ) -> None:
        super().__init__(
            field_path="event_tape",
            index_error_message="seeded event tape index out of range",
            record_parser=_parse_seeded_event_tape_view_record,
            materialized_records=materialized_records,
            native_chunk_loader=(
                native_result.get_event_tape_chunk_json if native_result is not None else None
            ),
            record_count=int(native_result.event_count) if native_result is not None else None,
            chunk_record_count=chunk_record_count,
            chunk_tracker=(
                tracker.record_event_tape_chunk if tracker is not None else None
            ),
        )


@dataclass(slots=True)
class SeededTickSimulationResponse(ContractModel):
    """Typed seeded-tick simulation response containing lazy fine-path and event-tape views."""

    schema_version: str
    metadata: SeededFinePathMetadata
    event_tape_metadata: SeededTickEventTapeMetadata
    diagnostics: SeededTickSimulationDiagnostics
    fine_path: SeededFinePathView
    event_tape: SeededTickEventTapeView

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "seeded_tick_simulation_response",
    ) -> SeededTickSimulationResponse:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        fine_path_items = require_sequence(
            raw_payload,
            "fine_path",
            f"{field_path}.fine_path",
            SIMULATION_ERROR_FACTORY,
        )
        fine_path_records = tuple(
            SeededFinePathRecord.from_mapping(
                raw_item,
                field_path=f"{field_path}.fine_path[{transition_index}]",
            )
            for transition_index, raw_item in enumerate(fine_path_items)
        )

        event_tape_items = require_sequence(
            raw_payload,
            "event_tape",
            f"{field_path}.event_tape",
            SIMULATION_ERROR_FACTORY,
        )
        event_tape_records = tuple(
            SeededTickEventRecord.from_mapping(
                raw_item,
                field_path=f"{field_path}.event_tape[{event_index}]",
            )
            for event_index, raw_item in enumerate(event_tape_items)
        )
        projected_event_tape = _project_event_tape_from_fine_path(fine_path_records)
        if event_tape_records != projected_event_tape:
            raise _simulation_error(
                f"{field_path}.event_tape",
                "must match the deterministic event projection derived from fine_path",
            )

        diagnostics = SeededTickSimulationDiagnostics.from_mapping(
            require_mapping(
                raw_payload,
                "diagnostics",
                f"{field_path}.diagnostics",
                SIMULATION_ERROR_FACTORY,
            ),
            field_path=f"{field_path}.diagnostics",
        )

        return SeededTickSimulationResponse(
            schema_version=require_string(
                raw_payload,
                "schema_version",
                f"{field_path}.schema_version",
                SIMULATION_ERROR_FACTORY,
            ),
            metadata=SeededFinePathMetadata.from_mapping(
                require_mapping(
                    raw_payload,
                    "metadata",
                    f"{field_path}.metadata",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.metadata",
            ),
            event_tape_metadata=SeededTickEventTapeMetadata.from_mapping(
                require_mapping(
                    raw_payload,
                    "event_tape_metadata",
                    f"{field_path}.event_tape_metadata",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.event_tape_metadata",
            ),
            diagnostics=diagnostics,
            fine_path=SeededFinePathView(materialized_records=fine_path_records),
            event_tape=SeededTickEventTapeView(materialized_records=event_tape_records),
        )

    @staticmethod
    def from_native_result(
        *,
        native_result: Any,
        native_runtime_seconds: float,
        python_response_construction_seconds: float,
        chunk_record_count: int,
    ) -> SeededTickSimulationResponse:
        schema_version = require_string(
            {
                "schema_version": _require_native_result_attribute(
                    native_result,
                    "schema_version",
                    field_path="seeded_tick_simulation_response.schema_version",
                )
            },
            "schema_version",
            "seeded_tick_simulation_response.schema_version",
            SIMULATION_ERROR_FACTORY,
        )
        transition_count = require_non_negative_integer(
            {
                "transition_count": _require_native_result_attribute(
                    native_result,
                    "transition_count",
                    field_path="seeded_tick_simulation_response.diagnostics.transition_count",
                )
            },
            "transition_count",
            "seeded_tick_simulation_response.diagnostics.transition_count",
            SIMULATION_ERROR_FACTORY,
        )
        event_count = require_non_negative_integer(
            {
                "event_count": _require_native_result_attribute(
                    native_result,
                    "event_count",
                    field_path="seeded_tick_simulation_response.diagnostics.event_count",
                )
            },
            "event_count",
            "seeded_tick_simulation_response.diagnostics.event_count",
            SIMULATION_ERROR_FACTORY,
        )
        native_estimated_retained_bytes = require_non_negative_integer(
            {
                "estimated_native_retained_bytes": _require_native_result_attribute(
                    native_result,
                    "estimated_native_retained_bytes",
                    field_path=(
                        "seeded_tick_simulation_response.diagnostics.native_estimated_retained_bytes"
                    ),
                )
            },
            "estimated_native_retained_bytes",
            "seeded_tick_simulation_response.diagnostics.native_estimated_retained_bytes",
            SIMULATION_ERROR_FACTORY,
        )
        diagnostics = SeededTickSimulationDiagnostics(
            schema_version=SEEDED_TICK_SIMULATION_DIAGNOSTICS_SCHEMA_VERSION,
            transition_count=transition_count,
            event_count=event_count,
            native_runtime_seconds=native_runtime_seconds,
            python_response_construction_seconds=python_response_construction_seconds,
            python_materialization_seconds=0.0,
            boundary_transport_bytes=0,
            native_estimated_retained_bytes=native_estimated_retained_bytes,
            materialized_transition_count=0,
            materialized_event_count=0,
            boundary_transport_policy=SEEDED_TICK_SIMULATION_BOUNDARY_TRANSPORT_POLICY,
            boundary_chunk_record_count=chunk_record_count,
        )
        tracker = _SeededTickSimulationMaterializationTracker(diagnostics)

        metadata_payload = parse_json_object(
            _require_native_result_json_method(
                native_result,
                "fine_path_metadata_json",
                field_path="seeded_tick_simulation_response.metadata",
            ),
            "seeded fine-path metadata",
            SIMULATION_ERROR_FACTORY,
        )
        event_tape_metadata_payload = parse_json_object(
            _require_native_result_json_method(
                native_result,
                "event_tape_metadata_json",
                field_path="seeded_tick_simulation_response.event_tape_metadata",
            ),
            "seeded event-tape metadata",
            SIMULATION_ERROR_FACTORY,
        )

        return SeededTickSimulationResponse(
            schema_version=schema_version,
            metadata=SeededFinePathMetadata.from_mapping(
                dict(metadata_payload),
                field_path="seeded_tick_simulation_response.metadata",
            ),
            event_tape_metadata=SeededTickEventTapeMetadata.from_mapping(
                dict(event_tape_metadata_payload),
                field_path="seeded_tick_simulation_response.event_tape_metadata",
            ),
            diagnostics=diagnostics,
            fine_path=SeededFinePathView(
                native_result=native_result,
                chunk_record_count=chunk_record_count,
                tracker=tracker,
            ),
            event_tape=SeededTickEventTapeView(
                native_result=native_result,
                chunk_record_count=chunk_record_count,
                tracker=tracker,
            ),
        )


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
        nig_shared_plus_shift_configuration=request.nig_shared_plus_shift_configuration,
        news_process_configuration=request.news_process_configuration,
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
        native_run_started = perf_counter()
        native_result: Any = native.run_seeded_tick_event_simulation(
            normalized_request.seed,
            normalized_request.start_time_utc.astimezone(UTC).isoformat(),
            normalized_request.horizon_interval_count,
            normalized_request.kernel_parameters.to_payload(),
            normalized_request.nig_shared_plus_shift_configuration.to_payload(),
            normalized_request.initial_excitation_state.to_payload(),
            normalized_request.hourly_factor_contract._native_result,
            serialize_deterministic_json(normalized_request.news_process_configuration),
            debug_trace_enabled=normalized_request.debug_trace_enabled,
            sample_every_nth_transition=normalized_request.sample_every_nth_transition,
            max_records_per_chunk=normalized_request.max_records_per_chunk,
            state_snapshot_capture=normalized_request.state_snapshot_capture.value,
        )
        native_runtime_seconds = perf_counter() - native_run_started
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
