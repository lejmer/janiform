import json
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from math import isfinite, sqrt
from typing import Any, NotRequired, TypedDict, cast

from .pricing import GammaSeverityMarkConfiguration, NigSharedPlusShiftConfiguration
from .simulation import HawkesExcitationState, HawkesKernelParameters

SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY = (
    "explicit_interval_aligned_s_h_and_nu_h_vectors_without_implicit_resampling.v1"
)
SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION = "risk.sufficient_statistics.run_request.v1"
SUFFICIENT_STATISTICS_RISK_PATH_SCHEMA_VERSION = "risk.sufficient_statistics.path.v1"
SUFFICIENT_STATISTICS_RISK_PATH_RECORD_SCHEMA_VERSION = "risk.sufficient_statistics.path_record.v1"
SUFFICIENT_STATISTICS_RISK_PATH_OUTPUT_SCHEMA_METADATA_VERSION = (
    "risk.sufficient_statistics.path_output_schema_metadata.v1"
)
SUFFICIENT_STATISTICS_RISK_PATH_PROVENANCE_SCHEMA_VERSION = (
    "risk.sufficient_statistics.path_provenance.v1"
)
SUFFICIENT_STATISTICS_RISK_PATH_DIAGNOSTICS_SCHEMA_VERSION = (
    "risk.sufficient_statistics.path_diagnostics.v1"
)
SUFFICIENT_STATISTICS_RISK_PATH_ARTIFACT_WORKFLOW_POLICY = (
    "compatible_with_calibration_and_validation_artifact_workflows.v1"
)
SUFFICIENT_STATISTICS_RISK_PATH_ENVIRONMENT_METADATA_POLICY = (
    "exclude_environment_specific_metadata.v1"
)
SUFFICIENT_STATISTICS_TRANSFORM_SLICE_SAMPLING_POLICY = (
    "counts_via_fractional_rounding_and_integrated_intensity_via_exponential_with_slice_implied_mean.v1"
)
SUFFICIENT_STATISTICS_BOUNDARY_STATE_UPDATE_POLICY = (
    "interval_boundary_drift_then_count_mapped_endogenous_jump_composition.v1"
)
SUFFICIENT_STATISTICS_INTERVAL_RETURN_GENERATION_POLICY = (
    "sample_trading_time_increment_from_sampled_statistics_via_shared_return_decomposition.v1"
)
SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_ROUTING_POLICY = (
    "open_market_state_routes_to_mt_closed_market_state_routes_to_gt.v1"
)
SUFFICIENT_STATISTICS_BATCH_SEED_PARTITION_POLICY = (
    "path_seed_partitioned_from_root_seed_and_path_index_with_shared_substream_mixer.v1"
)
SUFFICIENT_STATISTICS_RISK_PATH_REQUIRED_FIELDS: tuple[str, ...] = (
    "schema_version",
    "run_request_schema_version",
    "output_schema_metadata",
    "provenance",
    "diagnostics",
    "seed",
    "interval_delta",
    "simulated_interval_count",
    "initial_excitation_state",
    "terminal_excitation_state",
    "records",
)
SUFFICIENT_STATISTICS_RISK_PATH_REQUIRED_PROVENANCE_FIELDS: tuple[str, ...] = (
    "schema_version",
    "artifact_workflow_policy",
    "environment_metadata_policy",
    "interval_factor_policy",
    "transform_slice_sampling_policy",
    "boundary_state_update_policy",
    "interval_return_generation_policy",
    "interval_return_component_routing_policy",
    "batch_seed_partition_policy",
    "artifact_workflow_tags",
)
SUFFICIENT_STATISTICS_RISK_PATH_REQUIRED_DIAGNOSTICS_FIELDS: tuple[str, ...] = (
    "schema_version",
    "simulated_interval_count",
    "first_interval_index",
    "last_interval_index",
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
SUFFICIENT_STATISTICS_RISK_PATH_ARTIFACT_WORKFLOW_TAGS: tuple[str, ...] = (
    "calibration",
    "validation",
)
DEFAULT_INTRADAY_RISK_INTERVAL_DELTA = 1.0


class SufficientStatisticsRiskEquationSpecification(TypedDict):
    hawkes_kernel_parameters: HawkesKernelParameters
    gamma_severity_mark_configuration: GammaSeverityMarkConfiguration


class SufficientStatisticsRiskIntervalFactors(TypedDict):
    interval_delta: NotRequired[float]
    s_h: list[float]
    nu_h: list[float]


class SufficientStatisticsRiskModelInputs(TypedDict):
    equation_specification: SufficientStatisticsRiskEquationSpecification
    nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration
    initial_excitation_state: HawkesExcitationState


class SufficientStatisticsRiskRunRequest(TypedDict):
    seed: int
    horizon_interval_count: int
    interval_delta: NotRequired[float]
    model_inputs: SufficientStatisticsRiskModelInputs
    interval_factors: SufficientStatisticsRiskIntervalFactors


class SufficientStatisticsIntervalStatistics(TypedDict):
    a_plus: float
    a_minus: float
    n_plus: int
    n_minus: int


class SufficientStatisticsRiskReturnComponents(TypedDict):
    buy_side_return: float
    sell_side_return: float
    trading_time_return: float
    open_jump_return: float
    closed_gap_return: float
    total_return: float


class SufficientStatisticsRiskPathRecord(TypedDict):
    interval_index: int
    sufficient_statistics: SufficientStatisticsIntervalStatistics
    return_components: SufficientStatisticsRiskReturnComponents


class SufficientStatisticsRiskPathOutputSchemaMetadata(TypedDict):
    schema_version: str
    provenance_schema_version: str
    diagnostics_schema_version: str
    artifact_workflow_policy: str
    environment_metadata_policy: str
    required_path_fields: list[str]
    required_provenance_fields: list[str]
    required_diagnostics_fields: list[str]


class SufficientStatisticsRiskPathProvenance(TypedDict):
    schema_version: str
    artifact_workflow_policy: str
    environment_metadata_policy: str
    interval_factor_policy: str
    transform_slice_sampling_policy: str
    boundary_state_update_policy: str
    interval_return_generation_policy: str
    interval_return_component_routing_policy: str
    batch_seed_partition_policy: str
    artifact_workflow_tags: list[str]


class SufficientStatisticsRiskPathDiagnostics(TypedDict):
    schema_version: str
    simulated_interval_count: int
    first_interval_index: int | None
    last_interval_index: int | None
    cumulative_a_plus: float
    cumulative_a_minus: float
    cumulative_n_plus: int
    cumulative_n_minus: int
    cumulative_trading_time_return: float
    cumulative_open_jump_return: float
    cumulative_closed_gap_return: float
    cumulative_total_return: float
    max_abs_total_return: float


class SufficientStatisticsRiskPath(TypedDict):
    schema_version: str
    run_request_schema_version: str
    output_schema_metadata: SufficientStatisticsRiskPathOutputSchemaMetadata
    provenance: SufficientStatisticsRiskPathProvenance
    diagnostics: SufficientStatisticsRiskPathDiagnostics
    seed: int
    interval_delta: float
    simulated_interval_count: int
    initial_excitation_state: HawkesExcitationState
    terminal_excitation_state: HawkesExcitationState
    records: list[SufficientStatisticsRiskPathRecord]


@dataclass(frozen=True, slots=True)
class SufficientStatisticsRiskValidationFailure:
    field_path: str
    message: str


class SufficientStatisticsRiskValidationError(ValueError):
    def __init__(self, failure: SufficientStatisticsRiskValidationFailure) -> None:
        self.failure: SufficientStatisticsRiskValidationFailure = failure
        super().__init__(f"{failure.field_path}: {failure.message}")


def validate_sufficient_statistics_risk_run_request(
    raw_request: Mapping[str, Any],
) -> SufficientStatisticsRiskRunRequest:
    seed: int = _require_non_negative_integer(raw_request, "seed", "seed")
    horizon_interval_count: int = _require_positive_integer(
        raw_request,
        "horizon_interval_count",
        "horizon_interval_count",
    )
    interval_delta: float = (
        _require_positive_finite_number(raw_request, "interval_delta", "interval_delta")
        if "interval_delta" in raw_request
        else DEFAULT_INTRADAY_RISK_INTERVAL_DELTA
    )
    model_inputs: SufficientStatisticsRiskModelInputs = _validate_model_inputs(raw_request)
    interval_factors: SufficientStatisticsRiskIntervalFactors = _validate_interval_factors(
        raw_request,
        "interval_factors",
        "interval_factors",
        requested_interval_delta=interval_delta,
    )

    available_interval_count: int = len(interval_factors["s_h"])
    if horizon_interval_count > available_interval_count:
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path="horizon_interval_count",
                message=(
                    f"must not exceed available interval factors count ({available_interval_count})"
                ),
            )
        )

    return {
        "seed": seed,
        "horizon_interval_count": horizon_interval_count,
        "interval_delta": interval_delta,
        "model_inputs": model_inputs,
        "interval_factors": interval_factors,
    }


