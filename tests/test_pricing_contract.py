import json
from typing import Any

import pytest

from janiform.pricing import (
    AnchoredLocalTiltAdmissibilityStatus,
    AnchoredLocalTiltBoundaryStateSummary,
    AnchoredLocalTiltDomainStatus,
    AnchoredLocalTiltSolveDiagnostics,
    AnchoredVanillaOptionPricingBoundaryInput,
    FourierInversionContour,
    FourierInversionEngineSettings,
    FourierInversionIntegrationStabilityControls,
    FourierInversionNumericQualityGate,
    FourierInversionNumericSettings,
    FourierInversionQuadratureRule,
    MeanReversionAnchorModel,
    MeanReversionBoundaryState,
    MeanReversionFittedParameters,
    NigSharedPlusShiftConfiguration,
    VanillaOptionImpliedVolatilityInversionStatus,
    VanillaOptionPayoffConfiguration,
    VanillaOptionPayoffType,
    VanillaOptionPricingDiagnostics,
    VanillaOptionPricingExecutionInput,
    VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters,
    VanillaOptionPricingOptionalOutputSelection,
    VanillaOptionPricingRequest,
    VanillaOptionPricingResponse,
    VanillaOptionPricingRunResult,
    VanillaOptionPricingValidationError,
    deserialize_vanilla_option_pricing_request,
    deserialize_vanilla_option_pricing_response,
    deserialize_vanilla_option_pricing_run_result,
    run_vanilla_option_pricing,
    serialize_vanilla_option_pricing_request,
    serialize_vanilla_option_pricing_response,
    serialize_vanilla_option_pricing_run_result,
    validate_vanilla_option_pricing_request,
    validate_vanilla_option_pricing_response,
)
from janiform.simulation import HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters
from janiform.simulation.coarse import (
    CoupledGapMarkConfiguration,
    DoubleExponentialBaseShockConfiguration,
    KnownNewsArrivalConfiguration,
    NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration,
    NewsProcessConfiguration,
    SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
)
from tests.support.hourly_factor_contracts import build_hourly_factor_contract
from tests.support.pricing import (
    build_anchored_request_model,
    build_anchored_runtime_execution_input_model,
)


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
        boundary_input=_disabled_boundary_input(
            interval_delta=0.5,
            hawkes_boundary_state=_valid_request_model().boundary_input.hawkes_boundary_state,
        ),
        model_parameters=_valid_request_model().model_parameters,
        interval_delta=0.5,
    )

    assert request.interval_delta == pytest.approx(0.5)


def test_vanilla_option_request_validation_defaults_interval_delta() -> None:
    request_payload = _valid_request_payload()
    del request_payload["interval_delta"]

    validated_request = validate_vanilla_option_pricing_request(request_payload)

    assert validated_request.interval_delta == pytest.approx(1.0)


def test_vanilla_option_request_roundtrip_preserves_fourier_inversion_settings() -> None:
    request = VanillaOptionPricingRequest(
        market_inputs=_valid_request_model().market_inputs,
        payoff=_valid_request_model().payoff,
        boundary_input=_valid_request_model().boundary_input,
        model_parameters=_valid_request_model().model_parameters,
        fourier_inversion_settings=FourierInversionEngineSettings(
            contour=FourierInversionContour(damping_factor=1.2),
            numeric_settings=FourierInversionNumericSettings(
                integration_upper_bound=80.0,
                integration_step_count=2_048,
                quadrature_rule=FourierInversionQuadratureRule.TRAPEZOID,
            ),
            stability_controls=FourierInversionIntegrationStabilityControls(
                convergence_absolute_tolerance=1.0e-6,
                convergence_relative_tolerance=1.0e-5,
                terminal_integrand_absolute_tolerance=2.5e-4,
                terminal_integrand_ratio_tolerance=5.0e-3,
                quality_gate=FourierInversionNumericQualityGate.ENFORCE_FAILURE,
            ),
        ),
    )

    serialized_request = serialize_vanilla_option_pricing_request(request)
    deserialized_request = deserialize_vanilla_option_pricing_request(serialized_request)

    assert deserialized_request == request


