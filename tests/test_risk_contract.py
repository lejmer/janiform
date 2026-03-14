import json
from typing import Any

import pytest

from janiform.pricing import GammaSeverityMarkConfiguration, NigSharedPlusShiftConfiguration
from janiform.risk import (
    SufficientStatisticsIntervalStatistics,
    SufficientStatisticsRiskEquationSpecification,
    SufficientStatisticsRiskIntervalFactors,
    SufficientStatisticsRiskModelInputs,
    SufficientStatisticsRiskPath,
    SufficientStatisticsRiskPathRecord,
    SufficientStatisticsRiskReturnComponents,
    SufficientStatisticsRiskRunRequest,
    SufficientStatisticsRiskValidationError,
    deserialize_sufficient_statistics_risk_path,
    deserialize_sufficient_statistics_risk_run_request,
    serialize_sufficient_statistics_risk_path,
    serialize_sufficient_statistics_risk_run_request,
    validate_sufficient_statistics_risk_path,
    validate_sufficient_statistics_risk_run_request,
)
from janiform.simulation import HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters


def test_risk_run_request_model_construction_and_serialization_roundtrip() -> None:
    request = _valid_run_request_model()

    serialized_request = serialize_sufficient_statistics_risk_run_request(request)
    deserialized_request = deserialize_sufficient_statistics_risk_run_request(serialized_request)

    assert deserialized_request == request
    assert deserialized_request.interval_delta == pytest.approx(1.0)
    assert json.loads(serialized_request)["horizon_interval_count"] == 2
    assert json.loads(serialized_request)["interval_factors"]["nu_h_known"] == [0.12, 0.04]


def test_risk_run_request_defaults_interval_delta_to_hourly() -> None:
    request_payload = _valid_run_request_payload()
    del request_payload["interval_delta"]

    validated_request = validate_sufficient_statistics_risk_run_request(request_payload)

    assert validated_request.interval_delta == pytest.approx(1.0)


def test_risk_run_request_accepts_sub_hour_interval_when_interval_factors_are_compatible() -> None:
    request_payload = _valid_run_request_payload()
    request_payload["interval_delta"] = 0.5
    request_payload["interval_factors"]["interval_delta"] = 0.5

    validated_request = validate_sufficient_statistics_risk_run_request(request_payload)

    assert validated_request.interval_delta == pytest.approx(0.5)
    assert validated_request.interval_factors is not None
    assert validated_request.interval_factors.interval_delta == pytest.approx(0.5)


def test_risk_run_request_rejects_sub_hour_interval_without_compatible_factor_delta() -> None:
    request_payload = _valid_run_request_payload()
    request_payload["interval_delta"] = 0.5

    with pytest.raises(SufficientStatisticsRiskValidationError) as captured_error:
        validate_sufficient_statistics_risk_run_request(request_payload)

    assert captured_error.value.failure.error_id == "risk.validation.contract"
    assert captured_error.value.failure.workflow == "risk"
    assert captured_error.value.failure.entrypoint == "contract"
    assert captured_error.value.failure.field_path == "interval_factors.interval_delta"
    assert "must match interval_delta exactly" in captured_error.value.failure.message


def test_risk_run_request_rejects_non_negative_delta_beta_with_field_path() -> None:
    request_payload = _valid_run_request_payload()
    request_payload["model_inputs"]["nig_shared_plus_shift_configuration"]["delta_beta"] = 0.0

    with pytest.raises(SufficientStatisticsRiskValidationError) as captured_error:
        validate_sufficient_statistics_risk_run_request(request_payload)

    assert (
        captured_error.value.failure.field_path
        == "model_inputs.nig_shared_plus_shift_configuration.delta_beta"
    )
    assert captured_error.value.failure.message == "must be strictly negative"


def test_risk_run_request_accepts_negative_delta_beta_regression() -> None:
    request_payload = _valid_run_request_payload()
    request_payload["model_inputs"]["nig_shared_plus_shift_configuration"]["delta_beta"] = -0.12

    validated_request = validate_sufficient_statistics_risk_run_request(request_payload)

    assert validated_request.model_inputs is not None
    assert validated_request.model_inputs.nig_shared_plus_shift_configuration.delta_beta == pytest.approx(-0.12)


def test_risk_path_validation_and_serialization_roundtrip() -> None:
    path_payload = _valid_path_payload()

    validated_path = validate_sufficient_statistics_risk_path(path_payload)
    serialized_path = serialize_sufficient_statistics_risk_path(validated_path)
    deserialized_path = deserialize_sufficient_statistics_risk_path(serialized_path)

    assert deserialized_path == validated_path
    assert json.loads(serialized_path)["records"][0]["sufficient_statistics"]["n_plus"] == 3
    assert json.loads(serialized_path)["output_schema_metadata"]["schema_version"] == (
        "risk.sufficient_statistics.path_output_schema_metadata.v1"
    )
    assert json.loads(serialized_path)["provenance"]["schema_version"] == (
        "risk.sufficient_statistics.path_provenance.v1"
    )
    assert json.loads(serialized_path)["diagnostics"]["schema_version"] == (
        "risk.sufficient_statistics.path_diagnostics.v1"
    )
    assert json.loads(serialized_path)["diagnostics"]["cumulative_n_plus"] == 3