def validate_sufficient_statistics_risk_path(
    raw_path: Mapping[str, Any],
) -> SufficientStatisticsRiskPath:
    seed: int = _require_non_negative_integer(raw_path, "seed", "seed")
    interval_delta: float = _require_positive_finite_number(
        raw_path,
        "interval_delta",
        "interval_delta",
    )
    simulated_interval_count: int = _require_positive_integer(
        raw_path,
        "simulated_interval_count",
        "simulated_interval_count",
    )
    initial_excitation_state: HawkesExcitationState = _validate_excitation_state(
        _require_mapping(raw_path, "initial_excitation_state", "initial_excitation_state"),
        "initial_excitation_state",
    )
    terminal_excitation_state: HawkesExcitationState = _validate_excitation_state(
        _require_mapping(raw_path, "terminal_excitation_state", "terminal_excitation_state"),
        "terminal_excitation_state",
    )
    records = _validate_path_records(raw_path, "records", simulated_interval_count)
    output_schema_metadata = _build_risk_path_output_schema_metadata()
    provenance = _build_risk_path_provenance()
    diagnostics = _build_risk_path_diagnostics(records, simulated_interval_count)

    if "schema_version" in raw_path:
        _validate_exact_string(
            raw_path,
            "schema_version",
            "schema_version",
            SUFFICIENT_STATISTICS_RISK_PATH_SCHEMA_VERSION,
        )
    if "run_request_schema_version" in raw_path:
        _validate_exact_string(
            raw_path,
            "run_request_schema_version",
            "run_request_schema_version",
            SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION,
        )
    if "output_schema_metadata" in raw_path:
        _validate_output_schema_metadata(
            _require_mapping(raw_path, "output_schema_metadata", "output_schema_metadata"),
            output_schema_metadata,
            "output_schema_metadata",
        )
    if "provenance" in raw_path:
        _validate_path_provenance(
            _require_mapping(raw_path, "provenance", "provenance"),
            provenance,
            "provenance",
        )
    if "diagnostics" in raw_path:
        _validate_path_diagnostics(
            _require_mapping(raw_path, "diagnostics", "diagnostics"),
            diagnostics,
            "diagnostics",
            simulated_interval_count,
        )

    return {
        "schema_version": SUFFICIENT_STATISTICS_RISK_PATH_SCHEMA_VERSION,
        "run_request_schema_version": SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION,
        "output_schema_metadata": output_schema_metadata,
        "provenance": provenance,
        "diagnostics": diagnostics,
        "seed": seed,
        "interval_delta": interval_delta,
        "simulated_interval_count": simulated_interval_count,
        "initial_excitation_state": initial_excitation_state,
        "terminal_excitation_state": terminal_excitation_state,
        "records": records,
    }


