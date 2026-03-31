from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any, Optional

from .contracts import (
    ContractModel,
    MeanReversionAnchorModelKind,
    MeanReversionBoundaryState,
    MeanReversionFittedParameters,
    MeanReversionRuntimeIntervalAnchors,
    ValidationErrorFactory,
    parse_json_object,
    require_mapping,
    require_non_negative_finite_number,
    require_non_negative_integer,
    require_positive_finite_number,
    require_positive_integer,
    require_sequence,
    serialize_deterministic_json,
    validate_exact_integer,
    validate_exact_number,
    validate_exact_optional_integer,
    validate_exact_string,
    validate_required_field_list,
)
from .errors import ContractValidationFailure, JaniformValidationError
from .hourly_factors import (
    HourlyFactorContract,
    SeasonalityInputValidationError,
    load_hourly_factor_contract_payload,
)
from ._timestamps import hours_to_exact_nanoseconds

if TYPE_CHECKING:
    from .pricing import GammaSeverityMarkConfiguration, NigSharedPlusShiftConfiguration
    from .simulation import HawkesExcitationState, HawkesKernelParameters


def _parse_gamma_severity_mark_configuration(
    raw_payload: dict[str, Any] | Any,
    *,
    field_path: str,
) -> Any:
    from .pricing import GammaSeverityMarkConfiguration

    return GammaSeverityMarkConfiguration.from_mapping(raw_payload, field_path=field_path)


def _parse_nig_shared_plus_shift_configuration(
    raw_payload: dict[str, Any] | Any,
    *,
    field_path: str,
) -> Any:
    from .pricing import NigSharedPlusShiftConfiguration

    return NigSharedPlusShiftConfiguration.from_mapping(raw_payload, field_path=field_path)


def _parse_hawkes_kernel_parameters(
    raw_payload: dict[str, Any] | Any,
    *,
    field_path: str,
) -> Any:
    from .simulation import HawkesKernelParameters

    return HawkesKernelParameters.from_mapping(raw_payload, field_path=field_path)


def _parse_hawkes_excitation_state(
    raw_payload: dict[str, Any] | Any,
    *,
    field_path: str,
) -> Any:
    from .simulation import HawkesExcitationState

    return HawkesExcitationState.from_mapping(raw_payload, field_path=field_path)


SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY = (
    "explicit_interval_aligned_s_h_and_news_intensity_inputs_without_implicit_resampling.v1"
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
    "cumulative_observed_return",
    "max_abs_total_return",
    "max_abs_deviation_from_anchor",
    "terminal_observed_log_price",
    "terminal_deviation_from_anchor",
)
SUFFICIENT_STATISTICS_RISK_PATH_ARTIFACT_WORKFLOW_TAGS: tuple[str, ...] = (
    "calibration",
    "validation",
)
DEFAULT_INTRADAY_RISK_INTERVAL_DELTA = 1.0
_MEAN_REVERSION_ALIGNMENT_TOLERANCE = 1e-12
RISK_VALIDATION_ERROR_ID = "risk.validation.contract"

SufficientStatisticsRiskValidationFailure = ContractValidationFailure


class SufficientStatisticsRiskValidationError(JaniformValidationError):
    pass


def _risk_error(
    field_path: str,
    message: str,
    *,
    entrypoint: str = "contract",
    error_id: str = RISK_VALIDATION_ERROR_ID,
    context: Optional[dict[str, Optional[str | int | float | bool]]] = None,
) -> SufficientStatisticsRiskValidationError:
    return SufficientStatisticsRiskValidationError(
        SufficientStatisticsRiskValidationFailure.from_context(
            error_id=error_id,
            workflow="risk",
            entrypoint=entrypoint,
            field_path=field_path,
            message=message,
            context=context,
        )
    )


RISK_ERROR_FACTORY: ValidationErrorFactory = _risk_error


