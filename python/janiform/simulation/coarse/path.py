from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from datetime import datetime
from math import isfinite
from typing import Any, Optional

from ._native_path_hydration import coarse_simulation_path_from_validated_payload
from .news import (
    COARSE_SIMULATION_COMPATIBLE_INTERVAL_GRID_POLICY,
    COARSE_SIMULATION_ENVIRONMENT_METADATA_POLICY,
    COARSE_SIMULATION_INTERVAL_FACTOR_ALIGNMENT_POLICY,
    COARSE_SIMULATION_INTERVAL_IDENTIFIER_POLICY,
    COARSE_SIMULATION_INTERVAL_TIMING_POLICY,
    KnownNewsArrivalConfiguration,
    NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration,
    NewsProcessConfiguration,
    SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
    _interval_delta_to_nanos,
    _prefix_nested_field_path,
    _validate_calendar_registry_artifact_payload,
)
from ..shared import HawkesExcitationState
from ..shared.errors import (
    SIMULATION_ERROR_FACTORY,
    _simulation_error,
)
from ...mean_reversion import MeanReversionBoundaryState
from ...long_variance import LongVarianceParameters, LongVarianceState
from ...return_clock import ReturnClockExposureParameters
from ...contracts.base import (
    ContractModel,
    parse_json_object,
    require_datetime_utc,
    require_finite_number,
    require_mapping,
    require_non_empty_string,
    require_non_negative_integer,
    require_positive_finite_number,
    require_positive_integer,
    require_sequence,
    require_string,
    serialize_deterministic_json,
)
from ..._timestamps import datetime_to_utc_nanos
from ..._native_api import native_error_parts, native_exception_matches, native_module
from ...calendar import CalendarRegistry
from ...hourly_factors import (
    HourlyFactorContract,
    SeasonalityInputValidationError,
    load_hourly_factor_contract_payload,
)


COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION = "simulation.coarse_interval.run_request.v1"
COARSE_SIMULATION_PATH_SCHEMA_VERSION = "simulation.coarse_interval.path.v1"
COARSE_SIMULATION_PATH_RECORD_SCHEMA_VERSION = "simulation.coarse_interval.path_record.v1"
COARSE_SIMULATION_PATH_OUTPUT_SCHEMA_METADATA_VERSION = "simulation.coarse_interval.path_output_schema_metadata.v1"
COARSE_SIMULATION_PATH_PROVENANCE_SCHEMA_VERSION = "simulation.coarse_interval.path_provenance.v1"
COARSE_SIMULATION_PATH_DIAGNOSTICS_SCHEMA_VERSION = "simulation.coarse_interval.path_diagnostics.v1"
COARSE_SIMULATION_ARTIFACT_WORKFLOW_POLICY = (
    "time_indexed_coarse_interval_simulation_path_for_validation_and_analytics.v1"
)