def serialize_sufficient_statistics_risk_run_request(request: Mapping[str, Any]) -> str:
    validated_request: SufficientStatisticsRiskRunRequest = (
        validate_sufficient_statistics_risk_run_request(request)
    )
    return json.dumps(validated_request, sort_keys=True)


def deserialize_sufficient_statistics_risk_run_request(
    payload_json: str,
) -> SufficientStatisticsRiskRunRequest:
    raw_payload: Mapping[str, Any] = _parse_json_object(payload_json, "risk run request")
    return validate_sufficient_statistics_risk_run_request(raw_payload)


def serialize_sufficient_statistics_risk_path(path: Mapping[str, Any]) -> str:
    validated_path: SufficientStatisticsRiskPath = validate_sufficient_statistics_risk_path(path)
    return json.dumps(validated_path, sort_keys=True)


def deserialize_sufficient_statistics_risk_path(payload_json: str) -> SufficientStatisticsRiskPath:
    raw_payload: Mapping[str, Any] = _parse_json_object(payload_json, "risk path")
    return validate_sufficient_statistics_risk_path(raw_payload)


def _build_risk_path_output_schema_metadata() -> SufficientStatisticsRiskPathOutputSchemaMetadata:
    return {
        "schema_version": SUFFICIENT_STATISTICS_RISK_PATH_OUTPUT_SCHEMA_METADATA_VERSION,
        "provenance_schema_version": SUFFICIENT_STATISTICS_RISK_PATH_PROVENANCE_SCHEMA_VERSION,
        "diagnostics_schema_version": SUFFICIENT_STATISTICS_RISK_PATH_DIAGNOSTICS_SCHEMA_VERSION,
        "artifact_workflow_policy": SUFFICIENT_STATISTICS_RISK_PATH_ARTIFACT_WORKFLOW_POLICY,
        "environment_metadata_policy": SUFFICIENT_STATISTICS_RISK_PATH_ENVIRONMENT_METADATA_POLICY,
        "required_path_fields": list(SUFFICIENT_STATISTICS_RISK_PATH_REQUIRED_FIELDS),
        "required_provenance_fields": list(SUFFICIENT_STATISTICS_RISK_PATH_REQUIRED_PROVENANCE_FIELDS),
        "required_diagnostics_fields": list(SUFFICIENT_STATISTICS_RISK_PATH_REQUIRED_DIAGNOSTICS_FIELDS),
    }


def _build_risk_path_provenance() -> SufficientStatisticsRiskPathProvenance:
    return {
        "schema_version": SUFFICIENT_STATISTICS_RISK_PATH_PROVENANCE_SCHEMA_VERSION,
        "artifact_workflow_policy": SUFFICIENT_STATISTICS_RISK_PATH_ARTIFACT_WORKFLOW_POLICY,
        "environment_metadata_policy": SUFFICIENT_STATISTICS_RISK_PATH_ENVIRONMENT_METADATA_POLICY,
        "interval_factor_policy": SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY,
        "transform_slice_sampling_policy": SUFFICIENT_STATISTICS_TRANSFORM_SLICE_SAMPLING_POLICY,
        "boundary_state_update_policy": SUFFICIENT_STATISTICS_BOUNDARY_STATE_UPDATE_POLICY,
        "interval_return_generation_policy": SUFFICIENT_STATISTICS_INTERVAL_RETURN_GENERATION_POLICY,
        "interval_return_component_routing_policy": (
            SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_ROUTING_POLICY
        ),
        "batch_seed_partition_policy": SUFFICIENT_STATISTICS_BATCH_SEED_PARTITION_POLICY,
        "artifact_workflow_tags": list(SUFFICIENT_STATISTICS_RISK_PATH_ARTIFACT_WORKFLOW_TAGS),
    }


def _build_risk_path_diagnostics(
    records: Sequence[SufficientStatisticsRiskPathRecord],
    simulated_interval_count: int,
) -> SufficientStatisticsRiskPathDiagnostics:
    cumulative_a_plus: float = 0.0
    cumulative_a_minus: float = 0.0
    cumulative_n_plus: int = 0
    cumulative_n_minus: int = 0
    cumulative_trading_time_return: float = 0.0
    cumulative_open_jump_return: float = 0.0
    cumulative_closed_gap_return: float = 0.0
    cumulative_total_return: float = 0.0
    max_abs_total_return: float = 0.0

    for record in records:
        sufficient_statistics = record["sufficient_statistics"]
        return_components = record["return_components"]
        cumulative_a_plus += sufficient_statistics["a_plus"]
        cumulative_a_minus += sufficient_statistics["a_minus"]
        cumulative_n_plus += sufficient_statistics["n_plus"]
        cumulative_n_minus += sufficient_statistics["n_minus"]
        cumulative_trading_time_return += return_components["trading_time_return"]
        cumulative_open_jump_return += return_components["open_jump_return"]
        cumulative_closed_gap_return += return_components["closed_gap_return"]
        cumulative_total_return += return_components["total_return"]
        max_abs_total_return = max(max_abs_total_return, abs(return_components["total_return"]))

    first_interval_index: int | None = records[0]["interval_index"] if records else None
    last_interval_index: int | None = records[-1]["interval_index"] if records else None

    return {
        "schema_version": SUFFICIENT_STATISTICS_RISK_PATH_DIAGNOSTICS_SCHEMA_VERSION,
        "simulated_interval_count": simulated_interval_count,
        "first_interval_index": first_interval_index,
        "last_interval_index": last_interval_index,
        "cumulative_a_plus": cumulative_a_plus,
        "cumulative_a_minus": cumulative_a_minus,
        "cumulative_n_plus": cumulative_n_plus,
        "cumulative_n_minus": cumulative_n_minus,
        "cumulative_trading_time_return": cumulative_trading_time_return,
        "cumulative_open_jump_return": cumulative_open_jump_return,
        "cumulative_closed_gap_return": cumulative_closed_gap_return,
        "cumulative_total_return": cumulative_total_return,
        "max_abs_total_return": max_abs_total_return,
    }


