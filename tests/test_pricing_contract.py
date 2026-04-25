import json
from dataclasses import replace
from typing import Any

import pytest

from janiform.contracts import parse_json_object
from janiform.pricing import (
    AnchoredLocalTiltAdmissibilityStatus,
    AnchoredLocalTiltBoundaryStateSummary,
    AnchoredLocalTiltDomainStatus,
    AnchoredLocalTiltSolveDiagnostics,
    AnchoredVanillaOptionPricingBoundaryInput,
    MeanReversionAnchorModel,
    MeanReversionBoundaryState,
    MeanReversionFittedParameters,
    NigSharedPlusShiftConfiguration,
    PricingIntervalSchedule,
    PricingIntervalScheduleMetadata,
    VanillaOptionImpliedVolatilityInversionStatus,
    VanillaOptionPayoffConfiguration,
    VanillaOptionPayoffType,
    VanillaOptionPricingDiagnostics,
    VanillaOptionPricingFailureStage,
    VanillaOptionPricingExecutionInput,
    VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters,
    VanillaOptionPricingOptionalOutputSelection,
    VanillaOptionPricingRequest,
    VanillaOptionPricingResponse,
    VanillaOptionPricingRunResult,
    VanillaOptionPricingStatus,
    VanillaOptionSurfacePricingExecutionInput,
    VanillaOptionSurfacePricingNodeTable,
    VanillaOptionSurfacePricingRequest,
    VanillaOptionSurfacePricingResponse,
    VanillaOptionSurfacePricingRunResult,
    VanillaOptionPricingValidationError,
    run_vanilla_option_pricing,
    run_vanilla_option_surface_pricing,
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
    build_anchored_surface_execution_input_model,
    build_anchored_surface_request_model,
    build_anchored_runtime_execution_input_model,
)
from tests.support.time import REFERENCE_PRICING_TIMESTAMP_UTC_NANOS


def test_vanilla_option_request_model_construction_and_roundtrip() -> None:
    request = _valid_request_model()

    serialized_request = request.to_json()
    deserialized_request = VanillaOptionPricingRequest.from_mapping(
        parse_json_object(
            serialized_request,
            "vanilla option request",
            lambda field_path, message: ValueError(f"{field_path}: {message}"),
        )
    )

    assert deserialized_request == request
    assert deserialized_request.interval_delta == pytest.approx(1.0)
    assert json.loads(serialized_request)["payoff"]["payoff_type"] == "call"


def test_parse_json_object_rejects_malformed_json() -> None:
    with pytest.raises(ValueError, match="payload: must be valid JSON: unexpected character"):
        parse_json_object(
            "{foo",
            "payload",
            lambda field_path, message: ValueError(f"{field_path}: {message}"),
        )


def test_parse_json_object_rejects_non_object_payloads() -> None:
    with pytest.raises(ValueError, match="payload: must decode to a JSON object"):
        parse_json_object(
            "[1, 2, 3]",
            "payload",
            lambda field_path, message: ValueError(f"{field_path}: {message}"),
        )


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


def test_vanilla_option_request_validation_requires_explicit_interval_delta() -> None:
    request_payload = _valid_request_payload()
    del request_payload["interval_delta"]

    with pytest.raises(
        VanillaOptionPricingValidationError,
        match='field_path="interval_delta" message="is required"',
    ):
        VanillaOptionPricingRequest.from_mapping(request_payload)


def test_vanilla_option_request_roundtrip_preserves_public_contract_fields() -> None:
    request = VanillaOptionPricingRequest(
        market_inputs=_valid_request_model().market_inputs,
        payoff=_valid_request_model().payoff,
        boundary_input=_valid_request_model().boundary_input,
        model_parameters=_valid_request_model().model_parameters,
        interval_delta=_valid_request_model().interval_delta,
    )

    serialized_request = request.to_json()
    deserialized_request = VanillaOptionPricingRequest.from_mapping(
        parse_json_object(
            serialized_request,
            "vanilla option request",
            lambda field_path, message: ValueError(f"{field_path}: {message}"),
        )
    )

    assert deserialized_request == request


