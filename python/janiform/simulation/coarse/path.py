from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime
from typing import Any

from .news import (
    COARSE_SIMULATION_COMPATIBLE_INTERVAL_GRID_POLICY,
    COARSE_SIMULATION_ENVIRONMENT_METADATA_POLICY,
    COARSE_SIMULATION_INTERVAL_FACTOR_ALIGNMENT_POLICY,
    COARSE_SIMULATION_INTERVAL_IDENTIFIER_POLICY,
    COARSE_SIMULATION_INTERVAL_TIMING_POLICY,
    NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION,
    CoupledGapMarkConfiguration,
    DoubleExponentialBaseShockConfiguration,
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
from ..shared import HawkesExcitationState, SeededTickSimulationValidationError
from ..shared.errors import (
    SIMULATION_ERROR_FACTORY,
    _simulation_error,
)
from ...contracts import (
    ContractModel,
    MeanReversionBoundaryState,
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
    validate_exact_integer,
    validate_exact_number,
    validate_exact_string,
    validate_required_field_list,
)
from ..._timestamps import datetime_to_utc_nanos
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
    "core_total_return",
    "observed_interval_return",
    "observed_log_price_end",
    "anchor_value_end",
    "deviation_from_anchor_end",
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
    "cumulative_observed_return",
    "max_abs_total_return",
    "max_abs_deviation_from_anchor",
    "terminal_observed_log_price",
    "terminal_deviation_from_anchor",
)

_MEAN_REVERSION_ALIGNMENT_TOLERANCE = 1e-12

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

        from ...risk import SufficientStatisticsRiskModelInputs, SufficientStatisticsRiskValidationError

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

        runtime_interval_anchors = self.model_inputs.risk_model_inputs.mean_reversion_runtime_interval_anchors
        if len(runtime_interval_anchors.end_anchor_values) != self.horizon_interval_count:
            raise _simulation_error(
                "model_inputs.risk_model_inputs.mean_reversion_runtime_interval_anchors.end_anchor_values",
                f"must contain {self.horizon_interval_count} values",
            )

        for interval_index, observed_interval_delta in enumerate(
            runtime_interval_anchors.interval_deltas[: self.horizon_interval_count]
        ):
            if abs(observed_interval_delta - self.interval_delta) > _MEAN_REVERSION_ALIGNMENT_TOLERANCE:
                raise _simulation_error(
                    f"model_inputs.risk_model_inputs.mean_reversion_runtime_interval_anchors.interval_deltas[{interval_index}]",
                    f"must equal interval_delta ({self.interval_delta}), found {observed_interval_delta}",
                )

    @staticmethod
    def from_runtime_inputs(
        *,
        seed: int,
        start_time_utc: datetime,
        horizon_interval_count: int,
        interval_delta: float,
        risk_model_inputs: Any,
        hourly_factor_contract: HourlyFactorContract,
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

        from ...risk import (
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
    cumulative_observed_return = 0.0
    max_abs_total_return = 0.0
    max_abs_deviation_from_anchor = 0.0

    for record in records:
        cumulative_a_plus += record.sufficient_statistics.a_plus
        cumulative_a_minus += record.sufficient_statistics.a_minus
        cumulative_n_plus += record.sufficient_statistics.n_plus
        cumulative_n_minus += record.sufficient_statistics.n_minus
        cumulative_trading_time_return += record.return_components.trading_time_return
        cumulative_open_jump_return += record.return_components.open_jump_return
        cumulative_closed_gap_return += record.return_components.closed_gap_return
        cumulative_total_return += record.return_components.total_return
        cumulative_observed_return += record.observed_interval_return
        max_abs_total_return = max(max_abs_total_return, abs(record.return_components.total_return))
        max_abs_deviation_from_anchor = max(
            max_abs_deviation_from_anchor,
            abs(record.deviation_from_anchor_end),
        )

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
        cumulative_observed_return=cumulative_observed_return,
        max_abs_total_return=max_abs_total_return,
        max_abs_deviation_from_anchor=max_abs_deviation_from_anchor,
        terminal_observed_log_price=records[-1].observed_log_price_end,
        terminal_deviation_from_anchor=records[-1].deviation_from_anchor_end,
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
        "cumulative_observed_return",
        "diagnostics.cumulative_observed_return",
        expected_diagnostics.cumulative_observed_return,
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
    validate_exact_number(
        raw_payload,
        "max_abs_deviation_from_anchor",
        "diagnostics.max_abs_deviation_from_anchor",
        expected_diagnostics.max_abs_deviation_from_anchor,
        SIMULATION_ERROR_FACTORY,
        context_suffix=f"simulated_interval_count={simulated_interval_count}",
    )
    validate_exact_number(
        raw_payload,
        "terminal_observed_log_price",
        "diagnostics.terminal_observed_log_price",
        expected_diagnostics.terminal_observed_log_price,
        SIMULATION_ERROR_FACTORY,
        context_suffix=f"simulated_interval_count={simulated_interval_count}",
    )
    validate_exact_number(
        raw_payload,
        "terminal_deviation_from_anchor",
        "diagnostics.terminal_deviation_from_anchor",
        expected_diagnostics.terminal_deviation_from_anchor,
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
    "CoupledGapMarkConfiguration",
    "DoubleExponentialBaseShockConfiguration",
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
