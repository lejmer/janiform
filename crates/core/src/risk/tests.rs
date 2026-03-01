use crate::affine_oracle::{
    ExponentialAffineTransformEquationSpecification, ExponentialAffineTransformNewsIntensityInput,
};
use crate::calendar::MarketState;
use crate::complex_math::ComplexScalar;
use crate::hawkes::{
    HawkesEventSide, HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
    HawkesTransitionError, HawkesTransitionStep, evolve_hawkes_state,
};
use crate::news::GammaSeverityMarkConfiguration;
use crate::nig::NigSharedPlusShiftConfiguration;
use crate::randomness::SplitMix64SimulationRandomnessStream;
use crate::return_decomposition::{ReturnDecomposition, ReturnDecompositionInput};

use super::{
    SUFFICIENT_STATISTICS_BATCH_SEED_PARTITION_POLICY,
    SUFFICIENT_STATISTICS_BOUNDARY_STATE_UPDATE_POLICY,
    SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_ROUTING_POLICY,
    SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY,
    SUFFICIENT_STATISTICS_RISK_PATH_DIAGNOSTICS_SCHEMA_VERSION,
    SUFFICIENT_STATISTICS_RISK_PATH_OUTPUT_SCHEMA_METADATA_VERSION,
    SUFFICIENT_STATISTICS_RISK_PATH_PROVENANCE_SCHEMA_VERSION,
    SUFFICIENT_STATISTICS_RISK_PATH_RECORD_SCHEMA_VERSION,
    SUFFICIENT_STATISTICS_RISK_PATH_SCHEMA_VERSION,
    SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION,
    SUFFICIENT_STATISTICS_STREAMING_CHUNKING_POLICY, SufficientStatisticsBoundaryStateUpdateError,
    SufficientStatisticsBoundaryStateUpdateSnapshot, SufficientStatisticsGeneratedIntervalReturn,
    SufficientStatisticsIntervalStatistics, SufficientStatisticsRiskIntervalFactors,
    SufficientStatisticsRiskIntervalFactorsError, SufficientStatisticsRiskModelInputs,
    SufficientStatisticsRiskPath, SufficientStatisticsRiskPathError,
    SufficientStatisticsRiskPathRecord, SufficientStatisticsRiskReturnComponents,
    SufficientStatisticsRiskReturnGenerationOptions, SufficientStatisticsRiskRunRequest,
    SufficientStatisticsRiskRunRequestError, SufficientStatisticsStreamingChunkingSettings,
    SufficientStatisticsTransformSliceContext, SufficientStatisticsTransformSliceExecutionError,
    SufficientStatisticsTransformSliceSamplingError, SufficientStatisticsTransformSliceSettings,
    SufficientStatisticsTransformSliceSettingsError, SufficientStatisticsTransformSlices,
    derive_sufficient_statistics_batch_path_seed, generate_interval_return_from_sampled_statistics,
    sample_interval_statistics_batch_from_transform_slices,
    sample_interval_statistics_from_transform_slices,
    sample_interval_statistics_multi_path_batch_from_transform_slices,
    solve_sufficient_statistics_transform_slices, stream_interval_statistics_from_transform_slices,
    stream_interval_statistics_multi_path_batch_from_transform_slices,
    sufficient_statistics_batch_seed_partition_policy,
    update_boundary_state_from_interval_statistics,
};

#[test]
fn run_request_round_trip_serialization_preserves_contract_shape() {
    let run_request = build_valid_run_request();

    let serialized =
        serde_json::to_value(&run_request).expect("run request payload should serialize");
    assert_eq!(serialized["seed"], serde_json::json!(17));
    assert_eq!(serialized["horizon_interval_count"], serde_json::json!(2));
    assert_eq!(serialized["interval_delta"], serde_json::json!(1.0));
    assert_eq!(
        SufficientStatisticsRiskRunRequest::schema_version(),
        SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION
    );
    assert_eq!(
        SufficientStatisticsRiskIntervalFactors::policy(),
        SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY
    );

    let parsed: SufficientStatisticsRiskRunRequest =
        serde_json::from_value(serialized).expect("serialized payload should deserialize");
    assert_eq!(parsed, run_request);
}

#[test]
fn interval_factors_component_news_intensity_round_trip_and_context_consistency() {
    let factors = SufficientStatisticsRiskIntervalFactors::try_new(
        vec![1.1, 0.9],
        vec![0.12, 0.03],
        vec![0.08, 0.07],
        Some(1.0),
    )
    .expect("component-aware interval factors should construct");
    assert_eq!(factors.nu_h.as_ref(), &[0.2, 0.1]);

    let first_interval_news_intensity = factors.news_intensity_input_at(0);
    assert_eq!(
        first_interval_news_intensity,
        ExponentialAffineTransformNewsIntensityInput::try_new(0.12, 0.08)
            .expect("expected news-intensity input should construct")
    );

    let serialized = serde_json::to_value(&factors).expect("interval factors should serialize");
    assert_eq!(serialized["nu_h_known"], serde_json::json!([0.12, 0.03]));
    assert_eq!(serialized["nu_h_surprise"], serde_json::json!([0.08, 0.07]));

    let parsed: SufficientStatisticsRiskIntervalFactors =
        serde_json::from_value(serialized).expect("serialized interval factors should parse");
    assert_eq!(parsed, factors);
}

#[test]
fn interval_factors_reject_inconsistent_total_and_component_news_intensity() {
    let factors = SufficientStatisticsRiskIntervalFactors {
        interval_delta: Some(1.0),
        s_h: vec![1.1].into_boxed_slice(),
        nu_h: vec![0.3].into_boxed_slice(),
        nu_h_known: vec![0.1].into_boxed_slice(),
        nu_h_surprise: vec![0.1].into_boxed_slice(),
    };
    let error = factors
        .validate()
        .expect_err("inconsistent total and component intensities must fail validation");
    assert!(matches!(
        error,
        SufficientStatisticsRiskIntervalFactorsError::InconsistentNewsIntensityAggregation {
            interval_index: 0,
            ..
        }
    ));
}