def _validate_output_schema_metadata(
    raw_metadata: Mapping[str, Any],
    expected_metadata: SufficientStatisticsRiskPathOutputSchemaMetadata,
    field_path: str,
) -> None:
    _validate_exact_string(
        raw_metadata,
        "schema_version",
        f"{field_path}.schema_version",
        expected_metadata["schema_version"],
    )
    _validate_exact_string(
        raw_metadata,
        "provenance_schema_version",
        f"{field_path}.provenance_schema_version",
        expected_metadata["provenance_schema_version"],
    )
    _validate_exact_string(
        raw_metadata,
        "diagnostics_schema_version",
        f"{field_path}.diagnostics_schema_version",
        expected_metadata["diagnostics_schema_version"],
    )
    _validate_exact_string(
        raw_metadata,
        "artifact_workflow_policy",
        f"{field_path}.artifact_workflow_policy",
        expected_metadata["artifact_workflow_policy"],
    )
    _validate_exact_string(
        raw_metadata,
        "environment_metadata_policy",
        f"{field_path}.environment_metadata_policy",
        expected_metadata["environment_metadata_policy"],
    )
    _validate_required_field_list(
        raw_metadata,
        "required_path_fields",
        f"{field_path}.required_path_fields",
        expected_metadata["required_path_fields"],
    )
    _validate_required_field_list(
        raw_metadata,
        "required_provenance_fields",
        f"{field_path}.required_provenance_fields",
        expected_metadata["required_provenance_fields"],
    )
    _validate_required_field_list(
        raw_metadata,
        "required_diagnostics_fields",
        f"{field_path}.required_diagnostics_fields",
        expected_metadata["required_diagnostics_fields"],
    )


def _validate_path_provenance(
    raw_provenance: Mapping[str, Any],
    expected_provenance: SufficientStatisticsRiskPathProvenance,
    field_path: str,
) -> None:
    _validate_exact_string(
        raw_provenance,
        "schema_version",
        f"{field_path}.schema_version",
        expected_provenance["schema_version"],
    )
    _validate_exact_string(
        raw_provenance,
        "artifact_workflow_policy",
        f"{field_path}.artifact_workflow_policy",
        expected_provenance["artifact_workflow_policy"],
    )
    _validate_exact_string(
        raw_provenance,
        "environment_metadata_policy",
        f"{field_path}.environment_metadata_policy",
        expected_provenance["environment_metadata_policy"],
    )
    _validate_exact_string(
        raw_provenance,
        "interval_factor_policy",
        f"{field_path}.interval_factor_policy",
        expected_provenance["interval_factor_policy"],
    )
    _validate_exact_string(
        raw_provenance,
        "transform_slice_sampling_policy",
        f"{field_path}.transform_slice_sampling_policy",
        expected_provenance["transform_slice_sampling_policy"],
    )
    _validate_exact_string(
        raw_provenance,
        "boundary_state_update_policy",
        f"{field_path}.boundary_state_update_policy",
        expected_provenance["boundary_state_update_policy"],
    )
    _validate_exact_string(
        raw_provenance,
        "interval_return_generation_policy",
        f"{field_path}.interval_return_generation_policy",
        expected_provenance["interval_return_generation_policy"],
    )
    _validate_exact_string(
        raw_provenance,
        "interval_return_component_routing_policy",
        f"{field_path}.interval_return_component_routing_policy",
        expected_provenance["interval_return_component_routing_policy"],
    )
    _validate_exact_string(
        raw_provenance,
        "batch_seed_partition_policy",
        f"{field_path}.batch_seed_partition_policy",
        expected_provenance["batch_seed_partition_policy"],
    )
    _validate_required_field_list(
        raw_provenance,
        "artifact_workflow_tags",
        f"{field_path}.artifact_workflow_tags",
        expected_provenance["artifact_workflow_tags"],
    )


def _validate_path_diagnostics(
    raw_diagnostics: Mapping[str, Any],
    expected_diagnostics: SufficientStatisticsRiskPathDiagnostics,
    field_path: str,
    simulated_interval_count: int,
) -> None:
    _validate_exact_string(
        raw_diagnostics,
        "schema_version",
        f"{field_path}.schema_version",
        expected_diagnostics["schema_version"],
    )
    _validate_exact_integer(
        raw_diagnostics,
        "simulated_interval_count",
        f"{field_path}.simulated_interval_count",
        expected_diagnostics["simulated_interval_count"],
    )
    _validate_exact_optional_integer(
        raw_diagnostics,
        "first_interval_index",
        f"{field_path}.first_interval_index",
        expected_diagnostics["first_interval_index"],
    )
    _validate_exact_optional_integer(
        raw_diagnostics,
        "last_interval_index",
        f"{field_path}.last_interval_index",
        expected_diagnostics["last_interval_index"],
    )
    _validate_exact_number(
        raw_diagnostics,
        "cumulative_a_plus",
        f"{field_path}.cumulative_a_plus",
        expected_diagnostics["cumulative_a_plus"],
        simulated_interval_count,
    )
    _validate_exact_number(
        raw_diagnostics,
        "cumulative_a_minus",
        f"{field_path}.cumulative_a_minus",
        expected_diagnostics["cumulative_a_minus"],
        simulated_interval_count,
    )
    _validate_exact_integer(
        raw_diagnostics,
        "cumulative_n_plus",
        f"{field_path}.cumulative_n_plus",
        expected_diagnostics["cumulative_n_plus"],
        simulated_interval_count,
    )
    _validate_exact_integer(
        raw_diagnostics,
        "cumulative_n_minus",
        f"{field_path}.cumulative_n_minus",
        expected_diagnostics["cumulative_n_minus"],
        simulated_interval_count,
    )
    _validate_exact_number(
        raw_diagnostics,
        "cumulative_trading_time_return",
        f"{field_path}.cumulative_trading_time_return",
        expected_diagnostics["cumulative_trading_time_return"],
        simulated_interval_count,
    )
    _validate_exact_number(
        raw_diagnostics,
        "cumulative_open_jump_return",
        f"{field_path}.cumulative_open_jump_return",
        expected_diagnostics["cumulative_open_jump_return"],
        simulated_interval_count,
    )
    _validate_exact_number(
        raw_diagnostics,
        "cumulative_closed_gap_return",
        f"{field_path}.cumulative_closed_gap_return",
        expected_diagnostics["cumulative_closed_gap_return"],
        simulated_interval_count,
    )
    _validate_exact_number(
        raw_diagnostics,
        "cumulative_total_return",
        f"{field_path}.cumulative_total_return",
        expected_diagnostics["cumulative_total_return"],
        simulated_interval_count,
    )
    _validate_exact_number(
        raw_diagnostics,
        "max_abs_total_return",
        f"{field_path}.max_abs_total_return",
        expected_diagnostics["max_abs_total_return"],
        simulated_interval_count,
    )