@dataclass(frozen=True, slots=True)
class CoarseSimulationModelInputs(ContractModel):
    sufficient_statistics_model_inputs: Any
    news_process_configuration: NewsProcessConfiguration
    return_clock_exposure_parameters: ReturnClockExposureParameters
    long_variance_parameters: LongVarianceParameters
    initial_long_variance_state: LongVarianceState
    observed_price_tick_size: Optional[float]
    calendar_registry_artifact: dict[str, Any]
    exchange_code: str

    def __post_init__(self) -> None:
        if not isinstance(
            self.return_clock_exposure_parameters,
            ReturnClockExposureParameters,
        ):
            raise _simulation_error(
                "return_clock_exposure_parameters",
                "must be a ReturnClockExposureParameters",
            )

        if not isinstance(self.long_variance_parameters, LongVarianceParameters):
            raise _simulation_error(
                "long_variance_parameters",
                "must be a LongVarianceParameters",
            )

        if not isinstance(self.initial_long_variance_state, LongVarianceState):
            raise _simulation_error(
                "initial_long_variance_state",
                "must be a LongVarianceState",
            )

        if self.observed_price_tick_size is not None and (
            not isfinite(self.observed_price_tick_size)
            or self.observed_price_tick_size <= 0.0
        ):
            raise _simulation_error(
                "observed_price_tick_size",
                "must be finite and strictly positive when provided",
            )

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

        from ...sufficient_statistics import SufficientStatisticsModelInputs, SufficientStatisticsValidationError

        try:
            sufficient_statistics_model_inputs = SufficientStatisticsModelInputs.from_mapping(
                require_mapping(
                    raw_payload,
                    "sufficient_statistics_model_inputs",
                    f"{field_path}.sufficient_statistics_model_inputs",
                    SIMULATION_ERROR_FACTORY,
                )
            )
        except SufficientStatisticsValidationError as e:
            raise _simulation_error(
                _prefix_nested_field_path(
                    e.failure.field_path,
                    prefix=f"{field_path}.sufficient_statistics_model_inputs",
                    strip_prefix="model_inputs.",
                ),
                e.failure.message,
            ) from e

        return CoarseSimulationModelInputs(
            sufficient_statistics_model_inputs=sufficient_statistics_model_inputs,
            news_process_configuration=NewsProcessConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "news_process_configuration",
                    f"{field_path}.news_process_configuration",
                    SIMULATION_ERROR_FACTORY,
                )
            ),
            return_clock_exposure_parameters=ReturnClockExposureParameters.from_mapping(
                require_mapping(
                    raw_payload,
                    "return_clock_exposure_parameters",
                    f"{field_path}.return_clock_exposure_parameters",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.return_clock_exposure_parameters",
                error_factory=SIMULATION_ERROR_FACTORY,
            ),
            long_variance_parameters=LongVarianceParameters.from_mapping(
                require_mapping(
                    raw_payload,
                    "long_variance_parameters",
                    f"{field_path}.long_variance_parameters",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.long_variance_parameters",
                error_factory=SIMULATION_ERROR_FACTORY,
            ),
            initial_long_variance_state=LongVarianceState.from_mapping(
                require_mapping(
                    raw_payload,
                    "initial_long_variance_state",
                    f"{field_path}.initial_long_variance_state",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.initial_long_variance_state",
                error_factory=SIMULATION_ERROR_FACTORY,
            ),
            observed_price_tick_size=(
                require_finite_number(
                    raw_payload,
                    "observed_price_tick_size",
                    f"{field_path}.observed_price_tick_size",
                    SIMULATION_ERROR_FACTORY,
                )
                if "observed_price_tick_size" in raw_payload
                and raw_payload["observed_price_tick_size"] is not None
                else None
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
    hourly_factor_contract: HourlyFactorContract

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

        interval_delta_nanos = _interval_delta_to_nanos(self.interval_delta, field_path="interval_delta")

        if self.horizon_interval_count > self.hourly_factor_contract.interval_count:
            raise _simulation_error(
                "horizon_interval_count",
                (
                    "must not exceed available hourly factor interval count "
                    f"({self.hourly_factor_contract.interval_count})"
                ),
            )

        if self.hourly_factor_contract.interval_duration_nanos != interval_delta_nanos:
            raise _simulation_error(
                "hourly_factor_contract.interval_duration_nanos",
                (
                    "must match interval_delta exactly under explicit factor alignment without implicit "
                    f"resampling (interval_delta={self.interval_delta}, "
                    "hourly_factor_contract.interval_duration_nanos="
                    f"{self.hourly_factor_contract.interval_duration_nanos})"
                ),
            )

        if self.hourly_factor_contract.first_interval_start_utc_nanos != self.start_time_utc_nanos:
            raise _simulation_error(
                "hourly_factor_contract.first_interval_start_utc_nanos",
                (
                    "must match start_time_utc_nanos exactly under explicit factor alignment "
                    f"(start_time_utc_nanos={self.start_time_utc_nanos}, "
                    "hourly_factor_contract.first_interval_start_utc_nanos="
                    f"{self.hourly_factor_contract.first_interval_start_utc_nanos})"
                ),
            )

        runtime_interval_anchors = (
            self.model_inputs.sufficient_statistics_model_inputs.mean_reversion_runtime_interval_anchors
        )
        if len(runtime_interval_anchors.end_anchor_values) != self.horizon_interval_count:
            raise _simulation_error(
                "model_inputs.sufficient_statistics_model_inputs.mean_reversion_runtime_interval_anchors.end_anchor_values",
                f"must contain {self.horizon_interval_count} values",
            )

        for interval_index, observed_interval_delta in enumerate(
            runtime_interval_anchors.interval_deltas[: self.horizon_interval_count]
        ):
            field_path = (
                "model_inputs.sufficient_statistics_model_inputs"
                ".mean_reversion_runtime_interval_anchors"
                f".interval_deltas[{interval_index}]"
            )
            observed_interval_delta_nanos = _interval_delta_to_nanos(
                observed_interval_delta,
                field_path=field_path,
            )
            if observed_interval_delta_nanos != interval_delta_nanos:
                raise _simulation_error(
                    field_path,
                    (
                        f"must equal interval_delta ({self.interval_delta}), "
                        f"found {observed_interval_delta}"
                    ),
                )

    @staticmethod
    def from_runtime_inputs(
        *,
        seed: int,
        start_time_utc: datetime,
        horizon_interval_count: int,
        interval_delta: float,
        sufficient_statistics_model_inputs: Any,
        hourly_factor_contract: HourlyFactorContract,
        news_process_configuration: NewsProcessConfiguration,
        return_clock_exposure_parameters: ReturnClockExposureParameters,
        long_variance_parameters: LongVarianceParameters,
        initial_long_variance_state: LongVarianceState,
        observed_price_tick_size: Optional[float],
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
                sufficient_statistics_model_inputs=sufficient_statistics_model_inputs,
                news_process_configuration=news_process_configuration,
                return_clock_exposure_parameters=return_clock_exposure_parameters,
                long_variance_parameters=long_variance_parameters,
                initial_long_variance_state=initial_long_variance_state,
                observed_price_tick_size=observed_price_tick_size,
                calendar_registry_artifact=dict(calendar_registry._registry_payload),
                exchange_code=exchange_code,
            ),
            hourly_factor_contract=hourly_factor_contract,
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

        try:
            hourly_factor_contract = load_hourly_factor_contract_payload(
                require_mapping(
                    raw_payload,
                    "hourly_factor_contract",
                    "hourly_factor_contract",
                    SIMULATION_ERROR_FACTORY,
                )
            )
        except SeasonalityInputValidationError as e:
            raise _simulation_error("hourly_factor_contract", str(e)) from e

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
            hourly_factor_contract=hourly_factor_contract,
        )

    def to_payload(self) -> dict[str, Any]:
        return {
            "schema_version": self.schema_version,
            "seed": self.seed,
            "start_time_utc_nanos": self.start_time_utc_nanos,
            "horizon_interval_count": self.horizon_interval_count,
            "interval_delta": self.interval_delta,
            "model_inputs": self.model_inputs.to_payload(),
            "hourly_factor_contract": self.hourly_factor_contract.to_payload(),
        }


@dataclass(frozen=True, slots=True)
class CoarseSimulationPathRecord(ContractModel):
    schema_version: str
    interval_index: int
    interval_start_utc_nanos: int
    interval_end_utc_nanos: int
    sufficient_statistics: Any
    return_components: Any
    core_total_return: float
    observed_interval_return: float
    observed_log_price_end: float
    anchor_value_end: float
    pressure_state_end: float
    deviation_from_anchor_end: float

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

        if self.core_total_return != self.return_components.total_return:
            raise _simulation_error(
                "core_total_return",
                (
                    "must equal return_components.total_return "
                    f"({self.return_components.total_return}), found {self.core_total_return}"
                ),
            )

        MeanReversionBoundaryState(
            observed_log_price=self.observed_log_price_end,
            anchor_value=self.anchor_value_end,
            pressure_state=self.pressure_state_end,
            deviation=self.deviation_from_anchor_end,
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

        from ...sufficient_statistics import (
            SufficientStatisticsIntervalStatistics,
            SufficientStatisticsReturnComponents,
            SufficientStatisticsValidationError,
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
            return_components = SufficientStatisticsReturnComponents.from_mapping(
                require_mapping(
                    raw_payload,
                    "return_components",
                    f"{field_path}.return_components",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.return_components",
            )
        except SufficientStatisticsValidationError as e:
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
            core_total_return=require_finite_number(
                raw_payload,
                "core_total_return",
                f"{field_path}.core_total_return",
                SIMULATION_ERROR_FACTORY,
            ),
            observed_interval_return=require_finite_number(
                raw_payload,
                "observed_interval_return",
                f"{field_path}.observed_interval_return",
                SIMULATION_ERROR_FACTORY,
            ),
            observed_log_price_end=require_finite_number(
                raw_payload,
                "observed_log_price_end",
                f"{field_path}.observed_log_price_end",
                SIMULATION_ERROR_FACTORY,
            ),
            anchor_value_end=require_finite_number(
                raw_payload,
                "anchor_value_end",
                f"{field_path}.anchor_value_end",
                SIMULATION_ERROR_FACTORY,
            ),
            pressure_state_end=require_finite_number(
                raw_payload,
                "pressure_state_end",
                f"{field_path}.pressure_state_end",
                SIMULATION_ERROR_FACTORY,
            ),
            deviation_from_anchor_end=require_finite_number(
                raw_payload,
                "deviation_from_anchor_end",
                f"{field_path}.deviation_from_anchor_end",
                SIMULATION_ERROR_FACTORY,
            ),
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
    cumulative_observed_return: float
    max_abs_total_return: float
    max_abs_deviation_from_anchor: float
    terminal_observed_log_price: float
    terminal_deviation_from_anchor: float


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
        return validate_coarse_simulation_path(raw_payload)


def validate_coarse_simulation_run_request(
    raw_request: dict[str, Any] | Any,
) -> CoarseSimulationRunRequest:
    return CoarseSimulationRunRequest.from_mapping(raw_request)


def validate_coarse_simulation_path(
    raw_path: dict[str, Any] | Any,
) -> CoarseSimulationPath:
    native = native_module()
    try:
        validated_path = native.validate_coarse_simulation_path(raw_path)
    except Exception as e:
        if native_exception_matches(native, e, "SimulationExecutionError"):
            field_path, message = native_error_parts(
                str(e),
                default_field_path="coarse_simulation_path",
            )

            raise _simulation_error(field_path, message) from e

        raise

    if not isinstance(validated_path, Mapping):
        raise _simulation_error(
            "coarse_simulation_path",
            "native validation returned an invalid response payload",
        )

    return coarse_simulation_path_from_validated_payload(validated_path)


def serialize_coarse_simulation_run_request(request: CoarseSimulationRunRequest) -> str:
    return serialize_deterministic_json(request.to_payload())


def deserialize_coarse_simulation_run_request(payload_json: str) -> CoarseSimulationRunRequest:
    raw_payload = parse_json_object(payload_json, "coarse simulation run request", SIMULATION_ERROR_FACTORY)
    return validate_coarse_simulation_run_request(dict(raw_payload))


def serialize_coarse_simulation_path(path: CoarseSimulationPath) -> str:
    return serialize_deterministic_json(path)


def deserialize_coarse_simulation_path(payload_json: str) -> CoarseSimulationPath:
    raw_payload = parse_json_object(payload_json, "coarse simulation path", SIMULATION_ERROR_FACTORY)
    return validate_coarse_simulation_path(dict(raw_payload))


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
    "KnownNewsArrivalConfiguration",
    "NewsArrivalConfiguration",
    "NewsComponentMarkScaleConfiguration",
    "NewsProcessConfiguration",
    "SharedNewsMarkLawConfiguration",
    "SurpriseNewsArrivalConfiguration",
    "deserialize_coarse_simulation_path",
    "deserialize_coarse_simulation_run_request",
    "serialize_coarse_simulation_path",
    "serialize_coarse_simulation_run_request",
    "validate_coarse_simulation_path",
    "validate_coarse_simulation_run_request",
]