@dataclass(frozen=True, slots=True)
class SufficientStatisticsRiskEquationSpecification(ContractModel):
    hawkes_kernel_parameters: HawkesKernelParameters
    gamma_severity_mark_configuration: GammaSeverityMarkConfiguration

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> SufficientStatisticsRiskEquationSpecification:
        if not isinstance(raw_payload, dict):
            raise _risk_error(field_path, "must be an object")

        return SufficientStatisticsRiskEquationSpecification(
            hawkes_kernel_parameters=_parse_hawkes_kernel_parameters(
                require_mapping(
                    raw_payload,
                    "hawkes_kernel_parameters",
                    f"{field_path}.hawkes_kernel_parameters",
                    RISK_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.hawkes_kernel_parameters",
            ),
            gamma_severity_mark_configuration=_parse_gamma_severity_mark_configuration(
                require_mapping(
                    raw_payload,
                    "gamma_severity_mark_configuration",
                    f"{field_path}.gamma_severity_mark_configuration",
                    RISK_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.gamma_severity_mark_configuration",
            ),
        )


@dataclass(frozen=True, slots=True)
class SufficientStatisticsRiskModelInputs(ContractModel):
    equation_specification: SufficientStatisticsRiskEquationSpecification
    nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration
    initial_excitation_state: HawkesExcitationState
    mean_reversion_fitted_parameters: MeanReversionFittedParameters
    mean_reversion_runtime_interval_anchors: MeanReversionRuntimeIntervalAnchors
    initial_mean_reversion_boundary_state: MeanReversionBoundaryState

    def __post_init__(self) -> None:
        if (
            abs(
                self.initial_mean_reversion_boundary_state.anchor_value
                - self.mean_reversion_runtime_interval_anchors.start_anchor_value
            )
            > _MEAN_REVERSION_ALIGNMENT_TOLERANCE
        ):
            raise _risk_error(
                "model_inputs.initial_mean_reversion_boundary_state.anchor_value",
                (
                    "must match model_inputs.mean_reversion_runtime_interval_anchors."
                    f"start_anchor_value ({self.mean_reversion_runtime_interval_anchors.start_anchor_value})"
                ),
            )

        if (
            self.mean_reversion_fitted_parameters.anchor_model.anchor_model_kind
            != MeanReversionAnchorModelKind.ENDOGENOUS_CORE_PRESSURE_LEVEL
        ):
            raise _risk_error(
                "model_inputs.mean_reversion_fitted_parameters.anchor_model.anchor_model_kind",
                "must be endogenous_core_pressure_level",
            )

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any) -> SufficientStatisticsRiskModelInputs:
        if not isinstance(raw_payload, dict):
            raise _risk_error("model_inputs", "must be an object")

        try:
            return SufficientStatisticsRiskModelInputs(
                equation_specification=SufficientStatisticsRiskEquationSpecification.from_mapping(
                    require_mapping(
                        raw_payload,
                        "equation_specification",
                        "model_inputs.equation_specification",
                        RISK_ERROR_FACTORY,
                    ),
                    field_path="model_inputs.equation_specification",
                ),
                nig_shared_plus_shift_configuration=_parse_nig_shared_plus_shift_configuration(
                    require_mapping(
                        raw_payload,
                        "nig_shared_plus_shift_configuration",
                        "model_inputs.nig_shared_plus_shift_configuration",
                        RISK_ERROR_FACTORY,
                    ),
                    field_path="model_inputs.nig_shared_plus_shift_configuration",
                ),
                initial_excitation_state=_parse_hawkes_excitation_state(
                    require_mapping(
                        raw_payload,
                        "initial_excitation_state",
                        "model_inputs.initial_excitation_state",
                        RISK_ERROR_FACTORY,
                    ),
                    field_path="model_inputs.initial_excitation_state",
                ),
                mean_reversion_fitted_parameters=MeanReversionFittedParameters.from_mapping(
                    require_mapping(
                        raw_payload,
                        "mean_reversion_fitted_parameters",
                        "model_inputs.mean_reversion_fitted_parameters",
                        RISK_ERROR_FACTORY,
                    ),
                    field_path="model_inputs.mean_reversion_fitted_parameters",
                ),
                mean_reversion_runtime_interval_anchors=MeanReversionRuntimeIntervalAnchors.from_mapping(
                    require_mapping(
                        raw_payload,
                        "mean_reversion_runtime_interval_anchors",
                        "model_inputs.mean_reversion_runtime_interval_anchors",
                        RISK_ERROR_FACTORY,
                    ),
                    field_path="model_inputs.mean_reversion_runtime_interval_anchors",
                ),
                initial_mean_reversion_boundary_state=MeanReversionBoundaryState.from_mapping(
                    require_mapping(
                        raw_payload,
                        "initial_mean_reversion_boundary_state",
                        "model_inputs.initial_mean_reversion_boundary_state",
                        RISK_ERROR_FACTORY,
                    ),
                    field_path="model_inputs.initial_mean_reversion_boundary_state",
                ),
            )
        except JaniformValidationError as e:
            raise _risk_error(e.failure.field_path, e.failure.message) from e


@dataclass(frozen=True, slots=True)
class SufficientStatisticsRiskRunRequest(ContractModel):
    seed: int
    horizon_interval_count: int
    interval_delta: float = DEFAULT_INTRADAY_RISK_INTERVAL_DELTA
    model_inputs: Optional[SufficientStatisticsRiskModelInputs] = None
    hourly_factor_contract: Optional[HourlyFactorContract] = None

    def __post_init__(self) -> None:
        if self.seed < 0:
            raise _risk_error("seed", "must be non-negative")
        if self.horizon_interval_count <= 0:
            raise _risk_error("horizon_interval_count", "must be strictly positive")
        if self.interval_delta <= 0.0:
            raise _risk_error("interval_delta", "must be strictly positive")
        if self.model_inputs is None:
            raise _risk_error("model_inputs", "is required")
        if self.hourly_factor_contract is None:
            raise _risk_error("hourly_factor_contract", "is required")

        available_interval_count = self.hourly_factor_contract.interval_count
        try:
            expected_interval_duration_nanos = hours_to_exact_nanoseconds(self.interval_delta)
        except ValueError as e:
            raise _risk_error("interval_delta", str(e)) from e

        if self.horizon_interval_count > available_interval_count:
            raise _risk_error(
                "horizon_interval_count",
                f"must not exceed available hourly factor interval count ({available_interval_count})",
            )
        if self.hourly_factor_contract.interval_duration_nanos != expected_interval_duration_nanos:
            raise _risk_error(
                "hourly_factor_contract.interval_duration_nanos",
                (
                    "must match interval_delta exactly under explicit factor alignment without implicit "
                    f"resampling (interval_delta={self.interval_delta}, "
                    "hourly_factor_contract.interval_duration_nanos="
                    f"{self.hourly_factor_contract.interval_duration_nanos})"
                ),
            )

        runtime_interval_anchors = self.model_inputs.mean_reversion_runtime_interval_anchors
        if len(runtime_interval_anchors.end_anchor_values) != self.horizon_interval_count:
            raise _risk_error(
                "model_inputs.mean_reversion_runtime_interval_anchors.end_anchor_values",
                f"must contain {self.horizon_interval_count} values",
            )

        for interval_index, observed_interval_delta in enumerate(
            runtime_interval_anchors.interval_deltas[: self.horizon_interval_count]
        ):
            if abs(observed_interval_delta - self.interval_delta) > _MEAN_REVERSION_ALIGNMENT_TOLERANCE:
                raise _risk_error(
                    f"model_inputs.mean_reversion_runtime_interval_anchors.interval_deltas[{interval_index}]",
                    f"must equal interval_delta ({self.interval_delta}), found {observed_interval_delta}",
                )

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any) -> SufficientStatisticsRiskRunRequest:
        if not isinstance(raw_payload, dict):
            raise _risk_error("risk_run_request", "must be an object")

        interval_delta = (
            require_positive_finite_number(
                raw_payload,
                "interval_delta",
                "interval_delta",
                RISK_ERROR_FACTORY,
            )
            if "interval_delta" in raw_payload
            else DEFAULT_INTRADAY_RISK_INTERVAL_DELTA
        )

        try:
            hourly_factor_contract = load_hourly_factor_contract_payload(
                require_mapping(
                    raw_payload,
                    "hourly_factor_contract",
                    "hourly_factor_contract",
                    RISK_ERROR_FACTORY,
                )
            )
        except SeasonalityInputValidationError as e:
            raise _risk_error("hourly_factor_contract", str(e)) from e

        return SufficientStatisticsRiskRunRequest(
            seed=require_non_negative_integer(raw_payload, "seed", "seed", RISK_ERROR_FACTORY),
            horizon_interval_count=require_positive_integer(
                raw_payload,
                "horizon_interval_count",
                "horizon_interval_count",
                RISK_ERROR_FACTORY,
            ),
            interval_delta=interval_delta,
            model_inputs=SufficientStatisticsRiskModelInputs.from_mapping(
                require_mapping(raw_payload, "model_inputs", "model_inputs", RISK_ERROR_FACTORY)
            ),
            hourly_factor_contract=hourly_factor_contract,
        )

    def to_payload(self) -> dict[str, Any]:
        if self.model_inputs is None or self.hourly_factor_contract is None:
            raise _risk_error("risk_run_request", "must be fully initialized before serialization")

        return {
            "seed": self.seed,
            "horizon_interval_count": self.horizon_interval_count,
            "interval_delta": self.interval_delta,
            "model_inputs": self.model_inputs.to_payload(),
            "hourly_factor_contract": self.hourly_factor_contract.to_payload(),
        }