def _validate_model_inputs(raw_request: Mapping[str, Any]) -> SufficientStatisticsRiskModelInputs:
    raw_model_inputs: Mapping[str, Any] = _require_mapping(
        raw_request,
        "model_inputs",
        "model_inputs",
    )
    raw_equation_specification: Mapping[str, Any] = _require_mapping(
        raw_model_inputs,
        "equation_specification",
        "model_inputs.equation_specification",
    )
    raw_kernel_parameters: Mapping[str, Any] = _require_mapping(
        raw_equation_specification,
        "hawkes_kernel_parameters",
        "model_inputs.equation_specification.hawkes_kernel_parameters",
    )
    raw_gamma_configuration: Mapping[str, Any] = _require_mapping(
        raw_equation_specification,
        "gamma_severity_mark_configuration",
        "model_inputs.equation_specification.gamma_severity_mark_configuration",
    )
    raw_nig_configuration: Mapping[str, Any] = _require_mapping(
        raw_model_inputs,
        "nig_shared_plus_shift_configuration",
        "model_inputs.nig_shared_plus_shift_configuration",
    )
    raw_initial_state: Mapping[str, Any] = _require_mapping(
        raw_model_inputs,
        "initial_excitation_state",
        "model_inputs.initial_excitation_state",
    )

    kernel_parameters: HawkesKernelParameters = _validate_hawkes_kernel_parameters(
        raw_kernel_parameters,
        "model_inputs.equation_specification.hawkes_kernel_parameters",
    )
    gamma_configuration: GammaSeverityMarkConfiguration = _validate_gamma_configuration(
        raw_gamma_configuration,
        "model_inputs.equation_specification.gamma_severity_mark_configuration",
    )
    nig_configuration: NigSharedPlusShiftConfiguration = _validate_nig_configuration(
        raw_nig_configuration,
        "model_inputs.nig_shared_plus_shift_configuration",
    )
    initial_excitation_state: HawkesExcitationState = _validate_excitation_state(
        raw_initial_state,
        "model_inputs.initial_excitation_state",
    )

    return {
        "equation_specification": {
            "hawkes_kernel_parameters": kernel_parameters,
            "gamma_severity_mark_configuration": gamma_configuration,
        },
        "nig_shared_plus_shift_configuration": nig_configuration,
        "initial_excitation_state": initial_excitation_state,
    }


def _validate_interval_factors(
    raw_payload: Mapping[str, Any],
    field_name: str,
    field_path: str,
    *,
    requested_interval_delta: float,
) -> SufficientStatisticsRiskIntervalFactors:
    raw_interval_factors: Mapping[str, Any] = _require_mapping(raw_payload, field_name, field_path)
    raw_s_h: Sequence[object] = _require_sequence(raw_interval_factors, "s_h", f"{field_path}.s_h")
    raw_nu_h: Sequence[object] = _require_sequence(raw_interval_factors, "nu_h", f"{field_path}.nu_h")
    if len(raw_s_h) == 0:
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=f"{field_path}.s_h",
                message="must contain at least one interval factor",
            )
        )
    if len(raw_nu_h) != len(raw_s_h):
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=f"{field_path}.nu_h",
                message=f"must have the same length as {field_path}.s_h ({len(raw_s_h)})",
            )
        )

    s_h: list[float] = []
    nu_h: list[float] = []
    for interval_index, raw_value in enumerate(raw_s_h):
        value: float = _require_non_negative_finite_scalar(
            raw_value,
            field_path=f"{field_path}.s_h[{interval_index}]",
        )
        s_h.append(value)
    for interval_index, raw_value in enumerate(raw_nu_h):
        value = _require_non_negative_finite_scalar(
            raw_value,
            field_path=f"{field_path}.nu_h[{interval_index}]",
        )
        nu_h.append(value)

    interval_factor_delta: float = (
        _require_positive_finite_number(
            raw_interval_factors,
            "interval_delta",
            f"{field_path}.interval_delta",
        )
        if "interval_delta" in raw_interval_factors
        else DEFAULT_INTRADAY_RISK_INTERVAL_DELTA
    )
    if interval_factor_delta != requested_interval_delta:
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=f"{field_path}.interval_delta",
                message=(
                    "must match interval_delta exactly under explicit factor alignment "
                    f"without implicit resampling (interval_delta={requested_interval_delta}, "
                    f"interval_factors.interval_delta={interval_factor_delta})"
                ),
            )
        )

    return {
        "interval_delta": interval_factor_delta,
        "s_h": s_h,
        "nu_h": nu_h,
    }