def test_vanilla_option_response_model_construction_and_roundtrip() -> None:
    response = VanillaOptionPricingResponse(
        price=12.5,
        diagnostics=VanillaOptionPricingDiagnostics(
            forward_price=104.3,
            discount_factor=0.97,
            maturity_interval=0.5,
            boundary_state_norm=0.55,
        ),
        implied_volatility=0.2,
        implied_volatility_inversion_status=VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED,
    )

    serialized_response = serialize_vanilla_option_pricing_response(response)
    deserialized_response = deserialize_vanilla_option_pricing_response(serialized_response)

    assert deserialized_response == response
    assert json.loads(serialized_response)["price"] == pytest.approx(12.5)
    assert json.loads(serialized_response)["implied_volatility"] == pytest.approx(0.2)


def test_vanilla_option_request_rejects_incomplete_payload_with_structured_error() -> None:
    request_payload = _valid_request_payload()
    del request_payload["market_inputs"]["strike"]

    with pytest.raises(VanillaOptionPricingValidationError) as captured_error:
        validate_vanilla_option_pricing_request(request_payload)

    assert captured_error.value.failure.error_id == "pricing.validation.contract"
    assert captured_error.value.failure.workflow == "pricing"
    assert captured_error.value.failure.entrypoint == "contract"
    assert captured_error.value.failure.field_path == "market_inputs.strike"
    assert captured_error.value.failure.message == "is required"


def test_vanilla_option_request_rejects_negative_boundary_state_with_structured_error() -> None:
    request_payload = _valid_request_payload()
    request_payload["boundary_input"]["hawkes_boundary_state"]["y_plus_plus"] = -0.25

    with pytest.raises(VanillaOptionPricingValidationError) as captured_error:
        validate_vanilla_option_pricing_request(request_payload)

    assert captured_error.value.failure.field_path == "boundary_input.hawkes_boundary_state.y_plus_plus"
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

    assert captured_error.value.failure.error_id == "pricing.validation.contract"
    assert captured_error.value.failure.field_path == "price"
    assert captured_error.value.failure.message == "must be non-negative"


def test_vanilla_option_response_rejects_missing_implied_volatility_for_successful_status() -> None:
    response_payload = _valid_response_payload()
    response_payload["implied_volatility"] = None

    with pytest.raises(VanillaOptionPricingValidationError) as captured_error:
        validate_vanilla_option_pricing_response(response_payload)

    assert captured_error.value.failure.field_path == "vanilla_option_pricing_response.implied_volatility"
    assert "must be present" in captured_error.value.failure.message


def test_vanilla_option_pricing_execution_input_keeps_native_hourly_factor_contract() -> None:
    execution_input = _valid_execution_input_model()

    assert execution_input.hourly_factor_contract.known_component_nu_h[0] == pytest.approx(0.03)
    assert execution_input.hourly_factor_contract.surprise_component_nu_h[0] == pytest.approx(0.05)


def test_vanilla_option_pricing_execution_input_rejects_empty_hourly_factor_contract() -> None:
    request = _valid_request_model()

    with pytest.raises(VanillaOptionPricingValidationError) as captured_error:
        VanillaOptionPricingExecutionInput(
            request=request,
            hourly_factor_contract=build_hourly_factor_contract(
                request_start_utc_nanos=1_706_882_400_000_000_000,
                s_h=(),
                known_component_nu_h=(),
                surprise_component_nu_h=(),
            ),
        )

    assert captured_error.value.failure.field_path == "hourly_factor_contract.interval_count"
    assert captured_error.value.failure.message == "must contain at least one interval"


def test_anchored_boundary_input_roundtrip_and_validation() -> None:
    boundary_input = AnchoredVanillaOptionPricingBoundaryInput(
        interval_index=0,
        hawkes_boundary_state=HawkesExcitationState(
            y_plus_plus=0.1,
            y_plus_minus=0.0,
            y_minus_plus=0.2,
            y_minus_minus=0.3,
        ),
        mean_reversion_fitted_parameters=MeanReversionFittedParameters(
            kappa=0.4,
            anchor_model=MeanReversionAnchorModel.zero_gain_endogenous_core_pressure_level(),
        ),
        interval_delta=0.5,
        observed_boundary_state=MeanReversionBoundaryState(
            observed_log_price=0.15,
            anchor_value=0.0,
            pressure_state=0.0,
            deviation=0.15,
        ),
    )

    round_tripped = AnchoredVanillaOptionPricingBoundaryInput.from_mapping(
        json.loads(boundary_input.to_json()),
        field_path="boundary_input",
    )

    assert round_tripped == boundary_input
    assert round_tripped.local_martingale_deviation_adjustment < 0.0