#[test]
fn path_round_trip_serialization_preserves_interval_statistics_and_return_components() {
    let record = SufficientStatisticsRiskPathRecord::try_new(
        0,
        SufficientStatisticsIntervalStatistics::try_new(0.7, 0.5, 3, 2)
            .expect("statistics should be valid"),
        SufficientStatisticsRiskReturnComponents::try_new(0.04, -0.03, 0.01, 0.0, -0.02, -0.01)
            .expect("return components should be valid"),
    )
    .expect("record should be valid");
    let path = SufficientStatisticsRiskPath::try_new(
        17,
        1.0,
        1,
        build_initial_state(),
        build_initial_state(),
        vec![record],
    )
    .expect("path should be valid");

    let serialized = serde_json::to_value(&path).expect("path payload should serialize");
    assert_eq!(
        serialized["schema_version"],
        serde_json::json!(SUFFICIENT_STATISTICS_RISK_PATH_SCHEMA_VERSION)
    );
    assert_eq!(
        serialized["records"][0]["schema_version"],
        serde_json::json!(SUFFICIENT_STATISTICS_RISK_PATH_RECORD_SCHEMA_VERSION)
    );
    assert_eq!(
        serialized["records"][0]["sufficient_statistics"]["n_plus"],
        serde_json::json!(3)
    );
    assert_eq!(
        serialized["records"][0]["return_components"]["total_return"],
        serde_json::json!(-0.01)
    );
    assert_eq!(
        serialized["output_schema_metadata"]["schema_version"],
        serde_json::json!(SUFFICIENT_STATISTICS_RISK_PATH_OUTPUT_SCHEMA_METADATA_VERSION)
    );
    assert_eq!(
        serialized["provenance"]["schema_version"],
        serde_json::json!(SUFFICIENT_STATISTICS_RISK_PATH_PROVENANCE_SCHEMA_VERSION)
    );
    assert_eq!(
        serialized["diagnostics"]["schema_version"],
        serde_json::json!(SUFFICIENT_STATISTICS_RISK_PATH_DIAGNOSTICS_SCHEMA_VERSION)
    );
    assert_eq!(
        serialized["diagnostics"]["cumulative_n_plus"],
        serde_json::json!(3)
    );

    let parsed: SufficientStatisticsRiskPath =
        serde_json::from_value(serialized).expect("serialized path should deserialize");
    assert_eq!(parsed, path);
}

#[test]
fn risk_path_diagnostics_schema_metadata_exposes_required_fields() {
    let record = SufficientStatisticsRiskPathRecord::try_new(
        0,
        SufficientStatisticsIntervalStatistics::try_new(0.9, 0.4, 2, 1)
            .expect("statistics should be valid"),
        SufficientStatisticsRiskReturnComponents::try_new(0.03, -0.02, 0.01, 0.0, -0.01, 0.0)
            .expect("return components should be valid"),
    )
    .expect("record should be valid");
    let path = SufficientStatisticsRiskPath::try_new(
        19,
        1.0,
        1,
        build_initial_state(),
        build_initial_state(),
        vec![record],
    )
    .expect("path should be valid");

    let serialized = serde_json::to_value(path).expect("path payload should serialize");
    assert_eq!(
        serialized["output_schema_metadata"]["required_path_fields"],
        serde_json::json!([
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
            "records"
        ])
    );
    assert_eq!(
        serialized["output_schema_metadata"]["required_diagnostics_fields"],
        serde_json::json!([
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
            "max_abs_total_return"
        ])
    );
}

#[test]
fn risk_path_deserialization_rejects_incorrect_diagnostics_with_field_context() {
    let record = SufficientStatisticsRiskPathRecord::try_new(
        0,
        SufficientStatisticsIntervalStatistics::try_new(0.7, 0.5, 3, 2)
            .expect("statistics should be valid"),
        SufficientStatisticsRiskReturnComponents::try_new(0.04, -0.03, 0.01, 0.0, -0.02, -0.01)
            .expect("return components should be valid"),
    )
    .expect("record should be valid");
    let path = SufficientStatisticsRiskPath::try_new(
        17,
        1.0,
        1,
        build_initial_state(),
        build_initial_state(),
        vec![record],
    )
    .expect("path should be valid");

    let mut serialized = serde_json::to_value(path).expect("path payload should serialize");
    serialized["diagnostics"]["cumulative_n_plus"] = serde_json::json!(5);

    let error = serde_json::from_value::<SufficientStatisticsRiskPath>(serialized)
        .expect_err("mismatched diagnostics should fail deserialization");
    assert!(
        error
            .to_string()
            .contains("diagnostics.cumulative_n_plus must be 3, found 5")
    );
}

#[test]
fn risk_path_metadata_values_are_deterministic_across_repeated_runs() {
    let record = SufficientStatisticsRiskPathRecord::try_new(
        0,
        SufficientStatisticsIntervalStatistics::try_new(0.7, 0.5, 3, 2)
            .expect("statistics should be valid"),
        SufficientStatisticsRiskReturnComponents::try_new(0.04, -0.03, 0.01, 0.0, -0.02, -0.01)
            .expect("return components should be valid"),
    )
    .expect("record should be valid");
    let first_path = SufficientStatisticsRiskPath::try_new(
        17,
        1.0,
        1,
        build_initial_state(),
        build_initial_state(),
        vec![record.clone()],
    )
    .expect("first path should be valid");
    let second_path = SufficientStatisticsRiskPath::try_new(
        17,
        1.0,
        1,
        build_initial_state(),
        build_initial_state(),
        vec![record],
    )
    .expect("second path should be valid");

    assert_eq!(
        first_path.output_schema_metadata,
        second_path.output_schema_metadata
    );
    assert_eq!(first_path.provenance, second_path.provenance);
    assert_eq!(first_path.diagnostics, second_path.diagnostics);
}