def _validate_hawkes_kernel_parameters(
    raw_kernel_parameters: Mapping[str, Any],
    field_path: str,
) -> HawkesKernelParameters:
    raw_alpha: Mapping[str, Any] = _require_mapping(raw_kernel_parameters, "alpha", f"{field_path}.alpha")
    raw_rho: Mapping[str, Any] = _require_mapping(raw_kernel_parameters, "rho", f"{field_path}.rho")

    lambda_bar: float = _require_positive_finite_number(
        raw_kernel_parameters,
        "lambda_bar",
        f"{field_path}.lambda_bar",
    )
    beta: float = _require_positive_finite_number(
        raw_kernel_parameters,
        "beta",
        f"{field_path}.beta",
    )
    alpha = _validate_kernel_loadings(raw_alpha, f"{field_path}.alpha")
    rho = _validate_kernel_loadings(raw_rho, f"{field_path}.rho")

    spectral_radius: float = _compute_spectral_radius(alpha, beta)
    if spectral_radius >= 1.0:
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=f"{field_path}.alpha",
                message=f"spectral radius(alpha / beta) must be < 1.0, found {spectral_radius}",
            )
        )

    return {
        "lambda_bar": lambda_bar,
        "beta": beta,
        "alpha": alpha,
        "rho": rho,
    }


def _validate_kernel_loadings(
    raw_loadings: Mapping[str, Any],
    field_path: str,
) -> dict[str, float]:
    plus_plus: float = _require_non_negative_finite_number(raw_loadings, "plus_plus", f"{field_path}.plus_plus")
    plus_minus: float = _require_non_negative_finite_number(raw_loadings, "plus_minus", f"{field_path}.plus_minus")
    minus_plus: float = _require_non_negative_finite_number(raw_loadings, "minus_plus", f"{field_path}.minus_plus")
    minus_minus: float = _require_non_negative_finite_number(raw_loadings, "minus_minus", f"{field_path}.minus_minus")
    return {
        "plus_plus": plus_plus,
        "plus_minus": plus_minus,
        "minus_plus": minus_plus,
        "minus_minus": minus_minus,
    }


def _validate_gamma_configuration(
    raw_gamma_configuration: Mapping[str, Any],
    field_path: str,
) -> GammaSeverityMarkConfiguration:
    shape_k: float = _require_positive_finite_number(raw_gamma_configuration, "shape_k", f"{field_path}.shape_k")
    scale_theta: float = _require_positive_finite_number(
        raw_gamma_configuration,
        "scale_theta",
        f"{field_path}.scale_theta",
    )
    return {
        "shape_k": shape_k,
        "scale_theta": scale_theta,
    }


def _validate_nig_configuration(
    raw_nig_configuration: Mapping[str, Any],
    field_path: str,
) -> NigSharedPlusShiftConfiguration:
    alpha: float = _require_positive_finite_number(raw_nig_configuration, "alpha", f"{field_path}.alpha")
    beta_0: float = _require_finite_number(raw_nig_configuration, "beta_0", f"{field_path}.beta_0")
    delta: float = _require_positive_finite_number(raw_nig_configuration, "delta", f"{field_path}.delta")
    mu: float = _require_finite_number(raw_nig_configuration, "mu", f"{field_path}.mu")
    delta_beta: float = _require_finite_number(raw_nig_configuration, "delta_beta", f"{field_path}.delta_beta")
    if delta_beta >= 0.0:
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=f"{field_path}.delta_beta",
                message="must be strictly negative",
            )
        )

    if abs(beta_0) >= alpha:
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=f"{field_path}.beta_0",
                message="must satisfy abs(beta_0) < alpha",
            )
        )

    derived_sell_beta: float = beta_0 + delta_beta
    if abs(derived_sell_beta) >= alpha:
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=f"{field_path}.delta_beta",
                message="must satisfy abs(beta_0 + delta_beta) < alpha for sell-side parameters",
            )
        )

    return {
        "alpha": alpha,
        "beta_0": beta_0,
        "delta": delta,
        "mu": mu,
        "delta_beta": delta_beta,
    }


def _validate_excitation_state(
    raw_state: Mapping[str, Any],
    field_path: str,
) -> HawkesExcitationState:
    y_plus_plus: float = _require_non_negative_finite_number(raw_state, "y_plus_plus", f"{field_path}.y_plus_plus")
    y_plus_minus: float = _require_non_negative_finite_number(raw_state, "y_plus_minus", f"{field_path}.y_plus_minus")
    y_minus_plus: float = _require_non_negative_finite_number(raw_state, "y_minus_plus", f"{field_path}.y_minus_plus")
    y_minus_minus: float = _require_non_negative_finite_number(
        raw_state,
        "y_minus_minus",
        f"{field_path}.y_minus_minus",
    )
    return {
        "y_plus_plus": y_plus_plus,
        "y_plus_minus": y_plus_minus,
        "y_minus_plus": y_minus_plus,
        "y_minus_minus": y_minus_minus,
    }


