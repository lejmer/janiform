use serde_json::json;

use crate::{
    VANILLA_OPTION_CALIBRATION_HOOK_POLICY,
    VANILLA_OPTION_PRICING_CALIBRATION_HOOKS_SCHEMA_VERSION,
    VANILLA_OPTION_PRICING_RUN_DIAGNOSTICS_SCHEMA_VERSION,
    VANILLA_OPTION_PRICING_RUN_OUTPUT_SCHEMA_METADATA_VERSION,
    VANILLA_OPTION_PRICING_RUN_RESULT_SCHEMA_VERSION, VanillaOptionPayoffConfiguration,
    VanillaOptionPayoffType, VanillaOptionPricingCalibrationHooksError,
    VanillaOptionPricingDiagnostics, VanillaOptionPricingDiagnosticsError,
    VanillaOptionPricingExecutionInput, VanillaOptionPricingExecutionInputError,
    VanillaOptionPricingIntervalFactors, VanillaOptionPricingIntervalFactorsError,
    VanillaOptionPricingMarketInputs, VanillaOptionPricingMarketInputsError,
    VanillaOptionPricingOptionalOutputSelection, VanillaOptionPricingRequest,
    VanillaOptionPricingRequestError, VanillaOptionPricingResponse,
    VanillaOptionPricingResponseError, VanillaOptionPricingRunResultBuildError,
    VanillaOptionPricingRunResultBuildInput, VanillaOptionPricingStatus,
    build_vanilla_option_pricing_run_result,
};
use janiform_model_kernels::hawkes::HawkesExcitationState;

use super::support::{
    build_reference_integration_diagnostics_source, build_reference_market_inputs,
    build_reference_pricing_response, build_reference_transform_diagnostics_source,
    build_uniform_runtime_interval_schedule, build_valid_vanilla_option_request,
    disabled_boundary_input, valid_model_parameters,
};
#[test]
fn vanilla_option_request_roundtrip_serialization_preserves_contract_fields() {
    let request = build_valid_vanilla_option_request();

    let serialized_value =
        serde_json::to_value(&request).expect("request payload should serialize");
    let parsed_request: VanillaOptionPricingRequest = serde_json::from_value(serialized_value)
        .expect("serialized request payload should deserialize");

    assert_eq!(parsed_request, request);
    assert_eq!(parsed_request.interval_delta, request.interval_delta);
}

#[test]
fn vanilla_option_request_deserialization_requires_interval_delta() {
    let model_parameters_payload =
        serde_json::to_value(valid_model_parameters()).expect("model parameters should serialize");

    let payload_without_interval_delta = json!({
        "market_inputs": {
            "spot": 101.0,
            "strikes": [100.0],
            "maturity_intervals": [0.5],
            "risk_free_rate": 0.02,
            "dividend_yield": 0.01
        },
        "payoff": {
            "payoff_type": "call"
        },
        "boundary_input": {
            "interval_index": 0,
            "hawkes_boundary_state": {
                "y_plus_plus": 0.2,
                "y_plus_minus": 0.1,
                "y_minus_plus": 0.3,
                "y_minus_minus": 0.4
            },
            "mean_reversion_fitted_parameters": {
                "kappa": 0.0,
                "anchor_model": {
                    "reference_interval_hours": 1.0,
                    "pressure_persistence_per_reference_interval": 0.0,
                    "pressure_level_gain": 0.0
                }
            },
            "interval_delta": 1.0,
            "observed_boundary_state": {
                "observed_log_price": 0.0,
                "anchor_value": 0.0,
                "pressure_state": 0.0,
                "deviation": 0.0
            }
        },
        "model_parameters": model_parameters_payload
    });

    let error =
        serde_json::from_value::<VanillaOptionPricingRequest>(payload_without_interval_delta)
            .expect_err("payload without interval_delta should fail deserialization");

    assert!(
        error.to_string().contains("interval_delta"),
        "unexpected deserialization error: {error}"
    );
}

