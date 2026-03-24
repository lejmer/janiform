from collections.abc import Sequence
from dataclasses import dataclass, field
from datetime import datetime
from enum import StrEnum
from typing import Any, Optional, cast, Final

from ..coarse.news import NewsProcessConfiguration
from ..shared import (
    HawkesExcitationState,
    HawkesKernelParameters,
    SeededTickEventSide,
    SeededTickEventStateSnapshotCapture,
)
from ..shared.contracts import _parse_seeded_tick_state_snapshot_capture
from ..shared.errors import SIMULATION_ERROR_FACTORY, _simulation_error
from ...contracts import (
    ContractModel,
    MeanReversionBoundaryState,
    MeanReversionFittedParameters,
    MeanReversionRuntimeIntervalAnchors,
    require_datetime_utc,
    require_finite_number,
    require_mapping,
    require_non_negative_finite_number,
    require_non_negative_finite_scalar,
    require_non_negative_integer,
    require_positive_integer,
    require_sequence,
    require_string,
)
from ...calendar import MarketState
from ...hourly_factors import HourlyFactorContract
from ...pricing import NigSharedPlusShiftConfiguration
from ...risk import SufficientStatisticsRiskReturnComponents


_MEAN_REVERSION_ALIGNMENT_TOLERANCE: Final[float] = 1e-12
_ONE_HOUR_NANOS: Final[int] = 3_600_000_000_000