def test_anchored_boundary_input_accepts_endogenous_anchor_policy_boundary_state() -> None:
    boundary_payload = _disabled_boundary_input(
        interval_delta=1.0,
        hawkes_boundary_state=HawkesExcitationState(
            y_plus_plus=0.0,
            y_plus_minus=0.0,
            y_minus_plus=0.0,
            y_minus_minus=0.0,
        ),
    ).to_payload()

    boundary_payload["mean_reversion_fitted_parameters"] = MeanReversionFittedParameters(
        kappa=0.3,
        anchor_model=MeanReversionAnchorModel.zero_gain_endogenous_core_pressure_level(),
    ).to_payload()

    boundary_payload["observed_boundary_state"] = MeanReversionBoundaryState(
        observed_log_price=0.15,
        anchor_value=0.1,
        pressure_state=0.0,
        deviation=0.05,
    ).to_payload()

    boundary_input = AnchoredVanillaOptionPricingBoundaryInput.from_mapping(
        boundary_payload,
        field_path="boundary_input",
    )

    assert boundary_input.current_anchor_value == pytest.approx(0.1)
    assert boundary_input.interval_delta == pytest.approx(1.0)
    assert boundary_input.local_martingale_deviation_adjustment < 0.0


def test_anchored_vanilla_option_request_roundtrip_preserves_observed_boundary_contract() -> None:
    request = build_anchored_request_model()

    serialized_request = serialize_vanilla_option_pricing_request(request)
    deserialized_request = deserialize_vanilla_option_pricing_request(serialized_request)

    assert deserialized_request == request
    assert deserialized_request.boundary_input.mean_reversion_fitted_parameters.kappa == pytest.approx(0.35)
    assert deserialized_request.boundary_input.observed_boundary_state.deviation == pytest.approx(0.15)


def test_anchored_boundary_input_accepts_shifted_observed_anchor_state() -> None:
    boundary_payload = _disabled_boundary_input(
        interval_delta=1.0,
        hawkes_boundary_state=HawkesExcitationState(
            y_plus_plus=0.0,
            y_plus_minus=0.0,
            y_minus_plus=0.0,
            y_minus_minus=0.0,
        ),
    ).to_payload()

    boundary_payload["observed_boundary_state"]["anchor_value"] = 0.5
    boundary_payload["observed_boundary_state"]["observed_log_price"] = 0.5

    boundary_input = AnchoredVanillaOptionPricingBoundaryInput.from_mapping(
        boundary_payload,
        field_path="boundary_input",
    )

    assert boundary_input.current_anchor_value == pytest.approx(0.5)
    assert boundary_input.observed_boundary_state.deviation == pytest.approx(0.0)


def test_anchored_local_tilt_diagnostics_roundtrip() -> None:
    diagnostics = AnchoredLocalTiltSolveDiagnostics(
        boundary_state_summary=AnchoredLocalTiltBoundaryStateSummary(
            interval_index=0,
            hawkes_boundary_state=HawkesExcitationState(
                y_plus_plus=0.0,
                y_plus_minus=0.0,
                y_minus_plus=0.0,
                y_minus_minus=0.0,
            ),
            observed_boundary_state=MeanReversionBoundaryState(
                observed_log_price=0.1,
                anchor_value=0.0,
                pressure_state=0.0,
                deviation=0.1,
            ),
            current_anchor_value=0.0,
            interval_delta=0.5,
            kappa=0.4,
            deviation_decay_factor=0.8187307530779818,
            local_martingale_deviation_adjustment=-0.01812692469220182,
        ),
        target_log_moment_exponent=0.02,
        observed_log_moment_exponent=0.02,
        theta=-0.05,
        residual=0.0,
        absolute_residual=0.0,
        tolerance=1.0e-8,
        domain_status=AnchoredLocalTiltDomainStatus.IN_DOMAIN,
        admissibility_status=AnchoredLocalTiltAdmissibilityStatus.ADMISSIBLE,
        policy="solve_state_dependent_local_tilt_against_endogenous_q_martingale_target.v1",
    )

    round_tripped = AnchoredLocalTiltSolveDiagnostics.from_mapping(
        json.loads(diagnostics.to_json())
    )

    assert round_tripped == diagnostics