#[test]
fn vanilla_option_request_accepts_explicit_non_hourly_interval_delta() {
    let request = VanillaOptionPricingRequest::try_new(
        VanillaOptionPricingMarketInputs::try_new_single_node(101.0, 100.0, 0.5, 0.02, 0.01)
            .expect("market inputs should construct"),
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        disabled_boundary_input(
            0.5,
            HawkesExcitationState {
                y_plus_plus: 0.2,
                y_plus_minus: 0.1,
                y_minus_plus: 0.3,
                y_minus_minus: 0.4,
            },
        ),
        valid_model_parameters(),
        0.5,
    )
    .expect("non-hourly interval delta should be accepted");

    assert_eq!(request.interval_delta, 0.5);
}

#[test]
fn vanilla_option_pricing_execution_input_roundtrip_serialization_preserves_split_news_intensity() {
    let execution_input = VanillaOptionPricingExecutionInput::try_new(
        build_valid_vanilla_option_request(),
        build_uniform_runtime_interval_schedule(1, 1.0),
        vec![
            VanillaOptionPricingIntervalFactors::try_new(1.1, 0.03, 0.05, 1.0)
                .expect("interval factors should construct"),
        ],
        VanillaOptionPricingOptionalOutputSelection::new(true, false, true),
    )
    .expect("execution input should construct");

    let serialized_value =
        serde_json::to_value(&execution_input).expect("execution input payload should serialize");
    let parsed_execution_input: VanillaOptionPricingExecutionInput =
        serde_json::from_value(serialized_value)
            .expect("serialized execution input should deserialize");

    assert_eq!(parsed_execution_input, execution_input);
    assert_eq!(parsed_execution_input.interval_factors[0].nu_h_known, 0.03);
    assert_eq!(
        parsed_execution_input.interval_factors[0].nu_h_surprise,
        0.05
    );
}

#[test]
fn vanilla_option_pricing_execution_input_deserialization_requires_split_news_fields() {
    let invalid_payload = json!({
        "request": build_valid_vanilla_option_request(),
        "interval_factors": [
            {
                "s_h": 1.0,
                "nu_h_known": 0.03,
                "return_mark_scale": 1.0
            }
        ],
        "optional_output_selection": {
            "include_transform_diagnostics": true,
            "include_integration_quality_diagnostics": false,
            "include_calibration_hooks": false
        }
    });

    let error = serde_json::from_value::<VanillaOptionPricingExecutionInput>(invalid_payload)
        .expect_err("missing surprise news intensity must fail deserialization");

    let error_message = error.to_string();
    assert!(
        error_message.contains("missing field")
            || error_message.contains("nu_h_surprise")
            || error_message.contains("interval_factors")
    );
}

#[test]
fn vanilla_option_pricing_execution_input_rejects_negative_surprise_news_intensity() {
    let error = VanillaOptionPricingExecutionInput::try_new(
        build_valid_vanilla_option_request(),
        build_uniform_runtime_interval_schedule(1, 1.0),
        vec![VanillaOptionPricingIntervalFactors::new(
            1.0, 0.03, -0.01, 1.0,
        )],
        VanillaOptionPricingOptionalOutputSelection::default(),
    )
    .expect_err("negative surprise news intensity must fail execution input validation");

    assert!(matches!(
        error,
        VanillaOptionPricingExecutionInputError::InvalidIntervalFactors {
            source: VanillaOptionPricingIntervalFactorsError::InvalidNewsIntensity { .. }
        }
    ));
}

#[test]
fn vanilla_option_request_rejects_non_positive_interval_delta() {
    let error = VanillaOptionPricingRequest::try_new(
        VanillaOptionPricingMarketInputs::try_new_single_node(101.0, 100.0, 0.5, 0.02, 0.01)
            .expect("market inputs should construct"),
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        disabled_boundary_input(
            1.0,
            HawkesExcitationState {
                y_plus_plus: 0.2,
                y_plus_minus: 0.1,
                y_minus_plus: 0.3,
                y_minus_minus: 0.4,
            },
        ),
        valid_model_parameters(),
        0.0,
    )
    .expect_err("non-positive interval delta must fail request validation");

    assert!(matches!(
        error,
        VanillaOptionPricingRequestError::InvalidIntervalDelta { value } if value == 0.0
    ));
}