def _validate_path_records(
    raw_payload: Mapping[str, Any],
    field_name: str,
    simulated_interval_count: int,
) -> list[SufficientStatisticsRiskPathRecord]:
    records = _require_sequence(raw_payload, field_name, field_name)
    if len(records) != simulated_interval_count:
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=field_name,
                message=f"must contain exactly {simulated_interval_count} records",
            )
        )

    validated_records: list[SufficientStatisticsRiskPathRecord] = []
    for record_position, raw_record in enumerate(records):
        if not isinstance(raw_record, Mapping):
            raise SufficientStatisticsRiskValidationError(
                SufficientStatisticsRiskValidationFailure(
                    field_path=f"{field_name}[{record_position}]",
                    message="must be a mapping",
                )
            )
        interval_index: int = _require_non_negative_integer(
            raw_record,
            "interval_index",
            f"{field_name}[{record_position}].interval_index",
        )
        if interval_index != record_position:
            raise SufficientStatisticsRiskValidationError(
                SufficientStatisticsRiskValidationFailure(
                    field_path=f"{field_name}[{record_position}].interval_index",
                    message=f"must equal {record_position}",
                )
            )
        if "schema_version" in raw_record:
            _validate_exact_string(
                raw_record,
                "schema_version",
                f"{field_name}[{record_position}].schema_version",
                SUFFICIENT_STATISTICS_RISK_PATH_RECORD_SCHEMA_VERSION,
            )

        sufficient_statistics = _validate_sufficient_statistics(
            _require_mapping(
                raw_record,
                "sufficient_statistics",
                f"{field_name}[{record_position}].sufficient_statistics",
            ),
            f"{field_name}[{record_position}].sufficient_statistics",
        )
        return_components = _validate_return_components(
            _require_mapping(
                raw_record,
                "return_components",
                f"{field_name}[{record_position}].return_components",
            ),
            f"{field_name}[{record_position}].return_components",
        )

        validated_records.append(
            {
                "interval_index": interval_index,
                "sufficient_statistics": sufficient_statistics,
                "return_components": return_components,
            }
        )

    return validated_records


def _validate_sufficient_statistics(
    raw_statistics: Mapping[str, Any],
    field_path: str,
) -> SufficientStatisticsIntervalStatistics:
    a_plus = _require_non_negative_finite_number(raw_statistics, "a_plus", f"{field_path}.a_plus")
    a_minus = _require_non_negative_finite_number(raw_statistics, "a_minus", f"{field_path}.a_minus")
    n_plus = _require_non_negative_integer(raw_statistics, "n_plus", f"{field_path}.n_plus")
    n_minus = _require_non_negative_integer(raw_statistics, "n_minus", f"{field_path}.n_minus")
    return {
        "a_plus": a_plus,
        "a_minus": a_minus,
        "n_plus": n_plus,
        "n_minus": n_minus,
    }


def _validate_return_components(
    raw_components: Mapping[str, Any],
    field_path: str,
) -> SufficientStatisticsRiskReturnComponents:
    buy_side_return = _require_finite_number(raw_components, "buy_side_return", f"{field_path}.buy_side_return")
    sell_side_return = _require_finite_number(raw_components, "sell_side_return", f"{field_path}.sell_side_return")
    trading_time_return = _require_finite_number(
        raw_components,
        "trading_time_return",
        f"{field_path}.trading_time_return",
    )
    open_jump_return = _require_finite_number(
        raw_components,
        "open_jump_return",
        f"{field_path}.open_jump_return",
    )
    closed_gap_return = _require_finite_number(
        raw_components,
        "closed_gap_return",
        f"{field_path}.closed_gap_return",
    )
    total_return = _require_finite_number(raw_components, "total_return", f"{field_path}.total_return")

    return {
        "buy_side_return": buy_side_return,
        "sell_side_return": sell_side_return,
        "trading_time_return": trading_time_return,
        "open_jump_return": open_jump_return,
        "closed_gap_return": closed_gap_return,
        "total_return": total_return,
    }


def _validate_exact_string(
    raw_mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    expected_value: str,
) -> None:
    observed_value = _require_string(raw_mapping, field_name, field_path)
    if observed_value != expected_value:
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=field_path,
                message=f"must be {expected_value}, found {observed_value}",
            )
        )


def _validate_required_field_list(
    raw_mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    expected_field_names: Sequence[str],
) -> None:
    raw_values = _require_sequence(raw_mapping, field_name, field_path)
    if len(raw_values) != len(expected_field_names):
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=field_path,
                message=f"must contain exactly {len(expected_field_names)} entries",
            )
        )

    for field_index, expected_field_name in enumerate(expected_field_names):
        observed_field_name = _require_string(
            {"value": raw_values[field_index]},
            "value",
            f"{field_path}[{field_index}]",
        )
        if observed_field_name != expected_field_name:
            raise SufficientStatisticsRiskValidationError(
                SufficientStatisticsRiskValidationFailure(
                    field_path=f"{field_path}[{field_index}]",
                    message=f"must be {expected_field_name}, found {observed_field_name}",
                )
            )


def _validate_exact_integer(
    raw_mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    expected_value: int,
    simulated_interval_count: int | None = None,
) -> None:
    observed_value = _require_integer(raw_mapping, field_name, field_path)
    if observed_value != expected_value:
        context_suffix = (
            f" (simulated_interval_count={simulated_interval_count})"
            if simulated_interval_count is not None
            else ""
        )

        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=field_path,
                message=f"must be {expected_value}, found {observed_value}{context_suffix}",
            )
        )


def _validate_exact_optional_integer(
    raw_mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    expected_value: int | None,
) -> None:
    if field_name not in raw_mapping:
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(field_path=field_path, message="is required")
        )

    raw_value = raw_mapping[field_name]

    if raw_value is None:
        observed_value = None
    elif isinstance(raw_value, int) and not isinstance(raw_value, bool):
        observed_value = raw_value
    else:
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=field_path,
                message="must be an integer or null",
            )
        )

    if observed_value != expected_value:
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=field_path,
                message=f"must be {expected_value}, found {observed_value}",
            )
        )


def _validate_exact_number(
    raw_mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    expected_value: float,
    simulated_interval_count: int,
) -> None:
    observed_value = _require_finite_number(raw_mapping, field_name, field_path)
    if observed_value != expected_value:
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=field_path,
                message=(
                    f"must be {expected_value}, found {observed_value} "
                    f"(simulated_interval_count={simulated_interval_count})"
                ),
            )
        )