def test_run_vanilla_option_pricing_executes_native_workflow_with_optional_outputs() -> None:
    run_result = run_vanilla_option_pricing(_valid_runtime_execution_input_model())

    assert isinstance(run_result, VanillaOptionPricingRunResult)
    assert run_result.response.price >= 0.0
    assert run_result.response.implied_volatility is not None
    assert (
        run_result.response.implied_volatility_inversion_status
        is VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED
    )
    assert run_result.response.diagnostics.forward_price > 0.0
    assert run_result.run_diagnostics is not None
    assert run_result.run_diagnostics.transform is not None
    assert run_result.run_diagnostics.integration_quality is not None
    assert run_result.calibration_hooks is not None
    assert run_result.calibration_hooks.option_price == pytest.approx(run_result.response.price)


def test_vanilla_option_pricing_run_result_roundtrip_preserves_required_schema_fields() -> None:
    run_result = run_vanilla_option_pricing(_valid_runtime_execution_input_model())

    serialized_run_result = serialize_vanilla_option_pricing_run_result(run_result)
    deserialized_run_result = deserialize_vanilla_option_pricing_run_result(serialized_run_result)

    assert deserialized_run_result == run_result
    assert deserialized_run_result.schema_version == "pricing.vanilla_option.run_result.v1"
    assert (
        deserialized_run_result.output_schema_metadata.required_run_result_fields
        == (
            "schema_version",
            "output_schema_metadata",
            "response",
            "run_diagnostics",
            "calibration_hooks",
        )
    )


def test_anchored_pricing_run_result_roundtrip_preserves_anchored_recursion_diagnostics() -> None:
    run_result = run_vanilla_option_pricing(build_anchored_runtime_execution_input_model())

    serialized_run_result = serialize_vanilla_option_pricing_run_result(run_result)
    deserialized_run_result = deserialize_vanilla_option_pricing_run_result(serialized_run_result)

    assert deserialized_run_result == run_result
    assert deserialized_run_result.run_diagnostics is not None
    assert deserialized_run_result.run_diagnostics.anchored_recursion is not None
    assert (
        deserialized_run_result.run_diagnostics.anchored_recursion["refined_deviation_grid_node_count"]
        >= deserialized_run_result.run_diagnostics.anchored_recursion["deviation_grid_node_count"]
    )


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
        boundary_input=_disabled_boundary_input(
            interval_delta=1.0,
            hawkes_boundary_state=HawkesExcitationState(
                y_plus_plus=0.2,
                y_plus_minus=0.1,
                y_minus_plus=0.3,
                y_minus_minus=0.4,
            ),
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
                news_process_configuration=_valid_news_process_configuration(),
                nig_shared_plus_shift_configuration=NigSharedPlusShiftConfiguration(
                    alpha=2.3,
                    beta_0=-0.8,
                delta=0.7,
                mu=0.1,
                delta_beta=-0.2,
            ),
        ),
    )


def _valid_execution_input_model() -> VanillaOptionPricingExecutionInput:
    return VanillaOptionPricingExecutionInput(
        request=VanillaOptionPricingRequest(
            market_inputs=_valid_request_model().market_inputs,
            payoff=_valid_request_model().payoff,
            boundary_input=_disabled_boundary_input(
                interval_delta=0.5,
                hawkes_boundary_state=_valid_request_model().boundary_input.hawkes_boundary_state,
            ),
            model_parameters=_valid_request_model().model_parameters,
            interval_delta=0.5,
        ),
        hourly_factor_contract=build_hourly_factor_contract(
            request_start_utc_nanos=1_706_882_400_000_000_000,
            interval_duration_nanos=1_800_000_000_000,
            s_h=(1.1,),
            known_component_nu_h=(0.03,),
            surprise_component_nu_h=(0.05,),
        ),
        optional_output_selection=VanillaOptionPricingOptionalOutputSelection(
            include_transform_diagnostics=True,
            include_integration_quality_diagnostics=True,
            include_calibration_hooks=True,
        ),
    )


