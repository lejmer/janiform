import json

import pytest

from janiform.risk import (
    SufficientStatisticsRiskValidationError,
    deserialize_sufficient_statistics_risk_path,
    deserialize_sufficient_statistics_risk_run_request,
    serialize_sufficient_statistics_risk_path,
    serialize_sufficient_statistics_risk_run_request,
    validate_sufficient_statistics_risk_path,
    validate_sufficient_statistics_risk_run_request,
)


def test_risk_run_request_validation_and_serialization_roundtrip() -> None:
    request_payload = _valid_run_request_payload()

    validated_request = validate_sufficient_statistics_risk_run_request(request_payload)
    serialized_request = serialize_sufficient_statistics_risk_run_request(request_payload)
    deserialized_request = deserialize_sufficient_statistics_risk_run_request(serialized_request)

    assert deserialized_request == validated_request
    assert validated_request["interval_delta"] == pytest.approx(1.0)
    assert json.loads(serialized_request)["horizon_interval_count"] == 2


def test_risk_path_validation_and_serialization_roundtrip() -> None:
    path_payload = _valid_path_payload()

    validated_path = validate_sufficient_statistics_risk_path(path_payload)
    serialized_path = serialize_sufficient_statistics_risk_path(path_payload)
    deserialized_path = deserialize_sufficient_statistics_risk_path(serialized_path)

    assert deserialized_path == validated_path
    assert json.loads(serialized_path)["records"][0]["sufficient_statistics"]["n_plus"] == 3


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


def test_risk_run_request_rejects_interval_factor_length_mismatch() -> None:
    request_payload = _valid_run_request_payload()
    request_payload["interval_factors"]["nu_h"] = [0.2]

    with pytest.raises(SufficientStatisticsRiskValidationError) as captured_error:
        validate_sufficient_statistics_risk_run_request(request_payload)

    assert captured_error.value.failure.field_path == "interval_factors.nu_h"
    assert "must have the same length" in captured_error.value.failure.message


def test_risk_path_rejects_non_sequential_interval_index_with_structured_error() -> None:
    path_payload = _valid_path_payload()
    path_payload["records"][0]["interval_index"] = 1

    with pytest.raises(SufficientStatisticsRiskValidationError) as captured_error:
        validate_sufficient_statistics_risk_path(path_payload)

    assert captured_error.value.failure.field_path == "records[0].interval_index"
    assert captured_error.value.failure.message == "must equal 0"


def _valid_run_request_payload() -> dict[str, object]:
    return {
        "seed": 17,
        "horizon_interval_count": 2,
        "interval_delta": 1.0,
        "model_inputs": {
            "equation_specification": {
                "hawkes_kernel_parameters": {
                    "lambda_bar": 0.8,
                    "beta": 1.6,
                    "alpha": {
                        "plus_plus": 0.2,
                        "plus_minus": 0.1,
                        "minus_plus": 0.1,
                        "minus_minus": 0.25,
                    },
                    "rho": {
                        "plus_plus": 0.08,
                        "plus_minus": 0.04,
                        "minus_plus": 0.05,
                        "minus_minus": 0.1,
                    },
                },
                "gamma_severity_mark_configuration": {
                    "shape_k": 1.8,
                    "scale_theta": 0.7,
                },
            },
            "nig_shared_plus_shift_configuration": {
                "alpha": 2.3,
                "beta_0": -0.8,
                "delta": 0.7,
                "mu": 0.1,
                "delta_beta": -0.2,
            },
            "initial_excitation_state": {
                "y_plus_plus": 0.2,
                "y_plus_minus": 0.1,
                "y_minus_plus": 0.3,
                "y_minus_minus": 0.4,
            },
        },
        "interval_factors": {
            "s_h": [1.1, 0.9],
            "nu_h": [0.2, 0.1],
        },
    }


def _valid_path_payload() -> dict[str, object]:
    return {
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
            "y_minus_plus": 0.31,
            "y_minus_minus": 0.39,
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
                    "buy_side_return": 0.04,
                    "sell_side_return": -0.03,
                    "trading_time_return": 0.01,
                    "gap_return": -0.02,
                    "total_return": -0.01,
                },
            }
        ],
    }