#[test]
fn run_request_validation_rejects_invalid_values_with_structured_errors() {
    let invalid_horizon = SufficientStatisticsRiskRunRequest::try_new(
        7,
        0,
        1.0,
        build_model_inputs(),
        build_interval_factors(),
    )
    .expect_err("zero horizon must fail validation");
    assert_eq!(
        invalid_horizon,
        SufficientStatisticsRiskRunRequestError::InvalidHorizonIntervalCount { value: 0 }
    );

    let invalid_interval_delta = SufficientStatisticsRiskRunRequest::try_new(
        7,
        1,
        0.0,
        build_model_inputs(),
        build_interval_factors(),
    )
    .expect_err("non-positive interval delta must fail validation");
    assert_eq!(
        invalid_interval_delta,
        SufficientStatisticsRiskRunRequestError::InvalidIntervalDelta { value: 0.0 }
    );

    let unsupported_non_hourly = SufficientStatisticsRiskRunRequest::try_new(
        7,
        1,
        0.5,
        build_model_inputs(),
        build_interval_factors(),
    )
    .expect_err("non-hourly interval delta without compatible factor metadata must fail");
    assert_eq!(
        unsupported_non_hourly,
        SufficientStatisticsRiskRunRequestError::IntervalDeltaFactorMismatch {
            requested_interval_delta: 0.5,
            available_factor_interval_delta: 1.0,
        }
    );

    let supported_sub_hour = SufficientStatisticsRiskRunRequest::try_new(
        7,
        1,
        0.5,
        build_model_inputs(),
        build_interval_factors_with_interval_delta(0.5),
    )
    .expect("non-hourly interval delta with compatible factor metadata should pass");
    assert_eq!(supported_sub_hour.interval_delta, 0.5);
}

#[test]
fn deserialization_rejects_incomplete_or_malformed_request_payloads() {
    let missing_model_inputs_payload = serde_json::json!({
        "seed": 17,
        "horizon_interval_count": 2,
        "interval_delta": 1.0,
        "interval_factors": {
            "s_h": [1.1, 0.9],
            "nu_h_known": [0.12, 0.03],
            "nu_h_surprise": [0.08, 0.07]
        }
    });
    let missing_model_inputs_error =
        serde_json::from_value::<SufficientStatisticsRiskRunRequest>(missing_model_inputs_payload)
            .expect_err("missing model_inputs must fail deserialization");

    assert!(
        missing_model_inputs_error
            .to_string()
            .contains("missing field `model_inputs`")
    );

    let malformed_factors_payload = serde_json::json!({
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
                        "minus_minus": 0.25
                    },
                    "rho": {
                        "plus_plus": 0.08,
                        "plus_minus": 0.04,
                        "minus_plus": 0.05,
                        "minus_minus": 0.1
                    }
                },
                "gamma_severity_mark_configuration": {
                    "shape_k": 1.8,
                    "scale_theta": 0.7
                }
            },
            "nig_shared_plus_shift_configuration": {
                "alpha": 2.3,
                "beta_0": -0.8,
                "delta": 0.7,
                "mu": 0.1,
                "delta_beta": -0.2
            },
            "initial_excitation_state": {
                "y_plus_plus": 0.2,
                "y_plus_minus": 0.1,
                "y_minus_plus": 0.3,
                "y_minus_minus": 0.4
            }
        },
        "interval_factors": {
            "s_h": [1.1, 0.9],
            "nu_h_known": [0.2],
            "nu_h_surprise": [0.0, 0.1]
        }
    });
    let malformed_factors_error =
        serde_json::from_value::<SufficientStatisticsRiskRunRequest>(malformed_factors_payload)
            .expect_err("mismatched interval_factors lengths must fail deserialization");

    assert!(
        malformed_factors_error
            .to_string()
            .contains("interval_factors.nu_h_known length must match s_h length")
    );
}

#[test]
fn run_request_deserialization_defaults_interval_delta_to_hourly() {
    let payload_without_interval_delta = serde_json::json!({
        "seed": 17,
        "horizon_interval_count": 2,
        "model_inputs": {
            "equation_specification": {
                "hawkes_kernel_parameters": {
                    "lambda_bar": 0.8,
                    "beta": 1.6,
                    "alpha": {
                        "plus_plus": 0.2,
                        "plus_minus": 0.1,
                        "minus_plus": 0.1,
                        "minus_minus": 0.25
                    },
                    "rho": {
                        "plus_plus": 0.08,
                        "plus_minus": 0.04,
                        "minus_plus": 0.05,
                        "minus_minus": 0.1
                    }
                },
                "gamma_severity_mark_configuration": {
                    "shape_k": 1.8,
                    "scale_theta": 0.7
                }
            },
            "nig_shared_plus_shift_configuration": {
                "alpha": 2.3,
                "beta_0": -0.8,
                "delta": 0.7,
                "mu": 0.1,
                "delta_beta": -0.2
            },
            "initial_excitation_state": {
                "y_plus_plus": 0.2,
                "y_plus_minus": 0.1,
                "y_minus_plus": 0.3,
                "y_minus_minus": 0.4
            }
        },
        "interval_factors": {
            "s_h": [1.1, 0.9],
            "nu_h_known": [0.12, 0.03],
            "nu_h_surprise": [0.08, 0.07]
        }
    });

    let parsed_request: SufficientStatisticsRiskRunRequest =
        serde_json::from_value(payload_without_interval_delta)
            .expect("payload without interval_delta should deserialize");
    assert_eq!(
        parsed_request.interval_delta,
        SufficientStatisticsRiskRunRequest::default_interval_delta()
    );
}

#[test]
fn run_request_deserialization_accepts_component_only_news_intensity_factors() {
    let payload = serde_json::json!({
        "seed": 17,
        "horizon_interval_count": 2,
        "model_inputs": {
            "equation_specification": {
                "hawkes_kernel_parameters": {
                    "lambda_bar": 0.8,
                    "beta": 1.6,
                    "alpha": {
                        "plus_plus": 0.2,
                        "plus_minus": 0.1,
                        "minus_plus": 0.1,
                        "minus_minus": 0.25
                    },
                    "rho": {
                        "plus_plus": 0.08,
                        "plus_minus": 0.04,
                        "minus_plus": 0.05,
                        "minus_minus": 0.1
                    }
                },
                "gamma_severity_mark_configuration": {
                    "shape_k": 1.8,
                    "scale_theta": 0.7
                }
            },
            "nig_shared_plus_shift_configuration": {
                "alpha": 2.3,
                "beta_0": -0.8,
                "delta": 0.7,
                "mu": 0.1,
                "delta_beta": -0.2
            },
            "initial_excitation_state": {
                "y_plus_plus": 0.2,
                "y_plus_minus": 0.1,
                "y_minus_plus": 0.3,
                "y_minus_minus": 0.4
            }
        },
        "interval_factors": {
            "s_h": [1.1, 0.9],
            "nu_h_known": [0.12, 0.03],
            "nu_h_surprise": [0.08, 0.07]
        }
    });

    let parsed_request: SufficientStatisticsRiskRunRequest = serde_json::from_value(payload)
        .expect("component-only news-intensity payload should parse");
    assert_eq!(parsed_request.interval_factors.nu_h.as_ref(), &[0.2, 0.1]);
    assert_eq!(
        parsed_request.interval_factors.news_intensity_input_at(1),
        ExponentialAffineTransformNewsIntensityInput::try_new(0.03, 0.07)
            .expect("expected news-intensity input should construct")
    );
}

