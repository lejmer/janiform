import json

import pytest

from janiform.pricing import (
    GammaSeverityMarkConfiguration,
    NigSharedPlusShiftConfiguration,
    VanillaOptionPayoffConfiguration,
    VanillaOptionPayoffType,
    VanillaOptionPricingDiagnostics,
    VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters,
    VanillaOptionPricingRequest,
    VanillaOptionPricingResponse,
    VanillaOptionPricingValidationError,
    deserialize_vanilla_option_pricing_request,
    deserialize_vanilla_option_pricing_response,
    serialize_vanilla_option_pricing_request,
    serialize_vanilla_option_pricing_response,
    validate_vanilla_option_pricing_request,
    validate_vanilla_option_pricing_response,
)
from janiform.simulation import HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters


def test_vanilla_option_request_model_construction_and_roundtrip() -> None:
    request = _valid_request_model()

    serialized_request = serialize_vanilla_option_pricing_request(request)
    deserialized_request = deserialize_vanilla_option_pricing_request(serialized_request)

    assert deserialized_request == request
    assert deserialized_request.interval_delta == pytest.approx(1.0)
    assert json.loads(serialized_request)["payoff"]["payoff_type"] == "call"


def test_vanilla_option_request_accepts_explicit_non_hourly_interval_delta() -> None:
    request = VanillaOptionPricingRequest(
        market_inputs=_valid_request_model().market_inputs,
        payoff=_valid_request_model().payoff,
        boundary_state=_valid_request_model().boundary_state,
        model_parameters=_valid_request_model().model_parameters,
        interval_delta=0.5,
    )

    assert request.interval_delta == pytest.approx(0.5)


def test_vanilla_option_request_validation_defaults_interval_delta() -> None:
    request_payload = _valid_request_payload()
    del request_payload["interval_delta"]

    validated_request = validate_vanilla_option_pricing_request(request_payload)

    assert validated_request.interval_delta == pytest.approx(1.0)


def test_vanilla_option_response_model_construction_and_roundtrip() -> None:
    response = VanillaOptionPricingResponse(
        price=12.5,
        diagnostics=VanillaOptionPricingDiagnostics(
            forward_price=104.3,
            discount_factor=0.97,
            maturity_interval=0.5,
            boundary_state_norm=0.55,
        ),
    )

    serialized_response = serialize_vanilla_option_pricing_response(response)
    deserialized_response = deserialize_vanilla_option_pricing_response(serialized_response)

    assert deserialized_response == response
    assert json.loads(serialized_response)["price"] == pytest.approx(12.5)


def test_vanilla_option_request_rejects_incomplete_payload_with_structured_error() -> None:
    request_payload = _valid_request_payload()
    del request_payload["market_inputs"]["strike"]

    with pytest.raises(VanillaOptionPricingValidationError) as captured_error:
        validate_vanilla_option_pricing_request(request_payload)

    assert captured_error.value.failure.field_path == "market_inputs.strike"
    assert captured_error.value.failure.message == "is required"


def test_vanilla_option_request_rejects_negative_boundary_state_with_structured_error() -> None:
    request_payload = _valid_request_payload()
    request_payload["boundary_state"]["y_plus_plus"] = -0.25

    with pytest.raises(VanillaOptionPricingValidationError) as captured_error:
        validate_vanilla_option_pricing_request(request_payload)

    assert captured_error.value.failure.field_path == "boundary_state.y_plus_plus"
    assert captured_error.value.failure.message == "must be non-negative"


def test_vanilla_option_request_rejects_non_positive_interval_delta() -> None:
    request_payload = _valid_request_payload()
    request_payload["interval_delta"] = 0.0

    with pytest.raises(VanillaOptionPricingValidationError) as captured_error:
        validate_vanilla_option_pricing_request(request_payload)

    assert captured_error.value.failure.field_path == "interval_delta"
    assert captured_error.value.failure.message == "must be strictly positive"


def test_vanilla_option_request_rejects_non_negative_delta_beta_with_field_path() -> None:
    request_payload = _valid_request_payload()
    request_payload["model_parameters"]["nig_shared_plus_shift_configuration"]["delta_beta"] = 0.0

    with pytest.raises(VanillaOptionPricingValidationError) as captured_error:
        validate_vanilla_option_pricing_request(request_payload)

    assert (
        captured_error.value.failure.field_path
        == "model_parameters.nig_shared_plus_shift_configuration.delta_beta"
    )
    assert captured_error.value.failure.message == "must be strictly negative"


def test_vanilla_option_request_accepts_negative_delta_beta_regression() -> None:
    request_payload = _valid_request_payload()
    request_payload["model_parameters"]["nig_shared_plus_shift_configuration"]["delta_beta"] = -0.15

    validated_request = validate_vanilla_option_pricing_request(request_payload)

    assert validated_request.model_parameters.nig_shared_plus_shift_configuration.delta_beta == pytest.approx(-0.15)


def test_vanilla_option_response_rejects_negative_price_with_structured_error() -> None:
    response_payload = _valid_response_payload()
    response_payload["price"] = -0.01

    with pytest.raises(VanillaOptionPricingValidationError) as captured_error:
        validate_vanilla_option_pricing_response(response_payload)

    assert captured_error.value.failure.field_path == "price"
    assert captured_error.value.failure.message == "must be non-negative"


def _valid_request_model() -> VanillaOptionPricingRequest:
    return VanillaOptionPricingRequest(
        market_inputs=VanillaOptionPricingMarketInputs(
            spot=101.0,
            strike=100.0,
            maturity_interval=0.5,
            risk_free_rate=0.02,
            dividend_yield=0.01,
        ),
        payoff=VanillaOptionPayoffConfiguration(payoff_type=VanillaOptionPayoffType.CALL),
        boundary_state=HawkesExcitationState(
            y_plus_plus=0.2,
            y_plus_minus=0.1,
            y_minus_plus=0.3,
            y_minus_minus=0.4,
        ),
        model_parameters=VanillaOptionPricingModelParameters(
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
                    plus_plus=0.1,
                    plus_minus=0.05,
                    minus_plus=0.05,
                    minus_minus=0.1,
                ),
            ),
            gamma_severity_mark_configuration=GammaSeverityMarkConfiguration(
                shape_k=1.8,
                scale_theta=0.7,
            ),
            nig_shared_plus_shift_configuration=NigSharedPlusShiftConfiguration(
                alpha=2.3,
                beta_0=-0.8,
                delta=0.7,
                mu=0.1,
                delta_beta=-0.2,
            ),
        ),
    )


def _valid_request_payload() -> dict[str, object]:
    return _valid_request_model().to_payload()


def _valid_response_payload() -> dict[str, object]:
    return {
        "price": 12.5,
        "diagnostics": {
            "forward_price": 104.3,
            "discount_factor": 0.97,
            "maturity_interval": 0.5,
            "boundary_state_norm": 0.55,
        },
    }