def _valid_runtime_execution_input_model() -> VanillaOptionPricingExecutionInput:
    return VanillaOptionPricingExecutionInput(
        request=VanillaOptionPricingRequest(
            market_inputs=VanillaOptionPricingMarketInputs(
                spot=101.0,
                strike=100.0,
                maturity_interval=1.0,
                risk_free_rate=0.02,
                dividend_yield=0.01,
            ),
            payoff=VanillaOptionPayoffConfiguration(payoff_type=VanillaOptionPayoffType.CALL),
            boundary_input=_disabled_boundary_input(
                interval_delta=1.0,
                hawkes_boundary_state=HawkesExcitationState(
                    y_plus_plus=0.0,
                    y_plus_minus=0.0,
                    y_minus_plus=0.0,
                    y_minus_minus=0.0,
                ),
            ),
            model_parameters=_valid_request_model().model_parameters,
        ),
        hourly_factor_contract=build_hourly_factor_contract(
            request_start_utc_nanos=1_706_882_400_000_000_000,
            s_h=(1.0,),
            known_component_nu_h=(0.0,),
            surprise_component_nu_h=(0.0,),
        ),
        optional_output_selection=VanillaOptionPricingOptionalOutputSelection(
            include_transform_diagnostics=True,
            include_integration_quality_diagnostics=True,
            include_calibration_hooks=True,
        ),
    )


def _valid_request_payload() -> dict[str, Any]:
    return _valid_request_model().to_payload()


def _valid_response_payload() -> dict[str, Any]:
    return {
        "price": 12.5,
        "diagnostics": {
            "forward_price": 104.3,
            "discount_factor": 0.97,
            "maturity_interval": 0.5,
            "boundary_state_norm": 0.55,
        },
        "implied_volatility": 0.2,
        "implied_volatility_inversion_status": "succeeded",
    }


def _disabled_boundary_input(
    *,
    interval_delta: float,
    hawkes_boundary_state: HawkesExcitationState,
) -> AnchoredVanillaOptionPricingBoundaryInput:
    return AnchoredVanillaOptionPricingBoundaryInput(
        interval_index=0,
        hawkes_boundary_state=hawkes_boundary_state,
        mean_reversion_fitted_parameters=MeanReversionFittedParameters(
            kappa=0.0,
            anchor_model=MeanReversionAnchorModel.zero_gain_endogenous_core_pressure_level(),
        ),
        interval_delta=interval_delta,
        observed_boundary_state=MeanReversionBoundaryState(
            observed_log_price=0.0,
            anchor_value=0.0,
            pressure_state=0.0,
            deviation=0.0,
        ),
    )


def _valid_gamma_severity_mark_configuration() -> Any:
    from janiform.pricing import GammaSeverityMarkConfiguration

    return GammaSeverityMarkConfiguration(
        shape_k=1.8,
        scale_theta=0.7,
    )


def _valid_news_process_configuration() -> NewsProcessConfiguration:
    return NewsProcessConfiguration(
        schema_version="news.process.configuration.v1",
        arrivals=NewsArrivalConfiguration(
            known=KnownNewsArrivalConfiguration(
                scheduled_event_timestamps_utc_nanos=(),
            ),
            surprise=SurpriseNewsArrivalConfiguration(
                baseline_intensity=0.75,
            ),
        ),
        shared_mark_law=SharedNewsMarkLawConfiguration(
            gamma_severity_marks=_valid_gamma_severity_mark_configuration(),
            double_exponential_base_shock=DoubleExponentialBaseShockConfiguration(
                probability_positive=0.35,
                eta_plus=2.1,
                eta_minus=3.0,
            ),
            coupled_gap_marks=CoupledGapMarkConfiguration(
                sigma_g=0.6,
            ),
        ),
        known_component_mark_scales=NewsComponentMarkScaleConfiguration(
            activity_coupling_scale=0.0,
            return_jump_scale=1.0,
        ),
        surprise_component_mark_scales=NewsComponentMarkScaleConfiguration(
            activity_coupling_scale=0.0,
            return_jump_scale=1.0,
        ),
    )