#[test]
fn path_validation_rejects_non_sequential_interval_indices() {
    let first_record = SufficientStatisticsRiskPathRecord::try_new(
        1,
        SufficientStatisticsIntervalStatistics::try_new(0.7, 0.5, 3, 2)
            .expect("statistics should be valid"),
        SufficientStatisticsRiskReturnComponents::try_new(0.04, -0.03, 0.01, 0.0, -0.02, -0.01)
            .expect("return components should be valid"),
    )
    .expect("record should be valid");

    let error = SufficientStatisticsRiskPath::try_new(
        17,
        1.0,
        1,
        build_initial_state(),
        build_initial_state(),
        vec![first_record],
    )
    .expect_err("non-sequential interval index must fail validation");
    assert_eq!(
        error,
        SufficientStatisticsRiskPathError::NonSequentialIntervalIndex {
            record_position: 0,
            record_interval_index: 1,
        }
    );
}

#[test]
fn interval_return_generator_open_window_routes_open_jump_and_suppresses_closed_gap() {
    let sampled_statistics = SufficientStatisticsIntervalStatistics::try_new(0.8, 0.6, 2, 1)
        .expect("sampled statistics should be valid");
    let driver_parameters = valid_nig_driver_parameters();
    let mut observed_stream = SplitMix64SimulationRandomnessStream::from_seed(44_001);
    let mut expected_stream = SplitMix64SimulationRandomnessStream::from_seed(44_001);
    let open_jump_return = -0.27;
    let closed_gap_return = 0.19;

    let generated_return = generate_interval_return_from_sampled_statistics(
        driver_parameters,
        sampled_statistics,
        MarketState::Open,
        open_jump_return,
        closed_gap_return,
        &mut observed_stream,
        Some(SufficientStatisticsRiskReturnGenerationOptions {
            include_component_breakdown: true,
        }),
    )
    .expect("interval return generation should succeed for open-window path");
    let expected_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: sampled_statistics.a_plus,
        a_minus: sampled_statistics.a_minus,
        open_jump: open_jump_return,
        closed_gap: 0.0,
    })
    .expect("expected decomposition should construct");
    let expected_trading_contributions = expected_decomposition
        .sample_nig_trading_time_increment(driver_parameters, &mut expected_stream)
        .expect("reference trading-time sampling should succeed");

    let component_breakdown = generated_return
        .component_breakdown
        .expect("component breakdown should be present");
    assert_eq!(
        SufficientStatisticsGeneratedIntervalReturn::component_routing_policy(),
        SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_ROUTING_POLICY
    );
    assert_float_close(
        component_breakdown.buy_side_return,
        expected_trading_contributions.buy_side_increment,
    );
    assert_float_close(
        component_breakdown.sell_side_return,
        -expected_trading_contributions.sell_side_increment,
    );
    assert_float_close(
        component_breakdown.trading_time_return,
        expected_trading_contributions.combined_increment,
    );
    assert_float_close(component_breakdown.open_jump_return, open_jump_return);
    assert_float_close(component_breakdown.closed_gap_return, 0.0);
    assert_float_close(
        component_breakdown.total_return,
        expected_trading_contributions.combined_increment + open_jump_return,
    );
    assert_float_close(
        generated_return.total_return,
        expected_trading_contributions.combined_increment + open_jump_return,
    );
}

#[test]
fn interval_return_generator_closed_window_routes_closed_gap_and_supports_optional_breakdown() {
    let sampled_statistics = SufficientStatisticsIntervalStatistics::try_new(0.9, 0.7, 3, 2)
        .expect("sampled statistics should be valid");
    let driver_parameters = valid_nig_driver_parameters();
    let mut stream_with_breakdown = SplitMix64SimulationRandomnessStream::from_seed(44_002);
    let mut stream_without_breakdown = SplitMix64SimulationRandomnessStream::from_seed(44_002);
    let open_jump_return = -0.11;
    let closed_gap_return = 0.19;

    let with_breakdown = generate_interval_return_from_sampled_statistics(
        driver_parameters,
        sampled_statistics,
        MarketState::Closed,
        open_jump_return,
        closed_gap_return,
        &mut stream_with_breakdown,
        Some(SufficientStatisticsRiskReturnGenerationOptions {
            include_component_breakdown: true,
        }),
    )
    .expect("closed-window return generation should succeed with breakdown");
    let without_breakdown = generate_interval_return_from_sampled_statistics(
        driver_parameters,
        sampled_statistics,
        MarketState::Closed,
        open_jump_return,
        closed_gap_return,
        &mut stream_without_breakdown,
        Some(SufficientStatisticsRiskReturnGenerationOptions {
            include_component_breakdown: false,
        }),
    )
    .expect("closed-window return generation should succeed without breakdown");

    let component_breakdown = with_breakdown
        .component_breakdown
        .expect("component breakdown should be present");
    assert_float_close(
        component_breakdown.total_return,
        component_breakdown.trading_time_return + closed_gap_return,
    );
    assert_float_close(component_breakdown.open_jump_return, 0.0);
    assert_float_close(component_breakdown.closed_gap_return, closed_gap_return);
    assert!(without_breakdown.component_breakdown.is_none());
    assert_float_close(without_breakdown.total_return, with_breakdown.total_return);
}