#[test]
fn vanilla_option_response_roundtrip_serialization_preserves_contract_fields() {
    let response = VanillaOptionPricingResponse::try_new(
        VanillaOptionPricingStatus::Succeeded,
        None,
        Some(12.4),
        Some(
            VanillaOptionPricingDiagnostics::try_new(104.8, 0.97, 0.5, 0.4)
                .expect("diagnostics should construct"),
        ),
        Some(0.2),
        Some(crate::VanillaOptionImpliedVolatilityInversionStatus::Succeeded),
    )
    .expect("response should construct");

    let serialized_value =
        serde_json::to_value(&response).expect("response payload should serialize");
    let parsed_response: VanillaOptionPricingResponse = serde_json::from_value(serialized_value)
        .expect("serialized response payload should deserialize");

    assert_eq!(parsed_response, response);
}

#[test]
fn vanilla_option_request_rejects_invalid_market_input_with_structured_error() {
    let market_inputs =
        VanillaOptionPricingMarketInputs::new_single_node(100.0, -90.0, 0.5, 0.02, 0.01);

    let error = VanillaOptionPricingRequest::try_new(
        market_inputs,
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        disabled_boundary_input(
            1.0,
            HawkesExcitationState {
                y_plus_plus: 0.2,
                y_plus_minus: 0.1,
                y_minus_plus: 0.3,
                y_minus_minus: 0.4,
            },
        ),
        valid_model_parameters(),
        1.0,
    )
    .expect_err("negative strike should fail request validation");

    assert!(matches!(
        error,
        VanillaOptionPricingRequestError::InvalidMarketInputs {
            source: VanillaOptionPricingMarketInputsError::InvalidStrike { index, value }
        } if index == 0 && value == -90.0
    ));
}

#[test]
fn vanilla_option_request_deserialize_rejects_invalid_boundary_state_with_actionable_error() {
    let mut invalid_payload = serde_json::to_value(build_valid_vanilla_option_request())
        .expect("request should serialize");

    invalid_payload["payoff"]["payoff_type"] = json!("put");
    invalid_payload["boundary_input"]["hawkes_boundary_state"]["y_plus_plus"] = json!(-0.1);

    let error = serde_json::from_value::<VanillaOptionPricingRequest>(invalid_payload)
        .expect_err("invalid boundary state should fail request deserialization");
    let error_message = error.to_string();

    assert!(error_message.contains("boundary_input.hawkes_boundary_state.y_plus_plus"));
    assert!(error_message.contains("non-negative"));
}

#[test]
fn vanilla_option_request_deserialize_rejects_incomplete_payload() {
    let mut incomplete_payload = serde_json::to_value(build_valid_vanilla_option_request())
        .expect("request should serialize");

    incomplete_payload["market_inputs"]
        .as_object_mut()
        .expect("market_inputs should be an object")
        .remove("strikes");

    let error = serde_json::from_value::<VanillaOptionPricingRequest>(incomplete_payload)
        .expect_err("incomplete request payload should fail deserialization");
    let error_message = error.to_string();
    assert!(error_message.contains("missing field `strikes`"));
}

#[test]
fn vanilla_option_response_rejects_invalid_price_with_structured_error() {
    let diagnostics = VanillaOptionPricingDiagnostics::try_new(104.8, 0.97, 0.5, 0.4)
        .expect("diagnostics should construct");

    let error = VanillaOptionPricingResponse::try_new(
        VanillaOptionPricingStatus::Succeeded,
        None,
        Some(-0.01),
        Some(diagnostics),
        Some(0.2),
        Some(crate::VanillaOptionImpliedVolatilityInversionStatus::Succeeded),
    )
    .expect_err("negative option prices must fail response validation");

    assert!(matches!(
        error,
        VanillaOptionPricingResponseError::InvalidPrice { value } if value == -0.01
    ));
}