@dataclass(frozen=True, slots=True)
class SufficientStatisticsIntervalStatistics(ContractModel):
    a_plus: float
    a_minus: float
    n_plus: int
    n_minus: int

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> SufficientStatisticsIntervalStatistics:
        if not isinstance(raw_payload, dict):
            raise _risk_error(field_path, "must be an object")

        return SufficientStatisticsIntervalStatistics(
            a_plus=require_non_negative_finite_number(
                raw_payload,
                "a_plus",
                f"{field_path}.a_plus",
                RISK_ERROR_FACTORY,
            ),
            a_minus=require_non_negative_finite_number(
                raw_payload,
                "a_minus",
                f"{field_path}.a_minus",
                RISK_ERROR_FACTORY,
            ),
            n_plus=require_non_negative_integer(
                raw_payload,
                "n_plus",
                f"{field_path}.n_plus",
                RISK_ERROR_FACTORY,
            ),
            n_minus=require_non_negative_integer(
                raw_payload,
                "n_minus",
                f"{field_path}.n_minus",
                RISK_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class SufficientStatisticsRiskReturnComponents(ContractModel):
    buy_side_return: float
    sell_side_return: float
    trading_time_return: float
    open_jump_return: float
    closed_gap_return: float
    total_return: float

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> SufficientStatisticsRiskReturnComponents:
        if not isinstance(raw_payload, dict):
            raise _risk_error(field_path, "must be an object")

        return SufficientStatisticsRiskReturnComponents(
            buy_side_return=require_positive_or_negative(raw_payload, "buy_side_return", f"{field_path}.buy_side_return"),
            sell_side_return=require_positive_or_negative(raw_payload, "sell_side_return", f"{field_path}.sell_side_return"),
            trading_time_return=require_positive_or_negative(raw_payload, "trading_time_return", f"{field_path}.trading_time_return"),
            open_jump_return=require_positive_or_negative(raw_payload, "open_jump_return", f"{field_path}.open_jump_return"),
            closed_gap_return=require_positive_or_negative(raw_payload, "closed_gap_return", f"{field_path}.closed_gap_return"),
            total_return=require_positive_or_negative(raw_payload, "total_return", f"{field_path}.total_return"),
        )


def require_positive_or_negative(raw_payload: dict[str, Any], field_name: str, field_path: str) -> float:
    return require_positive_or_negative_impl(raw_payload, field_name, field_path)


def require_positive_or_negative_impl(raw_payload: dict[str, Any], field_name: str, field_path: str) -> float:
    return require_positive_or_negative_number(raw_payload, field_name, field_path)


def require_positive_or_negative_number(raw_payload: dict[str, Any], field_name: str, field_path: str) -> float:
    from .contracts import require_finite_number

    return require_finite_number(raw_payload, field_name, field_path, RISK_ERROR_FACTORY)


@dataclass(frozen=True, slots=True)
class SufficientStatisticsRiskPathRecord(ContractModel):
    interval_index: int
    sufficient_statistics: SufficientStatisticsIntervalStatistics
    return_components: SufficientStatisticsRiskReturnComponents
    core_total_return: float
    observed_interval_return: float
    observed_log_price_end: float
    anchor_value_end: float
    deviation_from_anchor_end: float

    def __post_init__(self) -> None:
        if self.core_total_return != self.return_components.total_return:
            raise _risk_error(
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
    ) -> SufficientStatisticsRiskPathRecord:
        if not isinstance(raw_payload, dict):
            raise _risk_error(field_path, "must be an object")

        if "schema_version" in raw_payload:
            validate_exact_string(
                raw_payload,
                "schema_version",
                f"{field_path}.schema_version",
                SUFFICIENT_STATISTICS_RISK_PATH_RECORD_SCHEMA_VERSION,
                RISK_ERROR_FACTORY,
            )

        interval_index = require_non_negative_integer(
            raw_payload,
            "interval_index",
            f"{field_path}.interval_index",
            RISK_ERROR_FACTORY,
        )
        if interval_index != expected_interval_index:
            raise _risk_error(
                f"{field_path}.interval_index",
                f"must equal {expected_interval_index}",
            )

        return SufficientStatisticsRiskPathRecord(
            interval_index=interval_index,
            sufficient_statistics=SufficientStatisticsIntervalStatistics.from_mapping(
                require_mapping(
                    raw_payload,
                    "sufficient_statistics",
                    f"{field_path}.sufficient_statistics",
                    RISK_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.sufficient_statistics",
            ),
            return_components=SufficientStatisticsRiskReturnComponents.from_mapping(
                require_mapping(
                    raw_payload,
                    "return_components",
                    f"{field_path}.return_components",
                    RISK_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.return_components",
            ),
            core_total_return=require_positive_or_negative(
                raw_payload,
                "core_total_return",
                f"{field_path}.core_total_return",
            ),
            observed_interval_return=require_positive_or_negative(
                raw_payload,
                "observed_interval_return",
                f"{field_path}.observed_interval_return",
            ),
            observed_log_price_end=require_positive_or_negative(
                raw_payload,
                "observed_log_price_end",
                f"{field_path}.observed_log_price_end",
            ),
            anchor_value_end=require_positive_or_negative(
                raw_payload,
                "anchor_value_end",
                f"{field_path}.anchor_value_end",
            ),
            deviation_from_anchor_end=require_positive_or_negative(
                raw_payload,
                "deviation_from_anchor_end",
                f"{field_path}.deviation_from_anchor_end",
            ),
        )


@dataclass(frozen=True, slots=True)
class SufficientStatisticsRiskPathOutputSchemaMetadata(ContractModel):
    schema_version: str
    provenance_schema_version: str
    diagnostics_schema_version: str
    artifact_workflow_policy: str
    environment_metadata_policy: str
    required_path_fields: tuple[str, ...]
    required_provenance_fields: tuple[str, ...]
    required_diagnostics_fields: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class SufficientStatisticsRiskPathProvenance(ContractModel):
    schema_version: str
    artifact_workflow_policy: str
    environment_metadata_policy: str
    interval_factor_policy: str
    transform_slice_sampling_policy: str
    boundary_state_update_policy: str
    interval_return_generation_policy: str
    interval_return_component_routing_policy: str
    batch_seed_partition_policy: str
    artifact_workflow_tags: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class SufficientStatisticsRiskPathDiagnostics(ContractModel):
    schema_version: str
    simulated_interval_count: int
    first_interval_index: Optional[int]
    last_interval_index: Optional[int]
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
class SufficientStatisticsRiskPath(ContractModel):
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
    records: tuple[SufficientStatisticsRiskPathRecord, ...]

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any) -> SufficientStatisticsRiskPath:
        if not isinstance(raw_payload, dict):
            raise _risk_error("risk_path", "must be an object")

        seed = require_non_negative_integer(raw_payload, "seed", "seed", RISK_ERROR_FACTORY)

        interval_delta = require_positive_finite_number(
            raw_payload,
            "interval_delta",
            "interval_delta",
            RISK_ERROR_FACTORY,
        )

        simulated_interval_count = require_positive_integer(
            raw_payload,
            "simulated_interval_count",
            "simulated_interval_count",
            RISK_ERROR_FACTORY,
        )

        records_payload = require_sequence(raw_payload, "records", "records", RISK_ERROR_FACTORY)
        if len(records_payload) != simulated_interval_count:
            raise _risk_error("records", f"must contain exactly {simulated_interval_count} records")

        records = tuple(
            SufficientStatisticsRiskPathRecord.from_mapping(
                raw_record,
                field_path=f"records[{record_position}]",
                expected_interval_index=record_position,
            )
            for record_position, raw_record in enumerate(records_payload)
        )

        output_schema_metadata = _build_risk_path_output_schema_metadata()
        provenance = _build_risk_path_provenance()
        diagnostics = _build_risk_path_diagnostics(records, simulated_interval_count)

        if "schema_version" in raw_payload:
            validate_exact_string(
                raw_payload,
                "schema_version",
                "schema_version",
                SUFFICIENT_STATISTICS_RISK_PATH_SCHEMA_VERSION,
                RISK_ERROR_FACTORY,
            )
        if "run_request_schema_version" in raw_payload:
            validate_exact_string(
                raw_payload,
                "run_request_schema_version",
                "run_request_schema_version",
                SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION,
                RISK_ERROR_FACTORY,
            )
        if "output_schema_metadata" in raw_payload:
            _validate_output_schema_metadata(
                require_mapping(
                    raw_payload,
                    "output_schema_metadata",
                    "output_schema_metadata",
                    RISK_ERROR_FACTORY,
                ),
                output_schema_metadata,
            )
        if "provenance" in raw_payload:
            _validate_path_provenance(
                require_mapping(raw_payload, "provenance", "provenance", RISK_ERROR_FACTORY),
                provenance,
            )
        if "diagnostics" in raw_payload:
            _validate_path_diagnostics(
                require_mapping(raw_payload, "diagnostics", "diagnostics", RISK_ERROR_FACTORY),
                diagnostics,
                simulated_interval_count,
            )

        try:
            return SufficientStatisticsRiskPath(
                schema_version=SUFFICIENT_STATISTICS_RISK_PATH_SCHEMA_VERSION,
                run_request_schema_version=SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION,
                output_schema_metadata=output_schema_metadata,
                provenance=provenance,
                diagnostics=diagnostics,
                seed=seed,
                interval_delta=interval_delta,
                simulated_interval_count=simulated_interval_count,
                initial_excitation_state=_parse_hawkes_excitation_state(
                    require_mapping(
                        raw_payload,
                        "initial_excitation_state",
                        "initial_excitation_state",
                        RISK_ERROR_FACTORY,
                    ),
                    field_path="initial_excitation_state",
                ),
                terminal_excitation_state=_parse_hawkes_excitation_state(
                    require_mapping(
                        raw_payload,
                        "terminal_excitation_state",
                        "terminal_excitation_state",
                        RISK_ERROR_FACTORY,
                    ),
                    field_path="terminal_excitation_state",
                ),
                records=records,
            )
        except JaniformValidationError as e:
            raise _risk_error(e.failure.field_path, e.failure.message) from e


def _build_risk_path_output_schema_metadata() -> SufficientStatisticsRiskPathOutputSchemaMetadata:
    return SufficientStatisticsRiskPathOutputSchemaMetadata(
        schema_version=SUFFICIENT_STATISTICS_RISK_PATH_OUTPUT_SCHEMA_METADATA_VERSION,
        provenance_schema_version=SUFFICIENT_STATISTICS_RISK_PATH_PROVENANCE_SCHEMA_VERSION,
        diagnostics_schema_version=SUFFICIENT_STATISTICS_RISK_PATH_DIAGNOSTICS_SCHEMA_VERSION,
        artifact_workflow_policy=SUFFICIENT_STATISTICS_RISK_PATH_ARTIFACT_WORKFLOW_POLICY,
        environment_metadata_policy=SUFFICIENT_STATISTICS_RISK_PATH_ENVIRONMENT_METADATA_POLICY,
        required_path_fields=SUFFICIENT_STATISTICS_RISK_PATH_REQUIRED_FIELDS,
        required_provenance_fields=SUFFICIENT_STATISTICS_RISK_PATH_REQUIRED_PROVENANCE_FIELDS,
        required_diagnostics_fields=SUFFICIENT_STATISTICS_RISK_PATH_REQUIRED_DIAGNOSTICS_FIELDS,
    )


def _build_risk_path_provenance() -> SufficientStatisticsRiskPathProvenance:
    return SufficientStatisticsRiskPathProvenance(
        schema_version=SUFFICIENT_STATISTICS_RISK_PATH_PROVENANCE_SCHEMA_VERSION,
        artifact_workflow_policy=SUFFICIENT_STATISTICS_RISK_PATH_ARTIFACT_WORKFLOW_POLICY,
        environment_metadata_policy=SUFFICIENT_STATISTICS_RISK_PATH_ENVIRONMENT_METADATA_POLICY,
        interval_factor_policy=SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY,
        transform_slice_sampling_policy=SUFFICIENT_STATISTICS_TRANSFORM_SLICE_SAMPLING_POLICY,
        boundary_state_update_policy=SUFFICIENT_STATISTICS_BOUNDARY_STATE_UPDATE_POLICY,
        interval_return_generation_policy=SUFFICIENT_STATISTICS_INTERVAL_RETURN_GENERATION_POLICY,
        interval_return_component_routing_policy=SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_ROUTING_POLICY,
        batch_seed_partition_policy=SUFFICIENT_STATISTICS_BATCH_SEED_PARTITION_POLICY,
        artifact_workflow_tags=SUFFICIENT_STATISTICS_RISK_PATH_ARTIFACT_WORKFLOW_TAGS,
    )


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
    cumulative_observed_return: float = 0.0
    max_abs_total_return: float = 0.0
    max_abs_deviation_from_anchor: float = 0.0

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

    first_interval_index: Optional[int] = records[0].interval_index if records else None
    last_interval_index: Optional[int] = records[-1].interval_index if records else None

    return SufficientStatisticsRiskPathDiagnostics(
        schema_version=SUFFICIENT_STATISTICS_RISK_PATH_DIAGNOSTICS_SCHEMA_VERSION,
        simulated_interval_count=simulated_interval_count,
        first_interval_index=first_interval_index,
        last_interval_index=last_interval_index,
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


def _validate_output_schema_metadata(
    raw_payload: dict[str, Any] | Any,
    expected_metadata: SufficientStatisticsRiskPathOutputSchemaMetadata,
) -> None:
    validate_exact_string(
        raw_payload,
        "schema_version",
        "output_schema_metadata.schema_version",
        expected_metadata.schema_version,
        RISK_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "provenance_schema_version",
        "output_schema_metadata.provenance_schema_version",
        expected_metadata.provenance_schema_version,
        RISK_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "diagnostics_schema_version",
        "output_schema_metadata.diagnostics_schema_version",
        expected_metadata.diagnostics_schema_version,
        RISK_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "artifact_workflow_policy",
        "output_schema_metadata.artifact_workflow_policy",
        expected_metadata.artifact_workflow_policy,
        RISK_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "environment_metadata_policy",
        "output_schema_metadata.environment_metadata_policy",
        expected_metadata.environment_metadata_policy,
        RISK_ERROR_FACTORY,
    )
    validate_required_field_list(
        raw_payload,
        "required_path_fields",
        "output_schema_metadata.required_path_fields",
        expected_metadata.required_path_fields,
        RISK_ERROR_FACTORY,
    )
    validate_required_field_list(
        raw_payload,
        "required_provenance_fields",
        "output_schema_metadata.required_provenance_fields",
        expected_metadata.required_provenance_fields,
        RISK_ERROR_FACTORY,
    )
    validate_required_field_list(
        raw_payload,
        "required_diagnostics_fields",
        "output_schema_metadata.required_diagnostics_fields",
        expected_metadata.required_diagnostics_fields,
        RISK_ERROR_FACTORY,
    )


def _validate_path_provenance(
    raw_payload: dict[str, Any] | Any,
    expected_provenance: SufficientStatisticsRiskPathProvenance,
) -> None:
    validate_exact_string(
        raw_payload,
        "schema_version",
        "provenance.schema_version",
        expected_provenance.schema_version,
        RISK_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "artifact_workflow_policy",
        "provenance.artifact_workflow_policy",
        expected_provenance.artifact_workflow_policy,
        RISK_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "environment_metadata_policy",
        "provenance.environment_metadata_policy",
        expected_provenance.environment_metadata_policy,
        RISK_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "interval_factor_policy",
        "provenance.interval_factor_policy",
        expected_provenance.interval_factor_policy,
        RISK_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "transform_slice_sampling_policy",
        "provenance.transform_slice_sampling_policy",
        expected_provenance.transform_slice_sampling_policy,
        RISK_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "boundary_state_update_policy",
        "provenance.boundary_state_update_policy",
        expected_provenance.boundary_state_update_policy,
        RISK_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "interval_return_generation_policy",
        "provenance.interval_return_generation_policy",
        expected_provenance.interval_return_generation_policy,
        RISK_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "interval_return_component_routing_policy",
        "provenance.interval_return_component_routing_policy",
        expected_provenance.interval_return_component_routing_policy,
        RISK_ERROR_FACTORY,
    )
    validate_exact_string(
        raw_payload,
        "batch_seed_partition_policy",
        "provenance.batch_seed_partition_policy",
        expected_provenance.batch_seed_partition_policy,
        RISK_ERROR_FACTORY,
    )
    validate_required_field_list(
        raw_payload,
        "artifact_workflow_tags",
        "provenance.artifact_workflow_tags",
        expected_provenance.artifact_workflow_tags,
        RISK_ERROR_FACTORY,
    )


def _validate_path_diagnostics(
    raw_payload: dict[str, Any] | Any,
    expected_diagnostics: SufficientStatisticsRiskPathDiagnostics,
    simulated_interval_count: int,
) -> None:
    validate_exact_string(
        raw_payload,
        "schema_version",
        "diagnostics.schema_version",
        expected_diagnostics.schema_version,
        RISK_ERROR_FACTORY,
    )
    validate_exact_integer(
        raw_payload,
        "simulated_interval_count",
        "diagnostics.simulated_interval_count",
        expected_diagnostics.simulated_interval_count,
        RISK_ERROR_FACTORY,
    )
    validate_exact_optional_integer(
        raw_payload,
        "first_interval_index",
        "diagnostics.first_interval_index",
        expected_diagnostics.first_interval_index,
        RISK_ERROR_FACTORY,
    )
    validate_exact_optional_integer(
        raw_payload,
        "last_interval_index",
        "diagnostics.last_interval_index",
        expected_diagnostics.last_interval_index,
        RISK_ERROR_FACTORY,
    )
    validate_exact_number(
        raw_payload,
        "cumulative_a_plus",
        "diagnostics.cumulative_a_plus",
        expected_diagnostics.cumulative_a_plus,
        RISK_ERROR_FACTORY,
        context_suffix=f"simulated_interval_count={simulated_interval_count}",
    )
    validate_exact_number(
        raw_payload,
        "cumulative_a_minus",
        "diagnostics.cumulative_a_minus",
        expected_diagnostics.cumulative_a_minus,
        RISK_ERROR_FACTORY,
        context_suffix=f"simulated_interval_count={simulated_interval_count}",
    )
    validate_exact_integer(
        raw_payload,
        "cumulative_n_plus",
        "diagnostics.cumulative_n_plus",
        expected_diagnostics.cumulative_n_plus,
        RISK_ERROR_FACTORY,
        context_suffix=f"simulated_interval_count={simulated_interval_count}",
    )
    validate_exact_integer(
        raw_payload,
        "cumulative_n_minus",
        "diagnostics.cumulative_n_minus",
        expected_diagnostics.cumulative_n_minus,
        RISK_ERROR_FACTORY,
        context_suffix=f"simulated_interval_count={simulated_interval_count}",
    )
    validate_exact_number(
        raw_payload,
        "cumulative_trading_time_return",
        "diagnostics.cumulative_trading_time_return",
        expected_diagnostics.cumulative_trading_time_return,
        RISK_ERROR_FACTORY,
        context_suffix=f"simulated_interval_count={simulated_interval_count}",
    )
    validate_exact_number(
        raw_payload,
        "cumulative_open_jump_return",
        "diagnostics.cumulative_open_jump_return",
        expected_diagnostics.cumulative_open_jump_return,
        RISK_ERROR_FACTORY,
        context_suffix=f"simulated_interval_count={simulated_interval_count}",
    )
    validate_exact_number(
        raw_payload,
        "cumulative_closed_gap_return",
        "diagnostics.cumulative_closed_gap_return",
        expected_diagnostics.cumulative_closed_gap_return,
        RISK_ERROR_FACTORY,
        context_suffix=f"simulated_interval_count={simulated_interval_count}",
    )
    validate_exact_number(
        raw_payload,
        "cumulative_total_return",
        "diagnostics.cumulative_total_return",
        expected_diagnostics.cumulative_total_return,
        RISK_ERROR_FACTORY,
        context_suffix=f"simulated_interval_count={simulated_interval_count}",
    )
    validate_exact_number(
        raw_payload,
        "cumulative_observed_return",
        "diagnostics.cumulative_observed_return",
        expected_diagnostics.cumulative_observed_return,
        RISK_ERROR_FACTORY,
        context_suffix=f"simulated_interval_count={simulated_interval_count}",
    )
    validate_exact_number(
        raw_payload,
        "max_abs_total_return",
        "diagnostics.max_abs_total_return",
        expected_diagnostics.max_abs_total_return,
        RISK_ERROR_FACTORY,
        context_suffix=f"simulated_interval_count={simulated_interval_count}",
    )
    validate_exact_number(
        raw_payload,
        "max_abs_deviation_from_anchor",
        "diagnostics.max_abs_deviation_from_anchor",
        expected_diagnostics.max_abs_deviation_from_anchor,
        RISK_ERROR_FACTORY,
        context_suffix=f"simulated_interval_count={simulated_interval_count}",
    )
    validate_exact_number(
        raw_payload,
        "terminal_observed_log_price",
        "diagnostics.terminal_observed_log_price",
        expected_diagnostics.terminal_observed_log_price,
        RISK_ERROR_FACTORY,
        context_suffix=f"simulated_interval_count={simulated_interval_count}",
    )
    validate_exact_number(
        raw_payload,
        "terminal_deviation_from_anchor",
        "diagnostics.terminal_deviation_from_anchor",
        expected_diagnostics.terminal_deviation_from_anchor,
        RISK_ERROR_FACTORY,
        context_suffix=f"simulated_interval_count={simulated_interval_count}",
    )


def validate_sufficient_statistics_risk_run_request(
    raw_request: dict[str, Any] | Any,
) -> SufficientStatisticsRiskRunRequest:
    return SufficientStatisticsRiskRunRequest.from_mapping(raw_request)


def validate_sufficient_statistics_risk_path(
    raw_path: dict[str, Any] | Any,
) -> SufficientStatisticsRiskPath:
    return SufficientStatisticsRiskPath.from_mapping(raw_path)


def serialize_sufficient_statistics_risk_run_request(
    request: SufficientStatisticsRiskRunRequest,
) -> str:
    return serialize_deterministic_json(request.to_payload())


def deserialize_sufficient_statistics_risk_run_request(
    payload_json: str,
) -> SufficientStatisticsRiskRunRequest:
    raw_payload = parse_json_object(payload_json, "risk run request", RISK_ERROR_FACTORY)
    return validate_sufficient_statistics_risk_run_request(dict(raw_payload))


def serialize_sufficient_statistics_risk_path(path: SufficientStatisticsRiskPath) -> str:
    return serialize_deterministic_json(path)


def deserialize_sufficient_statistics_risk_path(payload_json: str) -> SufficientStatisticsRiskPath:
    raw_payload = parse_json_object(payload_json, "risk path", RISK_ERROR_FACTORY)
    return validate_sufficient_statistics_risk_path(dict(raw_payload))


__all__ = [
    "SUFFICIENT_STATISTICS_RISK_PATH_DIAGNOSTICS_SCHEMA_VERSION",
    "SUFFICIENT_STATISTICS_RISK_PATH_OUTPUT_SCHEMA_METADATA_VERSION",
    "SUFFICIENT_STATISTICS_RISK_PATH_PROVENANCE_SCHEMA_VERSION",
    "SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY",
    "SufficientStatisticsIntervalStatistics",
    "SufficientStatisticsRiskPathDiagnostics",
    "SufficientStatisticsRiskEquationSpecification",
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