#[test]
fn interval_return_generator_is_seed_deterministic_across_repeated_runs() {
    let sampled_statistics = SufficientStatisticsIntervalStatistics::try_new(1.0, 0.4, 1, 2)
        .expect("sampled statistics should be valid");
    let driver_parameters = valid_nig_driver_parameters();
    let options = Some(SufficientStatisticsRiskReturnGenerationOptions {
        include_component_breakdown: true,
    });
    let mut first_stream = SplitMix64SimulationRandomnessStream::from_seed(44_003);
    let mut second_stream = SplitMix64SimulationRandomnessStream::from_seed(44_003);

    let first_generated_return = generate_interval_return_from_sampled_statistics(
        driver_parameters,
        sampled_statistics,
        MarketState::Closed,
        0.0,
        -0.05,
        &mut first_stream,
        options,
    )
    .expect("first seeded interval return generation should succeed");
    let second_generated_return = generate_interval_return_from_sampled_statistics(
        driver_parameters,
        sampled_statistics,
        MarketState::Closed,
        0.0,
        -0.05,
        &mut second_stream,
        options,
    )
    .expect("second seeded interval return generation should succeed");

    assert_eq!(first_generated_return, second_generated_return);
}

#[test]
fn boundary_state_update_is_consistent_with_shared_transition_api_reference_sequence() {
    let initial_state = build_initial_state();
    let kernel_parameters = build_equation_specification().hawkes_kernel_parameters;
    let sampled_statistics = SufficientStatisticsIntervalStatistics::try_new(1.4, 1.1, 3, 2)
        .expect("statistics should be valid");
    let update_snapshot = SufficientStatisticsBoundaryStateUpdateSnapshot::new(
        3,
        0.75,
        initial_state,
        sampled_statistics,
    );

    assert_eq!(
        SufficientStatisticsBoundaryStateUpdateSnapshot::policy(),
        SUFFICIENT_STATISTICS_BOUNDARY_STATE_UPDATE_POLICY
    );

    let updated_state =
        update_boundary_state_from_interval_statistics(kernel_parameters, update_snapshot)
            .expect("boundary-state update should succeed");

    let mut reference_state = evolve_hawkes_state(
        initial_state,
        kernel_parameters,
        HawkesTransitionStep::new(0.75, None, None),
    )
    .expect("reference drift step should succeed");

    for _buy_event_position in 0..sampled_statistics.n_plus {
        reference_state = evolve_hawkes_state(
            reference_state,
            kernel_parameters,
            HawkesTransitionStep::new(0.0, Some(HawkesEventSide::Buy), None),
        )
        .expect("reference buy jump step should succeed");
    }

    for _sell_event_position in 0..sampled_statistics.n_minus {
        reference_state = evolve_hawkes_state(
            reference_state,
            kernel_parameters,
            HawkesTransitionStep::new(0.0, Some(HawkesEventSide::Sell), None),
        )
        .expect("reference sell jump step should succeed");
    }

    assert_float_close(updated_state.y_plus_plus, reference_state.y_plus_plus);
    assert_float_close(updated_state.y_plus_minus, reference_state.y_plus_minus);
    assert_float_close(updated_state.y_minus_plus, reference_state.y_minus_plus);
    assert_float_close(updated_state.y_minus_minus, reference_state.y_minus_minus);
}

#[test]
fn boundary_state_update_maps_sampled_counts_to_canonical_components() {
    let initial_state = build_initial_state();
    let kernel_parameters = build_equation_specification().hawkes_kernel_parameters;
    let sampled_statistics = SufficientStatisticsIntervalStatistics::try_new(0.8, 0.6, 4, 5)
        .expect("statistics should be valid");
    let interval_delta = 0.5;
    let update_snapshot = SufficientStatisticsBoundaryStateUpdateSnapshot::new(
        1,
        interval_delta,
        initial_state,
        sampled_statistics,
    );

    let updated_state =
        update_boundary_state_from_interval_statistics(kernel_parameters, update_snapshot)
            .expect("boundary-state mapping update should succeed");

    let shared_decay_factor = (-kernel_parameters.beta * interval_delta).exp();
    let expected_y_plus_plus = initial_state.y_plus_plus * shared_decay_factor
        + kernel_parameters.alpha.plus_plus * sampled_statistics.n_plus as f64;
    let expected_y_plus_minus = initial_state.y_plus_minus * shared_decay_factor
        + kernel_parameters.alpha.plus_minus * sampled_statistics.n_minus as f64;
    let expected_y_minus_plus = initial_state.y_minus_plus * shared_decay_factor
        + kernel_parameters.alpha.minus_plus * sampled_statistics.n_plus as f64;
    let expected_y_minus_minus = initial_state.y_minus_minus * shared_decay_factor
        + kernel_parameters.alpha.minus_minus * sampled_statistics.n_minus as f64;

    assert_float_close(updated_state.y_plus_plus, expected_y_plus_plus);
    assert_float_close(updated_state.y_plus_minus, expected_y_plus_minus);
    assert_float_close(updated_state.y_minus_plus, expected_y_minus_plus);
    assert_float_close(updated_state.y_minus_minus, expected_y_minus_minus);
}

#[test]
fn boundary_state_update_preserves_deterministic_multi_interval_carryover() {
    let kernel_parameters = build_equation_specification().hawkes_kernel_parameters;
    let interval_delta = 1.0;
    let sampled_statistics_path = [
        SufficientStatisticsIntervalStatistics::try_new(0.7, 0.5, 2, 1)
            .expect("first interval statistics should be valid"),
        SufficientStatisticsIntervalStatistics::try_new(0.4, 0.9, 1, 3)
            .expect("second interval statistics should be valid"),
        SufficientStatisticsIntervalStatistics::try_new(1.1, 0.2, 4, 0)
            .expect("third interval statistics should be valid"),
    ];

    let mut first_run_state = build_initial_state();
    let mut second_run_state = build_initial_state();
    let mut first_run_boundary_states: Vec<HawkesExcitationState> = Vec::new();
    let mut second_run_boundary_states: Vec<HawkesExcitationState> = Vec::new();

    for (interval_index, sampled_statistics) in sampled_statistics_path.iter().copied().enumerate()
    {
        let update_snapshot = SufficientStatisticsBoundaryStateUpdateSnapshot::new(
            interval_index,
            interval_delta,
            first_run_state,
            sampled_statistics,
        );
        first_run_state =
            update_boundary_state_from_interval_statistics(kernel_parameters, update_snapshot)
                .expect("first run carryover update should succeed");
        first_run_boundary_states.push(first_run_state);
    }

    for (interval_index, sampled_statistics) in sampled_statistics_path.iter().copied().enumerate()
    {
        let update_snapshot = SufficientStatisticsBoundaryStateUpdateSnapshot::new(
            interval_index,
            interval_delta,
            second_run_state,
            sampled_statistics,
        );
        second_run_state =
            update_boundary_state_from_interval_statistics(kernel_parameters, update_snapshot)
                .expect("second run carryover update should succeed");
        second_run_boundary_states.push(second_run_state);
    }

    assert_eq!(first_run_boundary_states, second_run_boundary_states);
    assert_eq!(first_run_state, second_run_state);
}