#[test]
fn vanilla_option_response_rejects_invalid_diagnostics_with_structured_error() {
    let invalid_diagnostics = VanillaOptionPricingDiagnostics::new(0.0, 0.97, 0.5, 0.4);

    let error = VanillaOptionPricingResponse::try_new(
        VanillaOptionPricingStatus::Succeeded,
        None,
        Some(1.0),
        Some(invalid_diagnostics),
        Some(0.2),
        Some(crate::VanillaOptionImpliedVolatilityInversionStatus::Succeeded),
    )
    .expect_err("invalid diagnostics must fail response validation");

    assert!(matches!(
        error,
        VanillaOptionPricingResponseError::InvalidDiagnostics {
            source: VanillaOptionPricingDiagnosticsError::InvalidForwardPrice { value }
        } if value == 0.0
    ));
}

#[test]
fn vanilla_option_response_rejects_missing_implied_volatility_for_successful_inversion() {
    let diagnostics = VanillaOptionPricingDiagnostics::try_new(104.8, 0.97, 0.5, 0.4)
        .expect("diagnostics should construct");

    let error = VanillaOptionPricingResponse::try_new(
        VanillaOptionPricingStatus::Succeeded,
        None,
        Some(1.0),
        Some(diagnostics),
        None,
        Some(crate::VanillaOptionImpliedVolatilityInversionStatus::Succeeded),
    )
    .expect_err("successful inversion status must include an implied volatility");

    assert!(matches!(
        error,
        VanillaOptionPricingResponseError::MissingImpliedVolatilityForSuccessfulInversion
    ));
}

#[test]
fn vanilla_option_response_rejects_implied_volatility_for_failed_inversion() {
    let diagnostics = VanillaOptionPricingDiagnostics::try_new(104.8, 0.97, 0.5, 0.4)
        .expect("diagnostics should construct");

    let error = VanillaOptionPricingResponse::try_new(
        VanillaOptionPricingStatus::Succeeded,
        None,
        Some(1.0),
        Some(diagnostics),
        Some(0.2),
        Some(crate::VanillaOptionImpliedVolatilityInversionStatus::PriceAboveFiniteUpperBound),
    )
    .expect_err("failed inversion status must not carry an implied volatility");

    assert!(matches!(
        error,
        VanillaOptionPricingResponseError::UnexpectedImpliedVolatilityForFailedInversion {
            status:
                crate::VanillaOptionImpliedVolatilityInversionStatus::PriceAboveFiniteUpperBound
        }
    ));
}

#[test]
fn pricing_run_output_selection_defaults_to_base_response_only() {
    let market_inputs = build_reference_market_inputs();
    let response = build_reference_pricing_response(&market_inputs);
    let transform_diagnostics = build_reference_transform_diagnostics_source();
    let integration_quality = build_reference_integration_diagnostics_source(&market_inputs);

    let run_result =
        build_vanilla_option_pricing_run_result(VanillaOptionPricingRunResultBuildInput {
            response,
            output_selection: VanillaOptionPricingOptionalOutputSelection::default(),
            market_inputs_for_calibration_hooks: None,
            transform_diagnostics_source: Some(transform_diagnostics),
            integration_quality_source: Some(integration_quality),
            spectral_source: None,
            runtime_interval_schedule: build_uniform_runtime_interval_schedule(1, 1.0),
            maturity_terminal_interval_indices: vec![0],
        })
        .expect("default output selection should emit base response only");

    assert_eq!(
        run_result.schema_version,
        VANILLA_OPTION_PRICING_RUN_RESULT_SCHEMA_VERSION
    );
    assert!(run_result.run_diagnostics.is_none());
    assert!(run_result.calibration_hooks.is_none());
}