def test_risk_path_diagnostics_schema_exposes_required_field_lists() -> None:
    validated_path = validate_sufficient_statistics_risk_path(_valid_path_payload())

    output_schema_metadata = validated_path.output_schema_metadata
    assert output_schema_metadata.required_path_fields == (
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
    assert output_schema_metadata.required_diagnostics_fields == (
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


def test_risk_run_request_rejects_incomplete_payload_with_structured_error() -> None:
    request_payload = _valid_run_request_payload()
    del request_payload["model_inputs"]["equation_specification"]["gamma_severity_mark_configuration"]

    with pytest.raises(SufficientStatisticsRiskValidationError) as captured_error:
        validate_sufficient_statistics_risk_run_request(request_payload)

    assert (
        captured_error.value.failure.field_path
        == "model_inputs.equation_specification.gamma_severity_mark_configuration"
    )
    assert captured_error.value.failure.message == "is required"


def test_risk_run_request_rejects_known_intensity_length_mismatch() -> None:
    request_payload = _valid_run_request_payload()
    request_payload["interval_factors"]["nu_h_known"] = [0.2]

    with pytest.raises(SufficientStatisticsRiskValidationError) as captured_error:
        validate_sufficient_statistics_risk_run_request(request_payload)

    assert captured_error.value.failure.field_path == "interval_factors.nu_h_known"
    assert "must have the same length" in captured_error.value.failure.message


def test_risk_path_rejects_non_sequential_interval_index_with_structured_error() -> None:
    path_payload = _valid_path_payload()
    path_payload["records"][0]["interval_index"] = 1

    with pytest.raises(SufficientStatisticsRiskValidationError) as captured_error:
        validate_sufficient_statistics_risk_path(path_payload)

    assert captured_error.value.failure.error_id == "risk.validation.contract"
    assert captured_error.value.failure.field_path == "records[0].interval_index"
    assert captured_error.value.failure.message == "must equal 0"


def test_risk_path_rejects_mismatched_diagnostics_with_context() -> None:
    path_payload = _valid_path_payload()
    path_payload["diagnostics"] = {
        "schema_version": "risk.sufficient_statistics.path_diagnostics.v1",
        "simulated_interval_count": 1,
        "first_interval_index": 0,
        "last_interval_index": 0,
        "cumulative_a_plus": 0.7,
        "cumulative_a_minus": 0.5,
        "cumulative_n_plus": 5,
        "cumulative_n_minus": 2,
        "cumulative_trading_time_return": 0.01,
        "cumulative_open_jump_return": 0.0,
        "cumulative_closed_gap_return": -0.02,
        "cumulative_total_return": -0.01,
        "max_abs_total_return": 0.01,
    }

    with pytest.raises(SufficientStatisticsRiskValidationError) as captured_error:
        validate_sufficient_statistics_risk_path(path_payload)

    assert captured_error.value.failure.field_path == "diagnostics.cumulative_n_plus"
    assert "must be 3, found 5" in captured_error.value.failure.message
    assert "simulated_interval_count=1" in captured_error.value.failure.message


def test_risk_path_metadata_is_deterministic_across_repeated_validations() -> None:
    first_validated_path = validate_sufficient_statistics_risk_path(_valid_path_payload())
    second_validated_path = validate_sufficient_statistics_risk_path(_valid_path_payload())

    assert first_validated_path.output_schema_metadata == second_validated_path.output_schema_metadata
    assert first_validated_path.provenance == second_validated_path.provenance
    assert first_validated_path.diagnostics == second_validated_path.diagnostics


def _valid_run_request_model() -> SufficientStatisticsRiskRunRequest:
    return SufficientStatisticsRiskRunRequest(
        seed=17,
        horizon_interval_count=2,
        interval_delta=1.0,
        model_inputs=SufficientStatisticsRiskModelInputs(
            equation_specification=SufficientStatisticsRiskEquationSpecification(
                hawkes_kernel_parameters=HawkesKernelParameters(
                    lambda_bar=0.8,
                    beta=1.6,
                    alpha=HawkesKernelLoadings(
                        plus_plus=0.2,
                        plus_minus=0.1,
                        minus_plus=0.1,
                        minus_minus=0.25,
                    ),
                    rho=HawkesKernelLoadings(
                        plus_plus=0.08,
                        plus_minus=0.04,
                        minus_plus=0.05,
                        minus_minus=0.1,
                    ),
                ),
                gamma_severity_mark_configuration=GammaSeverityMarkConfiguration(
                    shape_k=1.8,
                    scale_theta=0.7,
                ),
            ),
            nig_shared_plus_shift_configuration=NigSharedPlusShiftConfiguration(
                alpha=2.3,
                beta_0=-0.8,
                delta=0.7,
                mu=0.1,
                delta_beta=-0.2,
            ),
            initial_excitation_state=HawkesExcitationState(
                y_plus_plus=0.2,
                y_plus_minus=0.1,
                y_minus_plus=0.3,
                y_minus_minus=0.4,
            ),
        ),
        interval_factors=SufficientStatisticsRiskIntervalFactors(
            interval_delta=1.0,
            s_h=(1.1, 0.9),
            nu_h_known=(0.12, 0.04),
            nu_h_surprise=(0.08, 0.06),
        ),
    )


def _valid_run_request_payload() -> dict[str, Any]:
    return _valid_run_request_model().to_payload()


def _valid_path_payload() -> dict[str, Any]:
    path = SufficientStatisticsRiskPath(
        schema_version="risk.sufficient_statistics.path.v1",
        run_request_schema_version="risk.sufficient_statistics.run_request.v1",
        output_schema_metadata=validate_sufficient_statistics_risk_path(
            {
                "seed": 17,
                "interval_delta": 1.0,
                "simulated_interval_count": 1,
                "initial_excitation_state": {
                    "y_plus_plus": 0.2,
                    "y_plus_minus": 0.1,
                    "y_minus_plus": 0.3,
                    "y_minus_minus": 0.4,
                },
                "terminal_excitation_state": {
                    "y_plus_plus": 0.25,
                    "y_plus_minus": 0.12,
                    "y_minus_plus": 0.34,
                    "y_minus_minus": 0.42,
                },
                "records": [
                    {
                        "interval_index": 0,
                        "sufficient_statistics": {
                            "a_plus": 0.7,
                            "a_minus": 0.5,
                            "n_plus": 3,
                            "n_minus": 2,
                        },
                        "return_components": {
                            "buy_side_return": 0.03,
                            "sell_side_return": -0.01,
                            "trading_time_return": 0.02,
                            "open_jump_return": 0.0,
                            "closed_gap_return": -0.02,
                            "total_return": 0.0,
                        },
                    }
                ],
            }
        ).output_schema_metadata,
        provenance=validate_sufficient_statistics_risk_path(
            {
                "seed": 17,
                "interval_delta": 1.0,
                "simulated_interval_count": 1,
                "initial_excitation_state": {
                    "y_plus_plus": 0.2,
                    "y_plus_minus": 0.1,
                    "y_minus_plus": 0.3,
                    "y_minus_minus": 0.4,
                },
                "terminal_excitation_state": {
                    "y_plus_plus": 0.25,
                    "y_plus_minus": 0.12,
                    "y_minus_plus": 0.34,
                    "y_minus_minus": 0.42,
                },
                "records": [
                    {
                        "interval_index": 0,
                        "sufficient_statistics": {
                            "a_plus": 0.7,
                            "a_minus": 0.5,
                            "n_plus": 3,
                            "n_minus": 2,
                        },
                        "return_components": {
                            "buy_side_return": 0.03,
                            "sell_side_return": -0.01,
                            "trading_time_return": 0.02,
                            "open_jump_return": 0.0,
                            "closed_gap_return": -0.02,
                            "total_return": 0.0,
                        },
                    }
                ],
            }
        ).provenance,
        diagnostics=validate_sufficient_statistics_risk_path(
            {
                "seed": 17,
                "interval_delta": 1.0,
                "simulated_interval_count": 1,
                "initial_excitation_state": {
                    "y_plus_plus": 0.2,
                    "y_plus_minus": 0.1,
                    "y_minus_plus": 0.3,
                    "y_minus_minus": 0.4,
                },
                "terminal_excitation_state": {
                    "y_plus_plus": 0.25,
                    "y_plus_minus": 0.12,
                    "y_minus_plus": 0.34,
                    "y_minus_minus": 0.42,
                },
                "records": [
                    {
                        "interval_index": 0,
                        "sufficient_statistics": {
                            "a_plus": 0.7,
                            "a_minus": 0.5,
                            "n_plus": 3,
                            "n_minus": 2,
                        },
                        "return_components": {
                            "buy_side_return": 0.03,
                            "sell_side_return": -0.01,
                            "trading_time_return": 0.02,
                            "open_jump_return": 0.0,
                            "closed_gap_return": -0.02,
                            "total_return": 0.0,
                        },
                    }
                ],
            }
        ).diagnostics,
        seed=17,
        interval_delta=1.0,
        simulated_interval_count=1,
        initial_excitation_state=HawkesExcitationState(
            y_plus_plus=0.2,
            y_plus_minus=0.1,
            y_minus_plus=0.3,
            y_minus_minus=0.4,
        ),
        terminal_excitation_state=HawkesExcitationState(
            y_plus_plus=0.25,
            y_plus_minus=0.12,
            y_minus_plus=0.34,
            y_minus_minus=0.42,
        ),
        records=(
            SufficientStatisticsRiskPathRecord(
                interval_index=0,
                sufficient_statistics=SufficientStatisticsIntervalStatistics(
                    a_plus=0.7,
                    a_minus=0.5,
                    n_plus=3,
                    n_minus=2,
                ),
                return_components=SufficientStatisticsRiskReturnComponents(
                    buy_side_return=0.03,
                    sell_side_return=-0.01,
                    trading_time_return=0.02,
                    open_jump_return=0.0,
                    closed_gap_return=-0.02,
                    total_return=0.0,
                ),
            ),
        ),
    )

    return path.to_payload()