#[test]
fn boundary_state_update_failures_include_interval_and_state_snapshot_metadata() {
    let kernel_parameters = build_equation_specification().hawkes_kernel_parameters;
    let sampled_statistics = SufficientStatisticsIntervalStatistics::try_new(0.7, 0.5, 2, 1)
        .expect("statistics should be valid");
    let update_snapshot = SufficientStatisticsBoundaryStateUpdateSnapshot::new(
        9,
        0.0,
        build_initial_state(),
        sampled_statistics,
    );

    let error = update_boundary_state_from_interval_statistics(kernel_parameters, update_snapshot)
        .expect_err("zero interval delta should fail boundary-state update");

    assert!(matches!(
        error,
        SufficientStatisticsBoundaryStateUpdateError::InvalidIntervalDelta {
            snapshot,
            value: 0.0
        }
        if snapshot.interval_index == 9
            && snapshot.state_before_update == update_snapshot.state_before_update
            && snapshot.sampled_statistics == update_snapshot.sampled_statistics
    ));
}

#[test]
fn boundary_state_update_wraps_transition_failures_with_snapshot_metadata() {
    let invalid_kernel_parameters = HawkesKernelParameters {
        lambda_bar: 0.8,
        beta: 0.0,
        alpha: HawkesKernelLoadings {
            plus_plus: 0.2,
            plus_minus: 0.1,
            minus_plus: 0.1,
            minus_minus: 0.25,
        },
        rho: HawkesKernelLoadings {
            plus_plus: 0.08,
            plus_minus: 0.04,
            minus_plus: 0.05,
            minus_minus: 0.1,
        },
    };
    let update_snapshot = SufficientStatisticsBoundaryStateUpdateSnapshot::new(
        4,
        0.5,
        build_initial_state(),
        SufficientStatisticsIntervalStatistics::try_new(0.7, 0.5, 2, 1)
            .expect("statistics should be valid"),
    );

    let error =
        update_boundary_state_from_interval_statistics(invalid_kernel_parameters, update_snapshot)
            .expect_err("invalid beta should fail transition composition");

    assert!(matches!(
        error,
        SufficientStatisticsBoundaryStateUpdateError::TransitionFailure {
            snapshot,
            source: HawkesTransitionError::InvalidBeta { value: 0.0 }
        }
        if snapshot.interval_index == 4
            && snapshot.state_before_update == update_snapshot.state_before_update
    ));
}

#[test]
fn transform_slice_sampler_outputs_domain_sane_statistics() {
    let equation_specification = build_equation_specification();
    let context = build_transform_slice_context(0, 1.1, 0.2, 1.0);
    let settings = SufficientStatisticsTransformSliceSettings::default();

    let slices =
        solve_sufficient_statistics_transform_slices(equation_specification, context, settings)
            .expect("transform slices should solve");
    let sampled_statistics = sample_interval_statistics_from_transform_slices(17, slices)
        .expect("slice sampling should succeed");

    sampled_statistics
        .validate()
        .expect("sampled statistics should satisfy domain checks");

    let serialized =
        serde_json::to_value(sampled_statistics).expect("sampled statistics should serialize");

    assert!(serialized.get("a_plus").is_some());
    assert!(serialized.get("a_minus").is_some());
    assert!(serialized.get("n_plus").is_some());
    assert!(serialized.get("n_minus").is_some());
}

#[test]
fn transform_slice_sampler_is_seed_deterministic_for_repeated_sampling() {
    let equation_specification = build_equation_specification();
    let context = build_transform_slice_context(1, 0.9, 0.1, 1.0);
    let settings = SufficientStatisticsTransformSliceSettings::default();
    let slices =
        solve_sufficient_statistics_transform_slices(equation_specification, context, settings)
            .expect("transform slices should solve");

    let first_sample = sample_interval_statistics_from_transform_slices(123, slices)
        .expect("first sampling run should succeed");
    let second_sample = sample_interval_statistics_from_transform_slices(123, slices)
        .expect("second sampling run should succeed");

    assert_eq!(first_sample, second_sample);
}

#[test]
fn transform_slice_batch_sampler_is_seed_deterministic_for_repeated_sampling() {
    let equation_specification = build_equation_specification();
    let settings = SufficientStatisticsTransformSliceSettings::default();
    let first_interval_slices = solve_sufficient_statistics_transform_slices(
        equation_specification,
        build_transform_slice_context(0, 1.1, 0.2, 1.0),
        settings,
    )
    .expect("first interval slices should solve");
    let second_interval_slices = solve_sufficient_statistics_transform_slices(
        equation_specification,
        build_transform_slice_context(1, 0.8, 0.15, 1.0),
        settings,
    )
    .expect("second interval slices should solve");
    let batch = vec![first_interval_slices, second_interval_slices];

    let first_batch_sample = sample_interval_statistics_batch_from_transform_slices(77, &batch)
        .expect("first batch sampling run should succeed");
    let second_batch_sample = sample_interval_statistics_batch_from_transform_slices(77, &batch)
        .expect("second batch sampling run should succeed");

    assert_eq!(first_batch_sample, second_batch_sample);
}