def _compute_spectral_radius(alpha: Mapping[str, float], beta: float) -> float:
    normalized_plus_plus: float = alpha["plus_plus"] / beta
    normalized_plus_minus: float = alpha["plus_minus"] / beta
    normalized_minus_plus: float = alpha["minus_plus"] / beta
    normalized_minus_minus: float = alpha["minus_minus"] / beta

    trace: float = normalized_plus_plus + normalized_minus_minus
    determinant: float = (
        normalized_plus_plus * normalized_minus_minus
        - normalized_plus_minus * normalized_minus_plus
    )
    discriminant: float = trace * trace - 4.0 * determinant
    clipped_discriminant: float = discriminant if discriminant > 0.0 else 0.0
    largest_eigenvalue: float = (trace + sqrt(clipped_discriminant)) / 2.0
    smallest_eigenvalue: float = (trace - sqrt(clipped_discriminant)) / 2.0

    return max(abs(largest_eigenvalue), abs(smallest_eigenvalue))


def _parse_json_object(payload_json: str, context_label: str) -> Mapping[str, Any]:
    try:
        payload = json.loads(payload_json)
    except json.JSONDecodeError as e:
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=context_label,
                message=f"must be valid JSON: {e.msg}",
            )
        ) from e

    if not isinstance(payload, dict):
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=context_label,
                message="must decode to a JSON object",
            )
        )

    return cast(Mapping[str, Any], payload)


def _require_mapping(raw_mapping: Mapping[str, Any], field_name: str, field_path: str) -> Mapping[str, Any]:
    if field_name not in raw_mapping:
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(field_path=field_path, message="is required")
        )

    value = raw_mapping[field_name]
    if not isinstance(value, Mapping):
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=field_path,
                message="must be a mapping",
            )
        )

    return cast(Mapping[str, Any], value)


def _require_string(raw_mapping: Mapping[str, Any], field_name: str, field_path: str) -> str:
    if field_name not in raw_mapping:
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(field_path=field_path, message="is required")
        )

    value = raw_mapping[field_name]
    if not isinstance(value, str):
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=field_path,
                message="must be a string",
            )
        )

    return value


def _require_sequence(raw_mapping: Mapping[str, Any], field_name: str, field_path: str) -> Sequence[object]:
    if field_name not in raw_mapping:
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(field_path=field_path, message="is required")
        )

    value = raw_mapping[field_name]
    if isinstance(value, str) or not isinstance(value, Sequence):
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=field_path,
                message="must be a sequence",
            )
        )

    return cast(Sequence[object], value)


def _require_non_negative_integer(
    raw_mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
) -> int:
    value = _require_integer(raw_mapping, field_name, field_path)
    if value < 0:
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=field_path,
                message="must be non-negative",
            )
        )

    return value


def _require_positive_integer(raw_mapping: Mapping[str, Any], field_name: str, field_path: str) -> int:
    value = _require_integer(raw_mapping, field_name, field_path)
    if value <= 0:
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=field_path,
                message="must be strictly positive",
            )
        )

    return value


def _require_integer(raw_mapping: Mapping[str, Any], field_name: str, field_path: str) -> int:
    if field_name not in raw_mapping:
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(field_path=field_path, message="is required")
        )

    value = raw_mapping[field_name]
    if not isinstance(value, int) or isinstance(value, bool):
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=field_path,
                message="must be an integer",
            )
        )

    return value


def _require_positive_finite_number(raw_mapping: Mapping[str, Any], field_name: str, field_path: str) -> float:
    value = _require_finite_number(raw_mapping, field_name, field_path)
    if value <= 0.0:
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=field_path,
                message="must be strictly positive",
            )
        )

    return value


def _require_non_negative_finite_number(
    raw_mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
) -> float:
    value = _require_finite_number(raw_mapping, field_name, field_path)
    if value < 0.0:
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=field_path,
                message="must be non-negative",
            )
        )

    return value


def _require_finite_number(raw_mapping: Mapping[str, Any], field_name: str, field_path: str) -> float:
    if field_name not in raw_mapping:
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(field_path=field_path, message="is required")
        )

    return _require_finite_scalar(raw_mapping[field_name], field_path=field_path)


def _require_non_negative_finite_scalar(raw_value: object, *, field_path: str) -> float:
    value = _require_finite_scalar(raw_value, field_path=field_path)
    if value < 0.0:
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=field_path,
                message="must be non-negative",
            )
        )

    return value


def _require_finite_scalar(raw_value: object, *, field_path: str) -> float:
    if not isinstance(raw_value, int | float) or isinstance(raw_value, bool):
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=field_path,
                message="must be a number",
            )
        )

    value = float(raw_value)
    if not isfinite(value):
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=field_path,
                message="must be finite",
            )
        )

    return value


__all__ = [
    "SUFFICIENT_STATISTICS_RISK_PATH_DIAGNOSTICS_SCHEMA_VERSION",
    "SUFFICIENT_STATISTICS_RISK_PATH_OUTPUT_SCHEMA_METADATA_VERSION",
    "SUFFICIENT_STATISTICS_RISK_PATH_PROVENANCE_SCHEMA_VERSION",
    "SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY",
    "SufficientStatisticsIntervalStatistics",
    "SufficientStatisticsRiskPathDiagnostics",
    "SufficientStatisticsRiskEquationSpecification",
    "SufficientStatisticsRiskIntervalFactors",
    "SufficientStatisticsRiskModelInputs",
    "SufficientStatisticsRiskPath",
    "SufficientStatisticsRiskPathOutputSchemaMetadata",
    "SufficientStatisticsRiskPathProvenance",
    "SufficientStatisticsRiskPathRecord",
    "SufficientStatisticsRiskReturnComponents",
    "SufficientStatisticsRiskRunRequest",
    "SufficientStatisticsRiskValidationError",
    "SufficientStatisticsRiskValidationFailure",
    "deserialize_sufficient_statistics_risk_path",
    "deserialize_sufficient_statistics_risk_run_request",
    "serialize_sufficient_statistics_risk_path",
    "serialize_sufficient_statistics_risk_run_request",
    "validate_sufficient_statistics_risk_path",
    "validate_sufficient_statistics_risk_run_request",
]