def _parse_nig_shared_plus_shift_configuration(
    raw_payload: dict[str, Any] | Any,
    *,
    field_path: str,
) -> Any:
    from ...pricing import NigSharedPlusShiftConfiguration

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
    mean_reversion_fitted_parameters: MeanReversionFittedParameters
    mean_reversion_runtime_interval_anchors: MeanReversionRuntimeIntervalAnchors
    initial_mean_reversion_boundary_state: MeanReversionBoundaryState
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

        if (
            len(self.mean_reversion_runtime_interval_anchors.end_anchor_values)
            != self.horizon_interval_count
        ):
            raise _simulation_error(
                "mean_reversion_runtime_interval_anchors.end_anchor_values",
                "must contain one end-anchor value per simulated interval",
            )

        if (
            abs(
                self.initial_mean_reversion_boundary_state.anchor_value
                - self.mean_reversion_runtime_interval_anchors.start_anchor_value
            )
            > _MEAN_REVERSION_ALIGNMENT_TOLERANCE
        ):
            raise _simulation_error(
                "initial_mean_reversion_boundary_state.anchor_value",
                (
                    "must match "
                    "mean_reversion_runtime_interval_anchors.start_anchor_value"
                ),
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
        mean_reversion_fitted_parameters: MeanReversionFittedParameters,
        mean_reversion_runtime_interval_anchors: MeanReversionRuntimeIntervalAnchors,
        initial_mean_reversion_boundary_state: MeanReversionBoundaryState,
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
            mean_reversion_fitted_parameters=mean_reversion_fitted_parameters,
            mean_reversion_runtime_interval_anchors=mean_reversion_runtime_interval_anchors,
            initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
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
            mean_reversion_fitted_parameters=MeanReversionFittedParameters.from_mapping(
                require_mapping(
                    raw_payload,
                    "mean_reversion_fitted_parameters",
                    "mean_reversion_fitted_parameters",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path="mean_reversion_fitted_parameters",
            ),
            mean_reversion_runtime_interval_anchors=MeanReversionRuntimeIntervalAnchors.from_mapping(
                require_mapping(
                    raw_payload,
                    "mean_reversion_runtime_interval_anchors",
                    "mean_reversion_runtime_interval_anchors",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path="mean_reversion_runtime_interval_anchors",
            ),
            initial_mean_reversion_boundary_state=MeanReversionBoundaryState.from_mapping(
                require_mapping(
                    raw_payload,
                    "initial_mean_reversion_boundary_state",
                    "initial_mean_reversion_boundary_state",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path="initial_mean_reversion_boundary_state",
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
    core_total_return_increment: float
    observed_log_price: float
    anchor_value: float
    deviation_from_anchor: float
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
            core_total_return_increment=require_finite_number(
                raw_payload,
                "core_total_return_increment",
                f"{field_path}.core_total_return_increment",
                SIMULATION_ERROR_FACTORY,
            ),
            observed_log_price=require_finite_number(
                raw_payload,
                "observed_log_price",
                f"{field_path}.observed_log_price",
                SIMULATION_ERROR_FACTORY,
            ),
            anchor_value=require_finite_number(
                raw_payload,
                "anchor_value",
                f"{field_path}.anchor_value",
                SIMULATION_ERROR_FACTORY,
            ),
            deviation_from_anchor=require_finite_number(
                raw_payload,
                "deviation_from_anchor",
                f"{field_path}.deviation_from_anchor",
                SIMULATION_ERROR_FACTORY,
            ),
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
    core_total_return_increment: float
    observed_log_price: float
    anchor_value: float
    deviation_from_anchor: float
    cumulative_core_log_return: float
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
            core_total_return_increment=require_finite_number(
                raw_payload,
                "core_total_return_increment",
                f"{field_path}.core_total_return_increment",
                SIMULATION_ERROR_FACTORY,
            ),
            observed_log_price=require_finite_number(
                raw_payload,
                "observed_log_price",
                f"{field_path}.observed_log_price",
                SIMULATION_ERROR_FACTORY,
            ),
            anchor_value=require_finite_number(
                raw_payload,
                "anchor_value",
                f"{field_path}.anchor_value",
                SIMULATION_ERROR_FACTORY,
            ),
            deviation_from_anchor=require_finite_number(
                raw_payload,
                "deviation_from_anchor",
                f"{field_path}.deviation_from_anchor",
                SIMULATION_ERROR_FACTORY,
            ),
            cumulative_core_log_return=require_finite_number(
                raw_payload,
                "cumulative_core_log_return",
                f"{field_path}.cumulative_core_log_return",
                SIMULATION_ERROR_FACTORY,
            ),
            state_snapshot=state_snapshot,
        )


SEEDED_TICK_SIMULATION_DIAGNOSTICS_SCHEMA_VERSION: str = "simulation.seeded_tick.path_diagnostics.v1"
SEEDED_TICK_SIMULATION_BOUNDARY_TRANSPORT_POLICY: str = "native_handle_typed_column_chunk_projection.v1"
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
                core_total_return_increment=fine_path_record.core_total_return_increment,
                observed_log_price=fine_path_record.observed_log_price,
                anchor_value=fine_path_record.anchor_value,
                deviation_from_anchor=fine_path_record.deviation_from_anchor,
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


def build_disabled_seeded_mean_reversion_runtime_inputs(
    *,
    hourly_factor_contract: HourlyFactorContract,
) -> tuple[
    MeanReversionFittedParameters,
    MeanReversionRuntimeIntervalAnchors,
    MeanReversionBoundaryState,
]:
    interval_delta_hours = hourly_factor_contract.interval_duration_nanos / _ONE_HOUR_NANOS
    interval_count = hourly_factor_contract.interval_count

    return (
        MeanReversionFittedParameters.disabled(),
        MeanReversionRuntimeIntervalAnchors(
            interval_alignment_policy=hourly_factor_contract.interval_alignment_policy,
            start_anchor_value=0.0,
            end_anchor_values=tuple(0.0 for _ in range(interval_count)),
            interval_deltas=tuple(interval_delta_hours for _ in range(interval_count)),
        ),
        MeanReversionBoundaryState(
            observed_log_price=0.0,
            anchor_value=0.0,
            deviation=0.0,
        ),
    )


__all__ = [
    "SEEDED_TICK_SIMULATION_BOUNDARY_TRANSPORT_POLICY",
    "SEEDED_TICK_SIMULATION_DIAGNOSTICS_SCHEMA_VERSION",
    "SEEDED_TICK_SIMULATION_NATIVE_CHUNK_RECORD_COUNT",
    "SEEDED_TICK_SIMULATION_PAYLOAD_TRANSPORT_POLICY",
    "build_disabled_seeded_mean_reversion_runtime_inputs",
    "SeededFinePathMetadata",
    "SeededFinePathNewsArrivalComponent",
    "SeededFinePathRecord",
    "SeededFinePathTransitionKind",
    "SeededTickEventRecord",
    "SeededTickEventStateSnapshot",
    "SeededTickEventTapeMetadata",
    "SeededTickSimulationDiagnostics",
    "SeededTickSimulationHourlyFactorSummary",
    "SeededTickSimulationRequest",
]