#[test]
fn transform_slice_streaming_sampler_matches_batch_sampler_with_ordered_complete_output() {
    let equation_specification = build_equation_specification();
    let settings = SufficientStatisticsTransformSliceSettings::default();

    let slices_batch = vec![
        solve_sufficient_statistics_transform_slices(
            equation_specification,
            build_transform_slice_context(0, 1.1, 0.2, 1.0),
            settings,
        )
        .expect("first interval slices should solve"),
        solve_sufficient_statistics_transform_slices(
            equation_specification,
            build_transform_slice_context(1, 0.8, 0.15, 1.0),
            settings,
        )
        .expect("second interval slices should solve"),
        solve_sufficient_statistics_transform_slices(
            equation_specification,
            build_transform_slice_context(2, 0.7, 0.1, 1.0),
            settings,
        )
        .expect("third interval slices should solve"),
    ];

    let chunking_settings = SufficientStatisticsStreamingChunkingSettings::try_new(2)
        .expect("chunking settings should be valid");

    assert_eq!(
        SufficientStatisticsStreamingChunkingSettings::policy(),
        SUFFICIENT_STATISTICS_STREAMING_CHUNKING_POLICY
    );

    let mut streamed_chunks = Vec::new();
    stream_interval_statistics_from_transform_slices(
        77,
        &slices_batch,
        chunking_settings,
        |chunk| streamed_chunks.push(chunk),
    )
    .expect("streaming sampling should succeed");

    assert_eq!(streamed_chunks.len(), 2);
    assert_eq!(streamed_chunks[0].chunk_index, 0);
    assert_eq!(streamed_chunks[0].start_interval_index, 0);
    assert_eq!(streamed_chunks[0].records.len(), 2);
    assert_eq!(streamed_chunks[1].chunk_index, 1);
    assert_eq!(streamed_chunks[1].start_interval_index, 2);
    assert_eq!(streamed_chunks[1].records.len(), 1);

    let streamed_statistics: Vec<SufficientStatisticsIntervalStatistics> = streamed_chunks
        .iter()
        .flat_map(|chunk| chunk.records.iter().copied())
        .collect();
    let batched_statistics =
        sample_interval_statistics_batch_from_transform_slices(77, &slices_batch)
            .expect("batch sampling should succeed");

    assert_eq!(streamed_statistics, batched_statistics);
}

#[test]
fn transform_slice_multi_path_batch_sampler_uses_deterministic_seed_partitioning() {
    let equation_specification = build_equation_specification();
    let settings = SufficientStatisticsTransformSliceSettings::default();

    let first_path = vec![
        solve_sufficient_statistics_transform_slices(
            equation_specification,
            build_transform_slice_context(0, 1.1, 0.2, 1.0),
            settings,
        )
        .expect("first path first interval should solve"),
        solve_sufficient_statistics_transform_slices(
            equation_specification,
            build_transform_slice_context(1, 0.85, 0.13, 1.0),
            settings,
        )
        .expect("first path second interval should solve"),
    ];
    let second_path = vec![
        solve_sufficient_statistics_transform_slices(
            equation_specification,
            build_transform_slice_context(0, 0.9, 0.11, 1.0),
            settings,
        )
        .expect("second path first interval should solve"),
        solve_sufficient_statistics_transform_slices(
            equation_specification,
            build_transform_slice_context(1, 0.75, 0.09, 1.0),
            settings,
        )
        .expect("second path second interval should solve"),
    ];

    let path_batch = vec![first_path, second_path];
    let chunking_settings = SufficientStatisticsStreamingChunkingSettings::try_new(1)
        .expect("chunking settings should be valid");

    assert_eq!(
        sufficient_statistics_batch_seed_partition_policy(),
        SUFFICIENT_STATISTICS_BATCH_SEED_PARTITION_POLICY
    );

    let first_batch = sample_interval_statistics_multi_path_batch_from_transform_slices(
        1_901,
        &path_batch,
        chunking_settings,
    )
    .expect("first multi-path batch sampling run should succeed");
    let second_batch = sample_interval_statistics_multi_path_batch_from_transform_slices(
        1_901,
        &path_batch,
        chunking_settings,
    )
    .expect("second multi-path batch sampling run should succeed");

    assert_eq!(first_batch, second_batch);

    for (path_index, path_slices) in path_batch.iter().enumerate() {
        let path_seed = derive_sufficient_statistics_batch_path_seed(1_901, path_index as u64);
        let expected_path_statistics =
            sample_interval_statistics_batch_from_transform_slices(path_seed, path_slices)
                .expect("single-path sampling with partitioned seed should succeed");
        assert_eq!(first_batch[path_index], expected_path_statistics);
    }

    let mut streamed_path_chunks = Vec::new();
    stream_interval_statistics_multi_path_batch_from_transform_slices(
        1_901,
        &path_batch,
        chunking_settings,
        |path_chunk| streamed_path_chunks.push(path_chunk),
    )
    .expect("multi-path streaming sampling should succeed");

    assert_eq!(streamed_path_chunks.len(), 4);
    assert_eq!(streamed_path_chunks[0].path_index, 0);
    assert_eq!(streamed_path_chunks[1].path_index, 0);
    assert_eq!(streamed_path_chunks[2].path_index, 1);
    assert_eq!(streamed_path_chunks[3].path_index, 1);
    assert_eq!(
        streamed_path_chunks[0].interval_chunk.start_interval_index,
        0
    );
    assert_eq!(
        streamed_path_chunks[1].interval_chunk.start_interval_index,
        1
    );
    assert_eq!(
        streamed_path_chunks[2].interval_chunk.start_interval_index,
        0
    );
    assert_eq!(
        streamed_path_chunks[3].interval_chunk.start_interval_index,
        1
    );
}

#[test]
fn transform_slice_streaming_sampler_rejects_zero_chunk_size() {
    let equation_specification = build_equation_specification();
    let settings = SufficientStatisticsTransformSliceSettings::default();

    let slices_batch = vec![
        solve_sufficient_statistics_transform_slices(
            equation_specification,
            build_transform_slice_context(0, 1.1, 0.2, 1.0),
            settings,
        )
        .expect("interval slices should solve"),
    ];

    let invalid_chunking_settings = SufficientStatisticsStreamingChunkingSettings {
        max_intervals_per_chunk: 0,
    };

    let error = stream_interval_statistics_from_transform_slices(
        77,
        &slices_batch,
        invalid_chunking_settings,
        |_chunk| {},
    )
    .expect_err("zero chunk size must fail streaming execution");

    assert!(matches!(
        error,
        SufficientStatisticsTransformSliceExecutionError::InvalidChunkingSettings { .. }
    ));
}

