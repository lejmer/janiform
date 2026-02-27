import json
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from math import isfinite, sqrt
from typing import NotRequired, TypedDict, cast

from .pricing import GammaSeverityMarkConfiguration, NigSharedPlusShiftConfiguration
from .simulation import HawkesExcitationState, HawkesKernelParameters

SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY = (
    "explicit_interval_aligned_s_h_and_nu_h_vectors_without_implicit_resampling.v1"
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
    gap_return: float
    total_return: float


class SufficientStatisticsRiskPathRecord(TypedDict):
    interval_index: int
    sufficient_statistics: SufficientStatisticsIntervalStatistics
    return_components: SufficientStatisticsRiskReturnComponents


class SufficientStatisticsRiskPath(TypedDict):
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
    raw_request: Mapping[str, object],
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
    raw_path: Mapping[str, object],
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

    return {
        "seed": seed,
        "interval_delta": interval_delta,
        "simulated_interval_count": simulated_interval_count,
        "initial_excitation_state": initial_excitation_state,
        "terminal_excitation_state": terminal_excitation_state,
        "records": records,
    }


def serialize_sufficient_statistics_risk_run_request(request: Mapping[str, object]) -> str:
    validated_request: SufficientStatisticsRiskRunRequest = (
        validate_sufficient_statistics_risk_run_request(request)
    )
    return json.dumps(validated_request, sort_keys=True)


def deserialize_sufficient_statistics_risk_run_request(
    payload_json: str,
) -> SufficientStatisticsRiskRunRequest:
    raw_payload: Mapping[str, object] = _parse_json_object(payload_json, "risk run request")
    return validate_sufficient_statistics_risk_run_request(raw_payload)


def serialize_sufficient_statistics_risk_path(path: Mapping[str, object]) -> str:
    validated_path: SufficientStatisticsRiskPath = validate_sufficient_statistics_risk_path(path)
    return json.dumps(validated_path, sort_keys=True)


def deserialize_sufficient_statistics_risk_path(payload_json: str) -> SufficientStatisticsRiskPath:
    raw_payload: Mapping[str, object] = _parse_json_object(payload_json, "risk path")
    return validate_sufficient_statistics_risk_path(raw_payload)


def _validate_model_inputs(raw_request: Mapping[str, object]) -> SufficientStatisticsRiskModelInputs:
    raw_model_inputs: Mapping[str, object] = _require_mapping(
        raw_request,
        "model_inputs",
        "model_inputs",
    )
    raw_equation_specification: Mapping[str, object] = _require_mapping(
        raw_model_inputs,
        "equation_specification",
        "model_inputs.equation_specification",
    )
    raw_kernel_parameters: Mapping[str, object] = _require_mapping(
        raw_equation_specification,
        "hawkes_kernel_parameters",
        "model_inputs.equation_specification.hawkes_kernel_parameters",
    )
    raw_gamma_configuration: Mapping[str, object] = _require_mapping(
        raw_equation_specification,
        "gamma_severity_mark_configuration",
        "model_inputs.equation_specification.gamma_severity_mark_configuration",
    )
    raw_nig_configuration: Mapping[str, object] = _require_mapping(
        raw_model_inputs,
        "nig_shared_plus_shift_configuration",
        "model_inputs.nig_shared_plus_shift_configuration",
    )
    raw_initial_state: Mapping[str, object] = _require_mapping(
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
    raw_payload: Mapping[str, object],
    field_name: str,
    field_path: str,
    *,
    requested_interval_delta: float,
) -> SufficientStatisticsRiskIntervalFactors:
    raw_interval_factors: Mapping[str, object] = _require_mapping(raw_payload, field_name, field_path)
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
    raw_kernel_parameters: Mapping[str, object],
    field_path: str,
) -> HawkesKernelParameters:
    raw_alpha: Mapping[str, object] = _require_mapping(raw_kernel_parameters, "alpha", f"{field_path}.alpha")
    raw_rho: Mapping[str, object] = _require_mapping(raw_kernel_parameters, "rho", f"{field_path}.rho")

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
    raw_loadings: Mapping[str, object],
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
    raw_gamma_configuration: Mapping[str, object],
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
    raw_nig_configuration: Mapping[str, object],
    field_path: str,
) -> NigSharedPlusShiftConfiguration:
    alpha: float = _require_positive_finite_number(raw_nig_configuration, "alpha", f"{field_path}.alpha")
    beta_0: float = _require_finite_number(raw_nig_configuration, "beta_0", f"{field_path}.beta_0")
    delta: float = _require_positive_finite_number(raw_nig_configuration, "delta", f"{field_path}.delta")
    mu: float = _require_finite_number(raw_nig_configuration, "mu", f"{field_path}.mu")
    delta_beta: float = _require_finite_number(raw_nig_configuration, "delta_beta", f"{field_path}.delta_beta")

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
    raw_state: Mapping[str, object],
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
    raw_payload: Mapping[str, object],
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
    raw_statistics: Mapping[str, object],
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
    raw_components: Mapping[str, object],
    field_path: str,
) -> SufficientStatisticsRiskReturnComponents:
    buy_side_return = _require_finite_number(raw_components, "buy_side_return", f"{field_path}.buy_side_return")
    sell_side_return = _require_finite_number(raw_components, "sell_side_return", f"{field_path}.sell_side_return")
    trading_time_return = _require_finite_number(
        raw_components,
        "trading_time_return",
        f"{field_path}.trading_time_return",
    )
    gap_return = _require_finite_number(raw_components, "gap_return", f"{field_path}.gap_return")
    total_return = _require_finite_number(raw_components, "total_return", f"{field_path}.total_return")

    return {
        "buy_side_return": buy_side_return,
        "sell_side_return": sell_side_return,
        "trading_time_return": trading_time_return,
        "gap_return": gap_return,
        "total_return": total_return,
    }


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


def _parse_json_object(payload_json: str, context_label: str) -> Mapping[str, object]:
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

    return cast(Mapping[str, object], payload)


def _require_mapping(raw_mapping: Mapping[str, object], field_name: str, field_path: str) -> Mapping[str, object]:
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

    return cast(Mapping[str, object], value)


def _require_sequence(raw_mapping: Mapping[str, object], field_name: str, field_path: str) -> Sequence[object]:
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
    raw_mapping: Mapping[str, object],
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


def _require_positive_integer(raw_mapping: Mapping[str, object], field_name: str, field_path: str) -> int:
    value = _require_integer(raw_mapping, field_name, field_path)
    if value <= 0:
        raise SufficientStatisticsRiskValidationError(
            SufficientStatisticsRiskValidationFailure(
                field_path=field_path,
                message="must be strictly positive",
            )
        )
    return value


def _require_integer(raw_mapping: Mapping[str, object], field_name: str, field_path: str) -> int:
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


def _require_positive_finite_number(raw_mapping: Mapping[str, object], field_name: str, field_path: str) -> float:
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
    raw_mapping: Mapping[str, object],
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


def _require_finite_number(raw_mapping: Mapping[str, object], field_name: str, field_path: str) -> float:
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
    "SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY",
    "SufficientStatisticsIntervalStatistics",
    "SufficientStatisticsRiskEquationSpecification",
    "SufficientStatisticsRiskIntervalFactors",
    "SufficientStatisticsRiskModelInputs",
    "SufficientStatisticsRiskPath",
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