def test_vanilla_option_response_model_construction_and_roundtrip() -> None:
    response = VanillaOptionPricingResponse(
        pricing_status=VanillaOptionPricingStatus.SUCCEEDED,
        pricing_failure_diagnostic=None,
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

    serialized_response = response.to_json()
    deserialized_response = VanillaOptionPricingResponse.from_mapping(
        parse_json_object(
            serialized_response,
            "vanilla option response",
            lambda field_path, message: ValueError(f"{field_path}: {message}"),
        )
    )

    assert deserialized_response == response
    assert json.loads(serialized_response)["price"] == pytest.approx(12.5)
    assert json.loads(serialized_response)["implied_volatility"] == pytest.approx(0.2)


def test_vanilla_option_surface_request_model_construction_and_roundtrip() -> None:
    request = build_anchored_surface_request_model()

    serialized_request = request.to_json()
    deserialized_request = VanillaOptionSurfacePricingRequest.from_mapping(
        parse_json_object(
            serialized_request,
            "vanilla option surface request",
            lambda field_path, message: ValueError(f"{field_path}: {message}"),
        )
    )

    assert deserialized_request == request
    assert deserialized_request.market_inputs.shape == (2, 3)
    assert (
        json.loads(serialized_request)["market_inputs"]["maturity_intervals"]
        == [1.0, 2.0]
    )


def test_vanilla_option_surface_request_validation_requires_explicit_interval_delta() -> None:
    request_payload = build_anchored_surface_request_model().to_payload()
    del request_payload["interval_delta"]

    with pytest.raises(
        VanillaOptionPricingValidationError,
        match='field_path="interval_delta" message="is required"',
    ):
        VanillaOptionSurfacePricingRequest.from_mapping(request_payload)


def test_vanilla_option_surface_response_derives_node_table_and_roundtrip() -> None:
    response = VanillaOptionSurfacePricingResponse(
        spot=100.0,
        strikes=(95.0, 105.0),
        maturity_intervals=(1.0, 2.0),
        payoff_type=VanillaOptionPayoffType.CALL,
        risk_free_rate=0.01,
        dividend_yield=0.0,
        price_grid=((8.0, 6.0), (10.0, 7.0)),
        implied_volatility_grid=((0.22, 0.19), (0.24, 0.2)),
        implied_volatility_inversion_status_grid=(
            (
                VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED,
                VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED,
            ),
            (
                VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED,
                VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED,
            ),
        ),
        pricing_status_grid=(
            (
                VanillaOptionPricingStatus.SUCCEEDED,
                VanillaOptionPricingStatus.SUCCEEDED,
            ),
            (
                VanillaOptionPricingStatus.SUCCEEDED,
                VanillaOptionPricingStatus.SUCCEEDED,
            ),
        ),
        pricing_failure_stage_grid=((None, None), (None, None)),
        pricing_failure_reason_grid=((None, None), (None, None)),
        forward_price_grid=((100.5, 100.5), (101.0, 101.0)),
        discount_factor_grid=((0.99, 0.99), (0.98, 0.98)),
        boundary_state_norm_grid=((0.1, 0.2), (0.3, 0.4)),
        node_table=VanillaOptionSurfacePricingNodeTable(
            strikes=(95.0, 105.0, 95.0, 105.0),
            maturity_intervals=(1.0, 1.0, 2.0, 2.0),
            option_types=(
                VanillaOptionPayoffType.CALL,
                VanillaOptionPayoffType.CALL,
                VanillaOptionPayoffType.CALL,
                VanillaOptionPayoffType.CALL,
            ),
            model_prices=(8.0, 6.0, 10.0, 7.0),
            implied_volatilities=(0.22, 0.19, 0.24, 0.2),
            implied_volatility_inversion_statuses=(
                VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED,
                VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED,
                VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED,
                VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED,
            ),
            pricing_statuses=(
                VanillaOptionPricingStatus.SUCCEEDED,
                VanillaOptionPricingStatus.SUCCEEDED,
                VanillaOptionPricingStatus.SUCCEEDED,
                VanillaOptionPricingStatus.SUCCEEDED,
            ),
            pricing_failure_stages=(None, None, None, None),
            pricing_failure_reasons=(None, None, None, None),
            forward_prices=(100.5, 100.5, 101.0, 101.0),
            discount_factors=(0.99, 0.99, 0.98, 0.98),
            boundary_state_norms=(0.1, 0.2, 0.3, 0.4),
        ),
    )

    serialized_response = response.to_json()
    deserialized_response = VanillaOptionSurfacePricingResponse.from_mapping(
        parse_json_object(
            serialized_response,
            "vanilla option surface response",
            lambda field_path, message: ValueError(f"{field_path}: {message}"),
        )
    )
    native_style_deserialized_response = VanillaOptionSurfacePricingResponse.from_mapping(
        {
            key: value
            for key, value in response.to_payload().items()
            if key != "node_table"
        }
    )

    assert deserialized_response == response
    assert native_style_deserialized_response == response
    assert native_style_deserialized_response.node_count == 4
    assert native_style_deserialized_response.to_records()[0] == {
        "strike": 95.0,
        "maturity_interval": 1.0,
        "option_type": "call",
        "model_price": 8.0,
        "implied_volatility": 0.22,
        "implied_volatility_inversion_status": "succeeded",
        "pricing_status": "succeeded",
        "pricing_failure_stage": None,
        "pricing_failure_reason": None,
        "forward_price": 100.5,
        "discount_factor": 0.99,
        "boundary_state_norm": 0.1,
    }


def test_vanilla_option_request_rejects_incomplete_payload_with_structured_error() -> None:
    request_payload = _valid_request_payload()
    del request_payload["market_inputs"]["strikes"]

    with pytest.raises(VanillaOptionPricingValidationError) as captured_error:
        VanillaOptionPricingRequest.from_mapping(request_payload)

    assert captured_error.value.failure.error_id == "pricing.validation.contract"
    assert captured_error.value.failure.workflow == "pricing"
    assert captured_error.value.failure.entrypoint == "contract"
    assert captured_error.value.failure.field_path == "market_inputs.strikes"
    assert captured_error.value.failure.message == "is required"


def test_vanilla_option_request_rejects_negative_boundary_state_with_structured_error() -> None:
    request_payload = _valid_request_payload()
    request_payload["boundary_input"]["hawkes_boundary_state"]["y_plus_plus"] = -0.25

    with pytest.raises(VanillaOptionPricingValidationError) as captured_error:
        VanillaOptionPricingRequest.from_mapping(request_payload)

    assert captured_error.value.failure.field_path == "boundary_input.hawkes_boundary_state.y_plus_plus"
    assert captured_error.value.failure.message == "must be non-negative"


def test_vanilla_option_request_rejects_non_positive_interval_delta() -> None:
    request_payload = _valid_request_payload()
    request_payload["interval_delta"] = 0.0

    with pytest.raises(VanillaOptionPricingValidationError) as captured_error:
        VanillaOptionPricingRequest.from_mapping(request_payload)

    assert captured_error.value.failure.field_path == "interval_delta"
    assert captured_error.value.failure.message == "must be strictly positive"


def test_vanilla_option_request_rejects_non_negative_delta_beta_with_field_path() -> None:
    request_payload = _valid_request_payload()
    request_payload["model_parameters"]["nig_shared_plus_shift_configuration"]["delta_beta"] = 0.0

    with pytest.raises(VanillaOptionPricingValidationError) as captured_error:
        VanillaOptionPricingRequest.from_mapping(request_payload)

    assert (
        captured_error.value.failure.field_path
        == "model_parameters.nig_shared_plus_shift_configuration.delta_beta"
    )
    assert captured_error.value.failure.message == "must be strictly negative"


def test_vanilla_option_response_rejects_negative_price_with_structured_error() -> None:
    response_payload = _valid_response_payload()
    response_payload["price"] = -0.01

    with pytest.raises(VanillaOptionPricingValidationError) as captured_error:
        VanillaOptionPricingResponse.from_mapping(response_payload)

    assert captured_error.value.failure.error_id == "pricing.validation.contract"
    assert captured_error.value.failure.field_path == "price"
    assert "non-negative" in captured_error.value.failure.message


def test_vanilla_option_response_rejects_missing_implied_volatility_for_successful_status() -> None:
    response_payload = _valid_response_payload()
    response_payload["implied_volatility"] = None

    with pytest.raises(VanillaOptionPricingValidationError) as captured_error:
        VanillaOptionPricingResponse.from_mapping(response_payload)

    assert captured_error.value.failure.field_path == "vanilla_option_pricing_response.implied_volatility"
    assert "must be present" in captured_error.value.failure.message


def test_vanilla_option_surface_execution_input_rejects_runtime_schedule_terminal_maturity_mismatch() -> None:
    with pytest.raises(VanillaOptionPricingValidationError) as captured_error:
        VanillaOptionSurfacePricingExecutionInput(
            request=build_anchored_surface_request_model(),
            runtime_interval_schedule=_runtime_interval_schedule((1.0,)),
            hourly_factor_contract=build_hourly_factor_contract(
                request_start_utc_nanos=REFERENCE_PRICING_TIMESTAMP_UTC_NANOS,
                s_h=(1.0,),
                known_component_nu_h=(0.0,),
                surprise_component_nu_h=(0.0,),
            ),
        )

    assert captured_error.value.failure.field_path == "request.market_inputs.maturity_intervals[1]"
    assert "terminal boundary exactly" in captured_error.value.failure.message


def test_vanilla_option_pricing_execution_input_keeps_native_hourly_factor_contract() -> None:
    execution_input = _valid_execution_input_model()

    assert execution_input.hourly_factor_contract.known_component_nu_h[0] == pytest.approx(0.03)
    assert execution_input.hourly_factor_contract.surprise_component_nu_h[0] == pytest.approx(0.05)


def test_vanilla_option_pricing_execution_input_rejects_empty_hourly_factor_contract() -> None:
    request = _valid_request_model()

    with pytest.raises(VanillaOptionPricingValidationError) as captured_error:
        VanillaOptionPricingExecutionInput(
            request=request,
            runtime_interval_schedule=_runtime_interval_schedule((0.5,)),
            hourly_factor_contract=build_hourly_factor_contract(
                request_start_utc_nanos=REFERENCE_PRICING_TIMESTAMP_UTC_NANOS,
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

    serialized_request = request.to_json()
    deserialized_request = VanillaOptionPricingRequest.from_mapping(
        parse_json_object(
            serialized_request,
            "vanilla option request",
            lambda field_path, message: ValueError(f"{field_path}: {message}"),
        )
    )

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


def test_run_vanilla_option_surface_pricing_executes_native_workflow_with_typed_surface_artifact() -> None:
    run_result = run_vanilla_option_surface_pricing(
        build_anchored_surface_execution_input_model()
    )

    assert isinstance(run_result, VanillaOptionSurfacePricingRunResult)
    assert run_result.response.shape == (1, 3)
    assert run_result.response.node_count == 3
    assert run_result.response.node_table.option_types == (
        VanillaOptionPayoffType.CALL,
        VanillaOptionPayoffType.CALL,
        VanillaOptionPayoffType.CALL,
    )
    records = run_result.response.to_records()

    assert records[0]["strike"] == pytest.approx(run_result.response.strikes[0])
    assert records[0]["maturity_interval"] == pytest.approx(run_result.response.maturity_intervals[0])
    assert records[-1]["strike"] == pytest.approx(run_result.response.strikes[-1])
    assert records[-1]["maturity_interval"] == pytest.approx(run_result.response.maturity_intervals[0])
    assert {
        record["pricing_status"] for record in records
    } <= {
        status.value for status in VanillaOptionPricingStatus
    }


def test_surface_pricing_handles_extreme_deep_in_the_money_strike_without_node_failure() -> None:
    execution_input = build_anchored_surface_execution_input_model()
    adjusted_market_inputs = replace(
        execution_input.request.market_inputs,
        strikes=(1e-6, 95.0, 105.0),
    )
    adjusted_request = replace(execution_input.request, market_inputs=adjusted_market_inputs)

    run_result = run_vanilla_option_surface_pricing(
        replace(execution_input, request=adjusted_request)
    )

    assert run_result.response.pricing_status_grid[0] == (
        VanillaOptionPricingStatus.SUCCEEDED,
        VanillaOptionPricingStatus.SUCCEEDED,
        VanillaOptionPricingStatus.SUCCEEDED,
    )
    assert run_result.response.pricing_failure_stage_grid[0] == (None, None, None)
    assert run_result.response.pricing_failure_reason_grid[0] == (None, None, None)
    assert run_result.response.price_grid[0][0] is not None
    assert run_result.response.price_grid[0][0] > run_result.response.price_grid[0][1]
    assert run_result.response.implied_volatility_inversion_status_grid[0][0] is (
        VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED
    )


def test_vanilla_option_pricing_run_result_roundtrip_preserves_required_schema_fields() -> None:
    run_result = run_vanilla_option_pricing(_valid_runtime_execution_input_model())

    serialized_run_result = run_result.to_json()
    deserialized_run_result = VanillaOptionPricingRunResult.from_mapping(
        parse_json_object(
            serialized_run_result,
            "vanilla option pricing run result",
            lambda field_path, message: ValueError(f"{field_path}: {message}"),
        )
    )

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
            "runtime_interval_schedule",
            "maturity_terminal_interval_indices",
        )
    )


def test_vanilla_option_surface_pricing_run_result_roundtrip_preserves_node_table() -> None:
    run_result = run_vanilla_option_surface_pricing(
        build_anchored_surface_execution_input_model()
    )

    serialized_run_result = run_result.to_json()
    deserialized_run_result = VanillaOptionSurfacePricingRunResult.from_mapping(
        parse_json_object(
            serialized_run_result,
            "vanilla option surface pricing run result",
            lambda field_path, message: ValueError(f"{field_path}: {message}"),
        )
    )

    assert deserialized_run_result == run_result
    assert (
        deserialized_run_result.response.node_table.model_prices[0]
        == run_result.response.price_grid[0][0]
    )


def test_pricing_run_result_roundtrip_preserves_spectral_diagnostics() -> None:
    run_result = run_vanilla_option_pricing(build_anchored_runtime_execution_input_model())

    serialized_run_result = run_result.to_json()
    deserialized_run_result = VanillaOptionPricingRunResult.from_mapping(
        parse_json_object(
            serialized_run_result,
            "vanilla option pricing run result",
            lambda field_path, message: ValueError(f"{field_path}: {message}"),
        )
    )

    assert deserialized_run_result == run_result
    assert deserialized_run_result.run_diagnostics is not None
    assert deserialized_run_result.run_diagnostics.spectral is not None
    assert (
        deserialized_run_result.run_diagnostics.spectral["collocation_node_count"]
        >= 1
    )


def _valid_request_model() -> VanillaOptionPricingRequest:
    return VanillaOptionPricingRequest(
        market_inputs=VanillaOptionPricingMarketInputs(
            spot=101.0,
            strikes=(100.0,),
            maturity_intervals=(0.5,),
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
        interval_delta=1.0,
    )


def _runtime_interval_schedule(
    interval_deltas_hours: tuple[float, ...],
    *,
    valuation_time_utc_nanos: int = REFERENCE_PRICING_TIMESTAMP_UTC_NANOS,
) -> PricingIntervalSchedule:
    interval_end_timestamps_utc_nanos: list[int] = []
    running_timestamp_utc_nanos = valuation_time_utc_nanos

    for interval_delta_hours in interval_deltas_hours:
        running_timestamp_utc_nanos += int(interval_delta_hours * 3_600_000_000_000)
        interval_end_timestamps_utc_nanos.append(running_timestamp_utc_nanos)

    return PricingIntervalSchedule(
        valuation_time_utc_nanos=valuation_time_utc_nanos,
        metadata=PricingIntervalScheduleMetadata(
            policy_id="explicit_interval_end_timestamps_utc.v1",
            derivation_policy="explicit_interval_end_timestamps_utc.v1",
            factor_assignment_policy="overlap_weighted_resampling_from_hourly_source_buckets.v1",
        ),
        interval_end_timestamps_utc_nanos=tuple(interval_end_timestamps_utc_nanos),
        interval_deltas_hours=interval_deltas_hours,
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
        runtime_interval_schedule=_runtime_interval_schedule((0.5,)),
        hourly_factor_contract=build_hourly_factor_contract(
            request_start_utc_nanos=REFERENCE_PRICING_TIMESTAMP_UTC_NANOS,
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
                strikes=(100.0,),
                maturity_intervals=(1.0,),
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
            interval_delta=1.0,
        ),
        runtime_interval_schedule=_runtime_interval_schedule((1.0,)),
        hourly_factor_contract=build_hourly_factor_contract(
            request_start_utc_nanos=REFERENCE_PRICING_TIMESTAMP_UTC_NANOS,
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
        "pricing_status": "succeeded",
        "pricing_failure_diagnostic": None,
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