#[test]
fn transform_slice_sampler_rejects_non_real_transform_inputs_with_context() {
    let context = build_transform_slice_context(4, 1.2, 0.3, 1.0);
    let slices = SufficientStatisticsTransformSlices::try_new(
        context,
        SufficientStatisticsTransformSliceSettings::default(),
        ComplexScalar::new(1.0, 1.0e-3),
        ComplexScalar::new(0.99, 0.0),
        ComplexScalar::new(0.98, 0.0),
        ComplexScalar::new(0.97, 0.0),
        ComplexScalar::new(0.96, 0.0),
    )
    .expect("finite transform payload should construct");

    let error = sample_interval_statistics_from_transform_slices(8, slices)
        .expect_err("non-real transform slice should fail sampling");
    assert!(matches!(
        error,
        SufficientStatisticsTransformSliceSamplingError::NonRealTransformSlice {
            interval_index: 4,
            field: "baseline_transform",
            ..
        }
    ));
}

#[test]
fn transform_slice_sampler_rejects_invalid_expected_statistics_from_invalid_slices() {
    let context = build_transform_slice_context(6, 1.2, 0.3, 1.0);
    let settings = SufficientStatisticsTransformSliceSettings::default();
    let invalid_slices = SufficientStatisticsTransformSlices::try_new(
        context,
        settings,
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(1.1, 0.0),
        ComplexScalar::new(0.95, 0.0),
        ComplexScalar::new(0.98, 0.0),
        ComplexScalar::new(0.97, 0.0),
    )
    .expect("finite transform payload should construct");

    let error = sample_interval_statistics_from_transform_slices(8, invalid_slices)
        .expect_err("invalid expected A-plus value should fail sampling");
    assert!(matches!(
        error,
        SufficientStatisticsTransformSliceSamplingError::InvalidExpectedStatistic {
            interval_index: 6,
            statistic: "a_plus",
            ..
        }
    ));
}

#[test]
fn transform_slice_settings_rejects_invalid_probe_values() {
    let invalid_settings =
        SufficientStatisticsTransformSliceSettings::try_new(1.0e-3, 1.0e-3, 1.0, 0.99)
            .expect_err("n_plus_z equal to one must fail validation");

    assert_eq!(
        invalid_settings,
        SufficientStatisticsTransformSliceSettingsError::InvalidNPlusZ { value: 1.0 }
    );
}

fn build_valid_run_request() -> SufficientStatisticsRiskRunRequest {
    SufficientStatisticsRiskRunRequest::try_new(
        17,
        2,
        1.0,
        build_model_inputs(),
        build_interval_factors(),
    )
    .expect("run request should be valid")
}

fn build_transform_slice_context(
    interval_index: usize,
    s_h: f64,
    total_nu_h: f64,
    delta: f64,
) -> SufficientStatisticsTransformSliceContext {
    let known_nu_h = total_nu_h * 0.5;
    let surprise_nu_h = total_nu_h - known_nu_h;
    SufficientStatisticsTransformSliceContext::try_new(
        interval_index,
        s_h,
        ExponentialAffineTransformNewsIntensityInput::try_new(known_nu_h, surprise_nu_h)
            .expect("slice context news_intensity should be valid"),
        delta,
        build_initial_state(),
    )
    .expect("slice context should be valid")
}

fn build_equation_specification() -> ExponentialAffineTransformEquationSpecification {
    let kernel_parameters = HawkesKernelParameters::try_new(
        0.8,
        1.6,
        HawkesKernelLoadings {
            plus_plus: 0.2,
            plus_minus: 0.1,
            minus_plus: 0.1,
            minus_minus: 0.25,
        },
        HawkesKernelLoadings {
            plus_plus: 0.08,
            plus_minus: 0.04,
            minus_plus: 0.05,
            minus_minus: 0.1,
        },
    )
    .expect("kernel parameters should be valid");

    ExponentialAffineTransformEquationSpecification::try_new(
        kernel_parameters,
        GammaSeverityMarkConfiguration::try_new(1.8, 0.7).expect("gamma marks should be valid"),
    )
    .expect("equation specification should be valid")
}

fn build_model_inputs() -> SufficientStatisticsRiskModelInputs {
    SufficientStatisticsRiskModelInputs::try_new(
        build_equation_specification(),
        NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("nig configuration should be valid"),
        build_initial_state(),
    )
    .expect("model inputs should be valid")
}

fn valid_nig_driver_parameters() -> crate::nig::NigBuySellDriverParameters {
    build_model_inputs()
        .nig_shared_plus_shift_configuration
        .derive_driver_parameters()
        .expect("nig driver parameters should derive from shared-plus-shift configuration")
}

fn build_interval_factors() -> SufficientStatisticsRiskIntervalFactors {
    SufficientStatisticsRiskIntervalFactors::try_new(
        vec![1.1, 0.9],
        vec![0.12, 0.03],
        vec![0.08, 0.07],
        None,
    )
    .expect("interval factors should be valid")
}

fn build_interval_factors_with_interval_delta(
    interval_delta: f64,
) -> SufficientStatisticsRiskIntervalFactors {
    SufficientStatisticsRiskIntervalFactors::try_new(
        vec![1.1, 0.9],
        vec![0.12, 0.03],
        vec![0.08, 0.07],
        Some(interval_delta),
    )
    .expect("interval factors with explicit interval_delta should be valid")
}

fn build_initial_state() -> HawkesExcitationState {
    HawkesExcitationState {
        y_plus_plus: 0.2,
        y_plus_minus: 0.1,
        y_minus_plus: 0.3,
        y_minus_minus: 0.4,
    }
}

fn assert_float_close(left: f64, right: f64) {
    assert!(
        (left - right).abs() <= 1.0e-12,
        "float mismatch: left={left}, right={right}, abs_diff={}",
        (left - right).abs()
    );
}