#[test]
fn pricing_run_output_selection_can_emit_diagnostics_and_calibration_hooks() {
    let market_inputs = build_reference_market_inputs();
    let response = build_reference_pricing_response(&market_inputs);
    let transform_diagnostics = build_reference_transform_diagnostics_source();
    let integration_quality = build_reference_integration_diagnostics_source(&market_inputs);

    let run_result =
        build_vanilla_option_pricing_run_result(VanillaOptionPricingRunResultBuildInput {
            response,
            output_selection: VanillaOptionPricingOptionalOutputSelection::new(true, true, true),
            market_inputs_for_calibration_hooks: Some(market_inputs),
            transform_diagnostics_source: Some(transform_diagnostics),
            integration_quality_source: Some(integration_quality),
            spectral_source: None,
            runtime_interval_schedule: build_uniform_runtime_interval_schedule(1, 1.0),
            maturity_terminal_interval_indices: vec![0],
        })
        .expect("full output selection should include diagnostics and hooks");

    let run_diagnostics = run_result
        .run_diagnostics
        .expect("run diagnostics should be present");
    assert_eq!(
        run_diagnostics.schema_version,
        VANILLA_OPTION_PRICING_RUN_DIAGNOSTICS_SCHEMA_VERSION
    );
    assert!(run_diagnostics.transform.is_some());
    assert!(run_diagnostics.integration_quality.is_some());

    let calibration_hooks = run_result
        .calibration_hooks
        .expect("calibration hooks should be present");
    assert_eq!(
        calibration_hooks.schema_version,
        VANILLA_OPTION_PRICING_CALIBRATION_HOOKS_SCHEMA_VERSION
    );
    assert_eq!(
        calibration_hooks.hook_policy,
        VANILLA_OPTION_CALIBRATION_HOOK_POLICY
    );
    assert!(calibration_hooks.forward_moneyness > 0.0);
    assert!(calibration_hooks.log_moneyness.is_finite());
}

#[test]
fn pricing_run_output_schema_metadata_is_machine_readable_with_required_fields() {
    let market_inputs = build_reference_market_inputs();
    let response = build_reference_pricing_response(&market_inputs);
    let transform_diagnostics = build_reference_transform_diagnostics_source();
    let integration_quality = build_reference_integration_diagnostics_source(&market_inputs);
    let run_result =
        build_vanilla_option_pricing_run_result(VanillaOptionPricingRunResultBuildInput {
            response,
            output_selection: VanillaOptionPricingOptionalOutputSelection::new(true, true, true),
            market_inputs_for_calibration_hooks: Some(market_inputs),
            transform_diagnostics_source: Some(transform_diagnostics),
            integration_quality_source: Some(integration_quality),
            spectral_source: None,
            runtime_interval_schedule: build_uniform_runtime_interval_schedule(1, 1.0),
            maturity_terminal_interval_indices: vec![0],
        })
        .expect("full output selection should construct");

    assert_eq!(
        run_result.output_schema_metadata.schema_version,
        VANILLA_OPTION_PRICING_RUN_OUTPUT_SCHEMA_METADATA_VERSION
    );
    assert!(
        run_result
            .output_schema_metadata
            .required_run_result_fields
            .contains(&"response")
    );
    assert!(
        run_result
            .output_schema_metadata
            .required_run_diagnostics_fields
            .contains(&"integration_quality")
    );
    assert!(
        run_result
            .output_schema_metadata
            .required_calibration_hooks_fields
            .contains(&"forward_moneyness")
    );

    let serialized_run_result =
        serde_json::to_value(&run_result).expect("run result should serialize");
    assert_eq!(
        serialized_run_result["schema_version"],
        json!(VANILLA_OPTION_PRICING_RUN_RESULT_SCHEMA_VERSION)
    );
    assert_eq!(
        serialized_run_result["output_schema_metadata"]["schema_version"],
        json!(VANILLA_OPTION_PRICING_RUN_OUTPUT_SCHEMA_METADATA_VERSION)
    );
    assert_eq!(
        serialized_run_result["output_schema_metadata"]["required_run_result_fields"],
        json!(run_result.output_schema_metadata.required_run_result_fields)
    );
    assert_eq!(
        serialized_run_result["output_schema_metadata"]["required_run_diagnostics_fields"],
        json!(
            run_result
                .output_schema_metadata
                .required_run_diagnostics_fields
        )
    );
    assert_eq!(
        serialized_run_result["output_schema_metadata"]["required_calibration_hooks_fields"],
        json!(
            run_result
                .output_schema_metadata
                .required_calibration_hooks_fields
        )
    );
}

#[test]
fn pricing_run_output_builder_reports_missing_selected_optional_sources() {
    let market_inputs = build_reference_market_inputs();
    let response = build_reference_pricing_response(&market_inputs);

    let missing_transform_error =
        build_vanilla_option_pricing_run_result(VanillaOptionPricingRunResultBuildInput {
            response: response.clone(),
            output_selection: VanillaOptionPricingOptionalOutputSelection::new(true, false, false),
            market_inputs_for_calibration_hooks: None,
            transform_diagnostics_source: None,
            integration_quality_source: None,
            spectral_source: None,
            runtime_interval_schedule: build_uniform_runtime_interval_schedule(1, 1.0),
            maturity_terminal_interval_indices: vec![0],
        })
        .expect_err("transform diagnostics selection without source must fail");
    assert!(matches!(
        missing_transform_error,
        VanillaOptionPricingRunResultBuildError::MissingTransformDiagnosticsSource
    ));

    let missing_integration_error =
        build_vanilla_option_pricing_run_result(VanillaOptionPricingRunResultBuildInput {
            response: response.clone(),
            output_selection: VanillaOptionPricingOptionalOutputSelection::new(false, true, false),
            market_inputs_for_calibration_hooks: None,
            transform_diagnostics_source: None,
            integration_quality_source: None,
            spectral_source: None,
            runtime_interval_schedule: build_uniform_runtime_interval_schedule(1, 1.0),
            maturity_terminal_interval_indices: vec![0],
        })
        .expect_err("integration diagnostics selection without source must fail");
    assert!(matches!(
        missing_integration_error,
        VanillaOptionPricingRunResultBuildError::MissingIntegrationQualityDiagnosticsSource
    ));

    let missing_market_inputs_error =
        build_vanilla_option_pricing_run_result(VanillaOptionPricingRunResultBuildInput {
            response,
            output_selection: VanillaOptionPricingOptionalOutputSelection::new(false, false, true),
            market_inputs_for_calibration_hooks: None,
            transform_diagnostics_source: None,
            integration_quality_source: None,
            spectral_source: None,
            runtime_interval_schedule: build_uniform_runtime_interval_schedule(1, 1.0),
            maturity_terminal_interval_indices: vec![0],
        })
        .expect_err("calibration hooks selection without market inputs must fail");
    assert!(matches!(
        missing_market_inputs_error,
        VanillaOptionPricingRunResultBuildError::MissingMarketInputsForCalibrationHooks
    ));
}

#[test]
fn calibration_hooks_reject_invalid_market_inputs_with_actionable_error() {
    let response = build_reference_pricing_response(&build_reference_market_inputs());
    let invalid_market_inputs =
        VanillaOptionPricingMarketInputs::new_single_node(0.0, 100.0, 0.5, 0.02, 0.01);

    let error = build_vanilla_option_pricing_run_result(VanillaOptionPricingRunResultBuildInput {
        response,
        output_selection: VanillaOptionPricingOptionalOutputSelection::new(false, false, true),
        market_inputs_for_calibration_hooks: Some(invalid_market_inputs),
        transform_diagnostics_source: None,
        integration_quality_source: None,
        spectral_source: None,
        runtime_interval_schedule: build_uniform_runtime_interval_schedule(1, 1.0),
        maturity_terminal_interval_indices: vec![0],
    })
    .expect_err("invalid market inputs should fail calibration hook construction");

    assert!(matches!(
        error,
        VanillaOptionPricingRunResultBuildError::InvalidCalibrationHooks {
            source: VanillaOptionPricingCalibrationHooksError::InvalidMarketInputs {
                source: VanillaOptionPricingMarketInputsError::InvalidSpot { value }
            }
        } if value == 0.0
    ));
}
