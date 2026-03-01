use crate::hawkes::{
    HAWKES_KERNEL_PARAMETER_VALIDATION_DETERMINISM_POLICY,
    HAWKES_KERNEL_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION, HAWKES_STRICT_STATIONARITY_THRESHOLD,
    HawkesEventSide, HawkesEventSidePayloadError, HawkesExcitationState, HawkesKernelLoadings,
    HawkesKernelParameterConstraint, HawkesKernelParameterError, HawkesKernelParameterViolation,
    HawkesKernelParameters, HawkesKernelValidationStatus, HawkesNewsSeverityMarkError,
    HawkesSeasonalityIntervalError, HawkesSourceComponents, HawkesStructuredAlphaParameterError,
    HawkesStructuredAlphaParameterization, HawkesStructuredRhoParameterError,
    HawkesStructuredRhoParameterization, HawkesStructuredRhoSelfDominancePolicy,
    HawkesTransitionError, HawkesTransitionStep,
};
use crate::hourly_factors::{
    HourlyFactorContractDiagnostics, HourlyFactorContractOutput,
    HourlyFactorMaterializationMetadata,
};
use serde_json::json;

const DECAY_TOLERANCE: f64 = 1e-12;

fn assert_within_tolerance(actual: f64, expected: f64, tolerance: f64, field_name: &str) {
    let absolute_difference = (actual - expected).abs();
    assert!(
        absolute_difference <= tolerance,
        "{field_name} mismatch expected {expected}, found {actual}, absolute_difference={absolute_difference}, tolerance={tolerance}"
    );
}

fn build_hourly_factor_contract_with_seasonality(
    seasonality_factors: Vec<f64>,
) -> HourlyFactorContractOutput {
    let interval_count = seasonality_factors.len();
    let interval_indices = (0..interval_count)
        .map(|interval_index| {
            u64::try_from(interval_index).expect("test interval index must fit in u64")
        })
        .collect::<Vec<_>>();
    let interval_start_utc_nanos = (0..interval_count)
        .map(|interval_index| {
            i64::try_from(interval_index).expect("test interval index must fit in i64")
                * 3_600_000_000_000
        })
        .collect::<Vec<_>>();
    let interval_end_utc_nanos = interval_start_utc_nanos
        .iter()
        .copied()
        .map(|interval_start| interval_start + 3_600_000_000_000)
        .collect::<Vec<_>>();
    let interval_start_exchange_local = (0..interval_count)
        .map(|hour| format!("2026-02-02 {hour:02}:00:00-05:00"))
        .collect::<Vec<_>>();
    let interval_end_exchange_local = (0..interval_count)
        .map(|hour| format!("2026-02-02 {:02}:00:00-05:00", hour + 1))
        .collect::<Vec<_>>();

    HourlyFactorContractOutput {
        schema_version: "test.hourly_factor.contract.v1",
        exchange_code: "XNYS".to_string(),
        exchange_timezone_name: "America/New_York".to_string(),
        request_start_utc_nanos: 0,
        request_end_utc_nanos: i64::try_from(interval_count)
            .expect("test interval count must fit in i64")
            * 3_600_000_000_000,
        interval_indices,
        interval_start_utc_nanos,
        interval_end_utc_nanos,
        interval_start_exchange_local,
        interval_end_exchange_local,
        s_h: seasonality_factors,
        nu_h: vec![0.0; interval_count],
        metadata: HourlyFactorMaterializationMetadata {
            seasonality_materialization_method: "test.seasonality.v1".to_string(),
            news_intensity_materialization_method: "test.news.v1".to_string(),
            interval_boundary_policy: "test.interval_boundary_policy.v1",
            interval_alignment_policy: "test.interval_alignment_policy.v1",
            timezone_transition_policy: "test.timezone_transition_policy.v1",
            spring_forward_policy: "test.spring_forward_policy.v1",
            fall_back_policy: "test.fall_back_policy.v1",
        },
        diagnostics: HourlyFactorContractDiagnostics {
            interval_count,
            first_interval_start_utc_nanos: 0,
            last_interval_end_utc_nanos: i64::try_from(interval_count)
                .expect("test interval count must fit in i64")
                * 3_600_000_000_000,
            skipped_local_hour_interval_count: 0,
            repeated_local_hour_interval_count: 0,
        },
    }
}

#[test]
fn excitation_state_from_target_components_preserves_component_mapping() {
    let target_plus = HawkesSourceComponents::new(1.0, 2.0);
    let target_minus = HawkesSourceComponents::new(3.0, 4.0);

    let state = HawkesExcitationState::from_target_components(target_plus, target_minus);

    assert_eq!(state.y_plus_plus, 1.0);
    assert_eq!(state.y_plus_minus, 2.0);
    assert_eq!(state.y_minus_plus, 3.0);
    assert_eq!(state.y_minus_minus, 4.0);
    assert_eq!(state.target_plus_components(), target_plus);
    assert_eq!(state.target_minus_components(), target_minus);
    assert_eq!(state.as_model_ordered_array(), [1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn shared_decay_drift_matches_closed_form_per_component() {
    let state = HawkesExcitationState {
        y_plus_plus: 0.75,
        y_plus_minus: 1.2,
        y_minus_plus: 0.05,
        y_minus_minus: 2.8,
    };
    let beta = 1.7;
    let delta_t = 0.35;

    let drifted_state = state.apply_shared_decay_drift(beta, delta_t);
    let decay_factor = (-beta * delta_t).exp();

    assert_within_tolerance(
        drifted_state.y_plus_plus,
        state.y_plus_plus * decay_factor,
        DECAY_TOLERANCE,
        "y_plus_plus",
    );
    assert_within_tolerance(
        drifted_state.y_plus_minus,
        state.y_plus_minus * decay_factor,
        DECAY_TOLERANCE,
        "y_plus_minus",
    );
    assert_within_tolerance(
        drifted_state.y_minus_plus,
        state.y_minus_plus * decay_factor,
        DECAY_TOLERANCE,
        "y_minus_plus",
    );
    assert_within_tolerance(
        drifted_state.y_minus_minus,
        state.y_minus_minus * decay_factor,
        DECAY_TOLERANCE,
        "y_minus_minus",
    );
}

#[test]
fn intensity_projection_matches_deseasonalized_and_scaled_equations() {
    let state = HawkesExcitationState {
        y_plus_plus: 1.2,
        y_plus_minus: 0.4,
        y_minus_plus: 0.3,
        y_minus_minus: 0.5,
    };

    let projection = state.project_intensities(0.8, 1.25);

    assert_within_tolerance(
        projection.deseasonalized.lambda_plus,
        2.4,
        DECAY_TOLERANCE,
        "deseasonalized.lambda_plus",
    );
    assert_within_tolerance(
        projection.deseasonalized.lambda_minus,
        1.6,
        DECAY_TOLERANCE,
        "deseasonalized.lambda_minus",
    );
    assert_within_tolerance(
        projection.seasonality_scaled.lambda_plus,
        3.0,
        DECAY_TOLERANCE,
        "seasonality_scaled.lambda_plus",
    );
    assert_within_tolerance(
        projection.seasonality_scaled.lambda_minus,
        2.0,
        DECAY_TOLERANCE,
        "seasonality_scaled.lambda_minus",
    );
}

#[test]
fn intensity_projection_handles_hourly_piecewise_seasonality_edge_values() {
    let state = HawkesExcitationState {
        y_plus_plus: 0.15,
        y_plus_minus: 0.05,
        y_minus_plus: 0.10,
        y_minus_minus: 0.20,
    };
    let lambda_bar = 0.40;
    let seasonality_factors = [0.25, 1.0, 3.5];
    let expected_scaled_plus = [0.15, 0.60, 2.10];
    let expected_scaled_minus = [0.175, 0.70, 2.45];

    for (position, seasonality_factor) in seasonality_factors.iter().copied().enumerate() {
        let projection = state.project_intensities(lambda_bar, seasonality_factor);

        assert_within_tolerance(
            projection.deseasonalized.lambda_plus,
            0.60,
            DECAY_TOLERANCE,
            "deseasonalized.lambda_plus",
        );
        assert_within_tolerance(
            projection.deseasonalized.lambda_minus,
            0.70,
            DECAY_TOLERANCE,
            "deseasonalized.lambda_minus",
        );
        assert_within_tolerance(
            projection.seasonality_scaled.lambda_plus,
            expected_scaled_plus[position],
            DECAY_TOLERANCE,
            "seasonality_scaled.lambda_plus",
        );
        assert_within_tolerance(
            projection.seasonality_scaled.lambda_minus,
            expected_scaled_minus[position],
            DECAY_TOLERANCE,
            "seasonality_scaled.lambda_minus",
        );
    }
}

#[test]
fn intensity_projection_is_symmetric_when_side_state_components_match() {
    let symmetric_state = HawkesExcitationState {
        y_plus_plus: 1.1,
        y_plus_minus: 0.9,
        y_minus_plus: 1.1,
        y_minus_minus: 0.9,
    };

    let projection = symmetric_state.project_intensities(0.7, 1.4);

    assert_within_tolerance(
        projection.deseasonalized.lambda_plus,
        projection.deseasonalized.lambda_minus,
        DECAY_TOLERANCE,
        "deseasonalized symmetry",
    );
    assert_within_tolerance(
        projection.seasonality_scaled.lambda_plus,
        projection.seasonality_scaled.lambda_minus,
        DECAY_TOLERANCE,
        "seasonality_scaled symmetry",
    );
}

#[test]
fn hourly_interval_projection_reads_seasonality_from_shared_contract() {
    let state = HawkesExcitationState {
        y_plus_plus: 0.2,
        y_plus_minus: 0.1,
        y_minus_plus: 0.4,
        y_minus_minus: 0.3,
    };
    let hourly_factor_contract = build_hourly_factor_contract_with_seasonality(vec![0.8, 1.3, 0.6]);

    let projection = state
        .project_intensities_for_hourly_interval(0.5, &hourly_factor_contract, 1)
        .expect("interval 1 must resolve from shared hourly contract");

    assert_within_tolerance(
        projection.deseasonalized.lambda_plus,
        0.8,
        DECAY_TOLERANCE,
        "deseasonalized.lambda_plus",
    );
    assert_within_tolerance(
        projection.deseasonalized.lambda_minus,
        1.2,
        DECAY_TOLERANCE,
        "deseasonalized.lambda_minus",
    );
    assert_within_tolerance(
        projection.seasonality_scaled.lambda_plus,
        1.04,
        DECAY_TOLERANCE,
        "seasonality_scaled.lambda_plus",
    );
    assert_within_tolerance(
        projection.seasonality_scaled.lambda_minus,
        1.56,
        DECAY_TOLERANCE,
        "seasonality_scaled.lambda_minus",
    );
}

#[test]
fn hourly_interval_projection_rejects_out_of_range_interval_index() {
    let state = HawkesExcitationState {
        y_plus_plus: 0.2,
        y_plus_minus: 0.1,
        y_minus_plus: 0.4,
        y_minus_minus: 0.3,
    };
    let hourly_factor_contract = build_hourly_factor_contract_with_seasonality(vec![1.0, 0.9]);

    let error = state
        .project_intensities_for_hourly_interval(0.5, &hourly_factor_contract, 2)
        .expect_err("out-of-range interval index must fail");

    assert_eq!(
        error,
        HawkesSeasonalityIntervalError {
            interval_index: 2,
            interval_count: 2,
        }
    );
    assert_eq!(
        error.to_string(),
        "interval_index 2 is out of range for hourly seasonality factor count 2"
    );
}

#[test]
fn shared_decay_drift_with_zero_delta_t_preserves_state_exactly() {
    let state = HawkesExcitationState {
        y_plus_plus: 0.9,
        y_plus_minus: 0.4,
        y_minus_plus: 0.8,
        y_minus_minus: 1.1,
    };

    let drifted_state = state.apply_shared_decay_drift(1.25, 0.0);

    assert_eq!(drifted_state, state);
}

#[test]
fn shared_decay_drift_with_large_delta_t_underflows_to_zero() {
    let state = HawkesExcitationState {
        y_plus_plus: 0.9,
        y_plus_minus: 0.4,
        y_minus_plus: 0.8,
        y_minus_minus: 1.1,
    };

    let drifted_state = state.apply_shared_decay_drift(1.0, 1_000.0);

    assert_eq!(
        drifted_state,
        HawkesExcitationState {
            y_plus_plus: 0.0,
            y_plus_minus: 0.0,
            y_minus_plus: 0.0,
            y_minus_minus: 0.0,
        }
    );
}

#[test]
fn buy_event_jump_applies_k_plus_component_mapping() {
    let state = HawkesExcitationState {
        y_plus_plus: 0.5,
        y_plus_minus: 0.7,
        y_minus_plus: 0.9,
        y_minus_minus: 1.1,
    };
    let alpha = HawkesKernelLoadings {
        plus_plus: 0.2,
        plus_minus: 0.3,
        minus_plus: 0.4,
        minus_minus: 0.6,
    };

    let updated_state = state.apply_buy_event_jump(alpha);

    assert_eq!(
        updated_state,
        HawkesExcitationState {
            y_plus_plus: 0.7,
            y_plus_minus: 0.7,
            y_minus_plus: 1.3,
            y_minus_minus: 1.1,
        }
    );
}

#[test]
fn sell_event_jump_applies_k_minus_component_mapping() {
    let state = HawkesExcitationState {
        y_plus_plus: 0.5,
        y_plus_minus: 0.7,
        y_minus_plus: 0.9,
        y_minus_minus: 1.1,
    };
    let alpha = HawkesKernelLoadings {
        plus_plus: 0.2,
        plus_minus: 0.3,
        minus_plus: 0.4,
        minus_minus: 0.6,
    };

    let updated_state = state.apply_sell_event_jump(alpha);

    assert_within_tolerance(
        updated_state.y_plus_plus,
        0.5,
        DECAY_TOLERANCE,
        "y_plus_plus",
    );
    assert_within_tolerance(
        updated_state.y_plus_minus,
        1.0,
        DECAY_TOLERANCE,
        "y_plus_minus",
    );
    assert_within_tolerance(
        updated_state.y_minus_plus,
        0.9,
        DECAY_TOLERANCE,
        "y_minus_plus",
    );
    assert_within_tolerance(
        updated_state.y_minus_minus,
        1.7,
        DECAY_TOLERANCE,
        "y_minus_minus",
    );
}

#[test]
fn interleaved_event_jump_sequence_composes_with_drift() {
    let state = HawkesExcitationState {
        y_plus_plus: 1.0,
        y_plus_minus: 2.0,
        y_minus_plus: 3.0,
        y_minus_minus: 4.0,
    };
    let alpha = HawkesKernelLoadings {
        plus_plus: 0.6,
        plus_minus: 0.1,
        minus_plus: 0.2,
        minus_minus: 0.7,
    };

    let updated_state = state
        .apply_shared_decay_drift(1.0, 0.5)
        .apply_endogenous_event_jump(HawkesEventSide::Buy, alpha)
        .apply_endogenous_event_jump(HawkesEventSide::Sell, alpha)
        .apply_endogenous_event_jump(HawkesEventSide::Buy, alpha);

    let decay_factor = (-0.5f64).exp();

    assert_within_tolerance(
        updated_state.y_plus_plus,
        1.0 * decay_factor + 1.2,
        DECAY_TOLERANCE,
        "y_plus_plus",
    );
    assert_within_tolerance(
        updated_state.y_plus_minus,
        2.0 * decay_factor + 0.1,
        DECAY_TOLERANCE,
        "y_plus_minus",
    );
    assert_within_tolerance(
        updated_state.y_minus_plus,
        3.0 * decay_factor + 0.4,
        DECAY_TOLERANCE,
        "y_minus_plus",
    );
    assert_within_tolerance(
        updated_state.y_minus_minus,
        4.0 * decay_factor + 0.7,
        DECAY_TOLERANCE,
        "y_minus_minus",
    );
}

#[test]
fn exogenous_news_loading_jump_applies_component_wise_rho_scaled_by_severity_mark() {
    let state = HawkesExcitationState {
        y_plus_plus: 1.0,
        y_plus_minus: 1.5,
        y_minus_plus: 2.0,
        y_minus_minus: 2.5,
    };
    let rho = HawkesKernelLoadings {
        plus_plus: 0.2,
        plus_minus: 0.1,
        minus_plus: 0.05,
        minus_minus: 0.3,
    };
    let severity_mark = 2.5;

    let updated_state = state
        .apply_exogenous_news_loading_jump(rho, severity_mark)
        .expect("valid non-negative severity mark must succeed");

    assert_within_tolerance(
        updated_state.y_plus_plus,
        1.0 + 0.2 * severity_mark,
        DECAY_TOLERANCE,
        "y_plus_plus",
    );
    assert_within_tolerance(
        updated_state.y_plus_minus,
        1.5 + 0.1 * severity_mark,
        DECAY_TOLERANCE,
        "y_plus_minus",
    );
    assert_within_tolerance(
        updated_state.y_minus_plus,
        2.0 + 0.05 * severity_mark,
        DECAY_TOLERANCE,
        "y_minus_plus",
    );
    assert_within_tolerance(
        updated_state.y_minus_minus,
        2.5 + 0.3 * severity_mark,
        DECAY_TOLERANCE,
        "y_minus_minus",
    );
}

#[test]
fn exogenous_news_loading_jump_with_zero_severity_preserves_state_exactly() {
    let state = HawkesExcitationState {
        y_plus_plus: 0.8,
        y_plus_minus: 0.7,
        y_minus_plus: 0.6,
        y_minus_minus: 0.5,
    };
    let rho = HawkesKernelLoadings {
        plus_plus: 0.2,
        plus_minus: 0.1,
        minus_plus: 0.3,
        minus_minus: 0.4,
    };

    let updated_state = state
        .apply_exogenous_news_loading_jump(rho, 0.0)
        .expect("zero severity mark is valid and deterministic");

    assert_eq!(updated_state, state);
}

#[test]
fn exogenous_news_loading_jump_rejects_negative_severity_mark() {
    let state = HawkesExcitationState {
        y_plus_plus: 0.1,
        y_plus_minus: 0.2,
        y_minus_plus: 0.3,
        y_minus_minus: 0.4,
    };
    let rho = HawkesKernelLoadings {
        plus_plus: 0.0,
        plus_minus: 0.0,
        minus_plus: 0.0,
        minus_minus: 0.0,
    };

    let error = state
        .apply_exogenous_news_loading_jump(rho, -0.000_1)
        .expect_err("negative severity mark must fail");

    assert_eq!(error, HawkesNewsSeverityMarkError { value: -0.000_1 });
    assert_eq!(
        error.to_string(),
        "news severity mark must be finite and non-negative, found -0.0001"
    );
}

#[test]
fn exogenous_and_endogenous_jumps_compose_with_drift() {
    let state = HawkesExcitationState {
        y_plus_plus: 1.0,
        y_plus_minus: 2.0,
        y_minus_plus: 3.0,
        y_minus_minus: 4.0,
    };
    let alpha = HawkesKernelLoadings {
        plus_plus: 0.3,
        plus_minus: 0.7,
        minus_plus: 0.5,
        minus_minus: 0.9,
    };
    let rho = HawkesKernelLoadings {
        plus_plus: 0.2,
        plus_minus: 0.1,
        minus_plus: 0.05,
        minus_minus: 0.12,
    };
    let severity_mark = 1.5;
    let decay_factor = (-0.4f64).exp();

    let updated_state = state
        .apply_shared_decay_drift(1.0, 0.4)
        .apply_endogenous_event_jump(HawkesEventSide::Buy, alpha)
        .apply_exogenous_news_loading_jump(rho, severity_mark)
        .expect("valid non-negative severity mark must succeed")
        .apply_endogenous_event_jump(HawkesEventSide::Sell, alpha);

    assert_within_tolerance(
        updated_state.y_plus_plus,
        1.0 * decay_factor + 0.3 + 0.2 * severity_mark,
        DECAY_TOLERANCE,
        "y_plus_plus",
    );
    assert_within_tolerance(
        updated_state.y_plus_minus,
        2.0 * decay_factor + 0.1 * severity_mark + 0.7,
        DECAY_TOLERANCE,
        "y_plus_minus",
    );
    assert_within_tolerance(
        updated_state.y_minus_plus,
        3.0 * decay_factor + 0.5 + 0.05 * severity_mark,
        DECAY_TOLERANCE,
        "y_minus_plus",
    );
    assert_within_tolerance(
        updated_state.y_minus_minus,
        4.0 * decay_factor + 0.12 * severity_mark + 0.9,
        DECAY_TOLERANCE,
        "y_minus_minus",
    );
}

#[test]
fn composed_transition_step_matches_reference_call_order_sequence() {
    let state = HawkesExcitationState {
        y_plus_plus: 1.0,
        y_plus_minus: 2.0,
        y_minus_plus: 3.0,
        y_minus_minus: 4.0,
    };
    let kernel_parameters = HawkesKernelParameters::try_new(
        0.75,
        1.0,
        HawkesKernelLoadings {
            plus_plus: 0.3,
            plus_minus: 0.2,
            minus_plus: 0.1,
            minus_minus: 0.4,
        },
        HawkesKernelLoadings {
            plus_plus: 0.2,
            plus_minus: 0.1,
            minus_plus: 0.05,
            minus_minus: 0.12,
        },
    )
    .expect("kernel fixture must be valid");
    let transition_step = HawkesTransitionStep::new(0.4, Some(HawkesEventSide::Buy), Some(1.5));

    let expected_state = state
        .apply_shared_decay_drift(kernel_parameters.beta, transition_step.delta_t)
        .apply_endogenous_event_jump(HawkesEventSide::Buy, kernel_parameters.alpha)
        .apply_exogenous_news_loading_jump(
            kernel_parameters.rho,
            transition_step
                .exogenous_news_severity_mark
                .expect("fixture must define exogenous severity mark"),
        )
        .expect("fixture severity mark must be valid");

    let composed_state = state
        .apply_transition_step(kernel_parameters, transition_step)
        .expect("canonical transition composition should succeed");

    assert_eq!(composed_state, expected_state);
}

#[test]
fn composed_transition_sequence_matches_manual_step_iteration() {
    let state = HawkesExcitationState {
        y_plus_plus: 0.8,
        y_plus_minus: 0.4,
        y_minus_plus: 0.2,
        y_minus_minus: 0.5,
    };

    let kernel_parameters = HawkesKernelParameters::try_new(
        0.60,
        1.2,
        HawkesKernelLoadings {
            plus_plus: 0.15,
            plus_minus: 0.05,
            minus_plus: 0.25,
            minus_minus: 0.2,
        },
        HawkesKernelLoadings {
            plus_plus: 0.2,
            plus_minus: 0.15,
            minus_plus: 0.1,
            minus_minus: 0.05,
        },
    )
    .expect("kernel fixture must be valid");

    let transition_steps = [
        HawkesTransitionStep::new(0.3, Some(HawkesEventSide::Sell), None),
        HawkesTransitionStep::new(0.1, None, Some(0.75)),
        HawkesTransitionStep::new(0.0, Some(HawkesEventSide::Buy), Some(0.0)),
    ];

    let expected_state = transition_steps
        .into_iter()
        .try_fold(state, |next_state, transition_step| {
            let mut manual_state = next_state
                .apply_shared_decay_drift(kernel_parameters.beta, transition_step.delta_t);
            if let Some(endogenous_event_side) = transition_step.endogenous_event_side {
                manual_state = manual_state
                    .apply_endogenous_event_jump(endogenous_event_side, kernel_parameters.alpha);
            }
            if let Some(exogenous_news_severity_mark) = transition_step.exogenous_news_severity_mark
            {
                return manual_state
                    .apply_exogenous_news_loading_jump(
                        kernel_parameters.rho,
                        exogenous_news_severity_mark,
                    )
                    .map_err(|source| HawkesTransitionError::InvalidNewsSeverityMark { source });
            }
            Ok(manual_state)
        })
        .expect("manual transition sequence should succeed");

    let composed_state = state
        .apply_transition_sequence(kernel_parameters, &transition_steps)
        .expect("canonical transition sequence should succeed");

    assert_eq!(composed_state, expected_state);
}

#[test]
fn composed_transition_step_rejects_negative_drift_duration() {
    let state = HawkesExcitationState {
        y_plus_plus: 0.0,
        y_plus_minus: 0.0,
        y_minus_plus: 0.0,
        y_minus_minus: 0.0,
    };

    let kernel_parameters = HawkesKernelParameters::try_new(
        0.60,
        1.0,
        HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.0,
            minus_plus: 0.0,
            minus_minus: 0.0,
        },
        HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.0,
            minus_plus: 0.0,
            minus_minus: 0.0,
        },
    )
    .expect("kernel fixture must be valid");

    let error = state
        .apply_transition_step(
            kernel_parameters,
            HawkesTransitionStep::new(-0.01, None, None),
        )
        .expect_err("negative drift duration must fail");

    assert_eq!(error, HawkesTransitionError::InvalidDeltaT { value: -0.01 });
}

#[test]
fn event_side_payload_parsing_rejects_unsupported_value() {
    let error =
        HawkesEventSide::try_from("neutral").expect_err("unsupported side payload must fail");

    assert_eq!(
        error,
        HawkesEventSidePayloadError {
            value: "neutral".to_owned(),
        }
    );
    assert_eq!(
        error.to_string(),
        "unsupported event side 'neutral', expected one of: buy, sell"
    );
}

#[test]
fn payload_based_event_jump_returns_error_for_unsupported_side() {
    let state = HawkesExcitationState {
        y_plus_plus: 0.0,
        y_plus_minus: 0.0,
        y_minus_plus: 0.0,
        y_minus_minus: 0.0,
    };
    let alpha = HawkesKernelLoadings {
        plus_plus: 0.2,
        plus_minus: 0.3,
        minus_plus: 0.4,
        minus_minus: 0.6,
    };

    let error = state
        .apply_endogenous_event_jump_from_payload("BUY", alpha)
        .expect_err("unsupported payload must fail");
    assert_eq!(
        error,
        HawkesEventSidePayloadError {
            value: "BUY".to_owned(),
        }
    );
}

#[test]
fn loadings_from_target_components_preserve_model_ordering() {
    let target_plus = HawkesSourceComponents::new(0.1, 0.2);
    let target_minus = HawkesSourceComponents::new(0.3, 0.4);

    let loadings = HawkesKernelLoadings::from_target_components(target_plus, target_minus);

    assert_eq!(
        loadings,
        HawkesKernelLoadings {
            plus_plus: 0.1,
            plus_minus: 0.2,
            minus_plus: 0.3,
            minus_minus: 0.4,
        }
    );
    assert_eq!(loadings.target_plus_components(), target_plus);
    assert_eq!(loadings.target_minus_components(), target_minus);
    assert_eq!(loadings.as_model_ordered_array(), [0.1, 0.2, 0.3, 0.4]);
}

#[test]
fn structured_alpha_derivation_builds_expected_canonical_matrix() {
    let structured_alpha = HawkesStructuredAlphaParameterization::new(0.75, 0.25, 0.5, 0.5);

    let alpha = HawkesKernelLoadings::try_from_structured_alpha(structured_alpha)
        .expect("valid structured alpha must derive canonical loadings");

    assert_eq!(
        alpha,
        HawkesKernelLoadings {
            plus_plus: 0.75,
            plus_minus: 0.25,
            minus_plus: 0.75,
            minus_minus: 1.25,
        }
    );
    assert_eq!(alpha.as_model_ordered_array(), [0.75, 0.25, 0.75, 1.25]);
}

#[test]
fn structured_alpha_derivation_accepts_zero_delta_boundary_case() {
    let structured_alpha = HawkesStructuredAlphaParameterization::new(0.5, 0.25, 0.0, 0.0);

    let alpha = HawkesKernelLoadings::try_from_structured_alpha(structured_alpha)
        .expect("zero deltas should be valid boundary values");

    assert_eq!(
        alpha,
        HawkesKernelLoadings {
            plus_plus: 0.5,
            plus_minus: 0.25,
            minus_plus: 0.25,
            minus_minus: 0.5,
        }
    );
    assert_eq!(alpha.minus_minus, alpha.plus_plus);
    assert_eq!(alpha.minus_plus, alpha.plus_minus);
}

#[test]
fn structured_alpha_derivation_rejects_invalid_base_and_delta_values() {
    let invalid_a_self_error = HawkesKernelLoadings::try_from_structured_alpha(
        HawkesStructuredAlphaParameterization::new(0.0, 0.1, 0.1, 0.1),
    )
    .expect_err("a_self must be strictly positive");
    assert_eq!(
        invalid_a_self_error,
        HawkesStructuredAlphaParameterError::InvalidASelf { value: 0.0 }
    );

    let invalid_a_cross_error = HawkesKernelLoadings::try_from_structured_alpha(
        HawkesStructuredAlphaParameterization::new(0.2, -0.1, 0.1, 0.1),
    )
    .expect_err("a_cross must be non-negative");
    assert_eq!(
        invalid_a_cross_error,
        HawkesStructuredAlphaParameterError::InvalidACross { value: -0.1 }
    );

    let invalid_delta_self_error = HawkesKernelLoadings::try_from_structured_alpha(
        HawkesStructuredAlphaParameterization::new(0.2, 0.1, -0.001, 0.0),
    )
    .expect_err("delta_self must be non-negative");
    assert_eq!(
        invalid_delta_self_error,
        HawkesStructuredAlphaParameterError::InvalidDeltaSelf { value: -0.001 }
    );

    let invalid_delta_cross_error = HawkesKernelLoadings::try_from_structured_alpha(
        HawkesStructuredAlphaParameterization::new(0.2, 0.1, 0.0, -0.002),
    )
    .expect_err("delta_cross must be non-negative");
    assert_eq!(
        invalid_delta_cross_error,
        HawkesStructuredAlphaParameterError::InvalidDeltaCross { value: -0.002 }
    );
}

#[test]
fn structured_alpha_derivation_rejects_derived_component_overflow() {
    let error = HawkesKernelLoadings::try_from_structured_alpha(
        HawkesStructuredAlphaParameterization::new(f64::MAX, 0.1, f64::MAX, 0.2),
    )
    .expect_err("overflowed derived alpha component must fail");

    assert_eq!(
        error,
        HawkesStructuredAlphaParameterError::InvalidDerivedComponent {
            field: "alpha.minus_minus",
            value: f64::INFINITY,
        }
    );
}

#[test]
fn structured_rho_validation_builds_expected_canonical_matrix_and_ratios() {
    let structured_rho = HawkesStructuredRhoParameterization::new(1.2, 0.3, 0.4, 0.2);

    let validation_output = HawkesKernelLoadings::validate_structured_rho(structured_rho, None)
        .expect("valid structured rho must produce machine-readable validation output");

    assert_eq!(
        validation_output.derived_rho,
        HawkesKernelLoadings {
            plus_plus: 1.2,
            plus_minus: 0.3,
            minus_plus: 0.5,
            minus_minus: 1.6,
        }
    );
    assert_within_tolerance(
        validation_output.target_plus_self_to_cross_ratio,
        4.0,
        DECAY_TOLERANCE,
        "target_plus_self_to_cross_ratio",
    );
    assert_within_tolerance(
        validation_output.target_minus_self_to_cross_ratio,
        3.2,
        DECAY_TOLERANCE,
        "target_minus_self_to_cross_ratio",
    );
    assert_eq!(validation_output.applied_self_dominance_policy, None);

    let derived_rho = HawkesKernelLoadings::try_from_structured_rho(structured_rho, None)
        .expect("valid structured rho should derive canonical loadings");
    assert_eq!(derived_rho, validation_output.derived_rho);
    assert_eq!(derived_rho.as_model_ordered_array(), [1.2, 0.3, 0.5, 1.6]);
}

#[test]
fn structured_rho_validation_accepts_configurable_self_dominance_policy() {
    let structured_rho = HawkesStructuredRhoParameterization::new(2.0, 0.5, 0.4, 0.0);
    let policy = HawkesStructuredRhoSelfDominancePolicy::new(3.5);

    let validation_output =
        HawkesKernelLoadings::validate_structured_rho(structured_rho, Some(policy))
            .expect("rho ratios above threshold should satisfy policy");

    assert_eq!(
        validation_output.applied_self_dominance_policy,
        Some(policy)
    );
    assert_within_tolerance(
        validation_output.target_plus_self_to_cross_ratio,
        4.0,
        DECAY_TOLERANCE,
        "target_plus_self_to_cross_ratio",
    );
    assert_within_tolerance(
        validation_output.target_minus_self_to_cross_ratio,
        4.8,
        DECAY_TOLERANCE,
        "target_minus_self_to_cross_ratio",
    );
}

#[test]
fn structured_rho_validation_rejects_self_dominance_policy_violation() {
    let structured_rho = HawkesStructuredRhoParameterization::new(0.8, 0.5, 0.2, 0.3);
    let policy = HawkesStructuredRhoSelfDominancePolicy::new(1.3);

    let error = HawkesKernelLoadings::validate_structured_rho(structured_rho, Some(policy))
        .expect_err("ratio below configured threshold must fail");

    assert_eq!(
        error,
        HawkesStructuredRhoParameterError::SelfDominancePolicyViolation {
            minimum_self_to_cross_ratio: 1.3,
            target_plus_self_to_cross_ratio: 1.6,
            target_minus_self_to_cross_ratio: 1.25,
        }
    );
}

#[test]
fn structured_rho_derivation_rejects_invalid_base_and_delta_values() {
    let invalid_r_self_error = HawkesKernelLoadings::validate_structured_rho(
        HawkesStructuredRhoParameterization::new(0.0, 0.1, 0.1, 0.1),
        None,
    )
    .expect_err("r_self must be strictly positive");
    assert_eq!(
        invalid_r_self_error,
        HawkesStructuredRhoParameterError::InvalidRSelf { value: 0.0 }
    );

    let invalid_r_cross_error = HawkesKernelLoadings::validate_structured_rho(
        HawkesStructuredRhoParameterization::new(0.2, -0.1, 0.1, 0.1),
        None,
    )
    .expect_err("r_cross must be non-negative");
    assert_eq!(
        invalid_r_cross_error,
        HawkesStructuredRhoParameterError::InvalidRCross { value: -0.1 }
    );

    let invalid_delta_self_r_error = HawkesKernelLoadings::validate_structured_rho(
        HawkesStructuredRhoParameterization::new(0.2, 0.1, -0.001, 0.0),
        None,
    )
    .expect_err("delta_self_r must be non-negative");
    assert_eq!(
        invalid_delta_self_r_error,
        HawkesStructuredRhoParameterError::InvalidDeltaSelfR { value: -0.001 }
    );

    let invalid_delta_cross_r_error = HawkesKernelLoadings::validate_structured_rho(
        HawkesStructuredRhoParameterization::new(0.2, 0.1, 0.0, -0.002),
        None,
    )
    .expect_err("delta_cross_r must be non-negative");
    assert_eq!(
        invalid_delta_cross_r_error,
        HawkesStructuredRhoParameterError::InvalidDeltaCrossR { value: -0.002 }
    );
}

#[test]
fn structured_rho_validation_rejects_derived_component_overflow() {
    let error = HawkesKernelLoadings::validate_structured_rho(
        HawkesStructuredRhoParameterization::new(f64::MAX, 0.1, f64::MAX, 0.2),
        None,
    )
    .expect_err("overflowed derived rho component must fail");

    assert_eq!(
        error,
        HawkesStructuredRhoParameterError::InvalidDerivedComponent {
            field: "rho.minus_minus",
            value: f64::INFINITY,
        }
    );
}

#[test]
fn structured_rho_validation_rejects_invalid_self_dominance_policy_threshold() {
    let error = HawkesKernelLoadings::validate_structured_rho(
        HawkesStructuredRhoParameterization::new(0.5, 0.25, 0.1, 0.1),
        Some(HawkesStructuredRhoSelfDominancePolicy::new(0.99)),
    )
    .expect_err("minimum_self_to_cross_ratio < 1.0 must fail");

    assert_eq!(
        error,
        HawkesStructuredRhoParameterError::InvalidSelfDominanceRatioThreshold { value: 0.99 }
    );
}

#[test]
fn kernel_parameter_construction_accepts_valid_payload() {
    let alpha = HawkesKernelLoadings {
        plus_plus: 0.6,
        plus_minus: 0.1,
        minus_plus: 0.2,
        minus_minus: 0.7,
    };
    let rho = HawkesKernelLoadings {
        plus_plus: 0.0,
        plus_minus: 0.05,
        minus_plus: 0.12,
        minus_minus: 0.0,
    };

    let parameters = HawkesKernelParameters::try_new(0.85, 1.25, alpha, rho)
        .expect("valid Hawkes kernel payload should construct");

    assert_eq!(parameters.lambda_bar, 0.85);
    assert_eq!(parameters.beta, 1.25);
    assert_eq!(parameters.alpha, alpha);
    assert_eq!(parameters.rho, rho);
}

#[test]
fn kernel_parameter_domain_precheck_accepts_interior_and_boundary_adjacent_values() {
    let alpha = HawkesKernelLoadings {
        plus_plus: f64::EPSILON * 0.5,
        plus_minus: 0.0,
        minus_plus: f64::EPSILON * 0.5,
        minus_minus: f64::EPSILON * 0.5,
    };
    let rho = HawkesKernelLoadings {
        plus_plus: 0.0,
        plus_minus: 0.0,
        minus_plus: 0.25,
        minus_minus: 0.0,
    };

    let interior_report = HawkesKernelParameters::validate_domain(0.85, 1.25, alpha, rho);
    assert!(interior_report.is_valid());
    assert_eq!(
        interior_report.first_violation(),
        None,
        "valid interior payload should not report violations"
    );

    let boundary_adjacent_report =
        HawkesKernelParameters::validate_domain(f64::EPSILON, f64::EPSILON, alpha, rho);
    assert!(boundary_adjacent_report.is_valid());
    assert_eq!(
        boundary_adjacent_report.violations,
        Vec::<HawkesKernelParameterViolation>::new(),
        "strict positivity must accept tiny positive values at the boundary"
    );
}

#[test]
fn kernel_parameter_domain_precheck_returns_optimizer_friendly_report_payload_for_valid_input() {
    let alpha = HawkesKernelLoadings {
        plus_plus: 0.6,
        plus_minus: 0.1,
        minus_plus: 0.2,
        minus_minus: 0.7,
    };
    let rho = HawkesKernelLoadings {
        plus_plus: 0.0,
        plus_minus: 0.05,
        minus_plus: 0.12,
        minus_minus: 0.0,
    };

    let report = HawkesKernelParameters::validate_domain(0.85, 1.25, alpha, rho);

    assert_eq!(
        report.schema_version,
        HAWKES_KERNEL_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION
    );
    assert_eq!(
        report.determinism_policy,
        HAWKES_KERNEL_PARAMETER_VALIDATION_DETERMINISM_POLICY
    );
    assert_eq!(report.status, HawkesKernelValidationStatus::Pass);
    assert_eq!(report.summary.total_check_count, 12);
    assert_eq!(report.summary.passed_check_count, 12);
    assert_eq!(report.summary.failed_check_count, 0);
    assert_eq!(report.derived_alpha, None);
    assert_eq!(report.derived_rho, None);
    assert_eq!(report.checks.len(), report.summary.total_check_count);
    assert!(
        report
            .checks
            .iter()
            .all(|check| check.status == HawkesKernelValidationStatus::Pass)
    );
}

#[test]
fn structured_domain_validation_report_orders_multi_failure_checks_deterministically() {
    let report = HawkesKernelParameters::validate_structured_domain_with_stability_tolerance(
        0.0,
        -0.5,
        HawkesStructuredAlphaParameterization::new(0.0, -0.1, -0.2, -0.3),
        HawkesStructuredRhoParameterization::new(0.0, -0.2, -0.1, -0.4),
        Some(HawkesStructuredRhoSelfDominancePolicy::new(0.9)),
        -1.0,
    );

    assert_eq!(report.status, HawkesKernelValidationStatus::Fail);
    assert_eq!(report.derived_alpha, None);
    assert_eq!(report.derived_rho, None);
    assert_eq!(report.summary.failed_check_count, report.violations.len());
    assert_eq!(
        report
            .checks
            .iter()
            .filter(|check| check.status == HawkesKernelValidationStatus::Fail)
            .map(|check| check.check_id)
            .collect::<Vec<_>>(),
        vec![
            "positivity.lambda_bar",
            "positivity.beta",
            "positivity.structured_alpha.a_self",
            "positivity.structured_alpha.a_cross",
            "positivity.structured_alpha.delta_self",
            "positivity.structured_alpha.delta_cross",
            "positivity.alpha.plus_minus",
            "positivity.alpha.minus_plus",
            "positivity.alpha.minus_minus",
            "positivity.structured_rho.r_self",
            "positivity.structured_rho.r_cross",
            "positivity.structured_rho.delta_self_r",
            "positivity.structured_rho.delta_cross_r",
            "positivity.rho.plus_minus",
            "positivity.rho.minus_plus",
            "positivity.rho.minus_minus",
            "structural.rho.self_dominance.minimum_ratio",
            "stability.numeric_tolerance",
        ]
    );
}

#[test]
fn kernel_parameter_domain_precheck_reports_stability_violation_and_spectral_radius() {
    let alpha = HawkesKernelLoadings {
        plus_plus: 0.9,
        plus_minus: 0.2,
        minus_plus: 0.1,
        minus_minus: 0.8,
    };
    let rho = HawkesKernelLoadings {
        plus_plus: 0.0,
        plus_minus: 0.0,
        minus_plus: 0.0,
        minus_minus: 0.0,
    };

    let report = HawkesKernelParameters::validate_domain(0.85, 0.8, alpha, rho);

    assert!(!report.is_valid());
    assert_eq!(
        report.violations,
        vec![HawkesKernelParameterViolation {
            field: "alpha_over_beta.spectral_radius",
            constraint: HawkesKernelParameterConstraint::SpectralRadiusStrictlyBelowOne,
            value: 1.25,
        }]
    );
}

#[test]
fn spectral_radius_stability_checker_supports_tolerance_policy_near_threshold() {
    let alpha = HawkesKernelLoadings {
        plus_plus: 0.999_999_999_6,
        plus_minus: 0.0,
        minus_plus: 0.0,
        minus_minus: 0.2,
    };

    let strict_check = HawkesKernelParameters::check_spectral_radius_stability(1.0, alpha, 0.0)
        .expect("strict threshold check should evaluate");
    assert_eq!(
        strict_check.spectral_radius_alpha_over_beta,
        0.999_999_999_6
    );
    assert!(strict_check.is_stable);

    let tolerant_check = HawkesKernelParameters::check_spectral_radius_stability(1.0, alpha, 1e-9)
        .expect("tightened tolerance policy should evaluate");
    assert_eq!(
        tolerant_check.spectral_radius_alpha_over_beta,
        0.999_999_999_6
    );
    assert_eq!(
        tolerant_check.effective_stationarity_threshold,
        HAWKES_STRICT_STATIONARITY_THRESHOLD - 1e-9
    );
    assert!(!tolerant_check.is_stable);
}

#[test]
fn spectral_radius_stability_checker_reports_deterministic_numeric_output() {
    let alpha = HawkesKernelLoadings {
        plus_plus: 0.6,
        plus_minus: 0.1,
        minus_plus: 0.2,
        minus_minus: 0.7,
    };

    let reference_check = HawkesKernelParameters::check_spectral_radius_stability(1.25, alpha, 0.0)
        .expect("valid payload should evaluate deterministic spectral radius");
    assert!(reference_check.is_stable);
    assert_eq!(reference_check.spectral_radius_alpha_over_beta, 0.64);

    for _ in 0..16 {
        let repeated_check =
            HawkesKernelParameters::check_spectral_radius_stability(1.25, alpha, 0.0)
                .expect("repeated checks should not fail");

        assert_eq!(
            repeated_check.spectral_radius_alpha_over_beta.to_bits(),
            reference_check.spectral_radius_alpha_over_beta.to_bits()
        );
        assert_eq!(repeated_check, reference_check);
    }
}

#[test]
fn kernel_parameter_domain_precheck_reports_exact_violation_payload_structure() {
    let alpha = HawkesKernelLoadings {
        plus_plus: 0.6,
        plus_minus: -0.01,
        minus_plus: 0.2,
        minus_minus: 0.7,
    };
    let rho = HawkesKernelLoadings {
        plus_plus: 0.0,
        plus_minus: 0.05,
        minus_plus: 0.12,
        minus_minus: -0.001,
    };

    let report = HawkesKernelParameters::validate_domain(0.0, -0.5, alpha, rho);

    assert!(!report.is_valid());
    assert_eq!(
        report.violations,
        vec![
            HawkesKernelParameterViolation {
                field: "lambda_bar",
                constraint: HawkesKernelParameterConstraint::FiniteAndStrictlyPositive,
                value: 0.0,
            },
            HawkesKernelParameterViolation {
                field: "beta",
                constraint: HawkesKernelParameterConstraint::FiniteAndStrictlyPositive,
                value: -0.5,
            },
            HawkesKernelParameterViolation {
                field: "alpha.plus_minus",
                constraint: HawkesKernelParameterConstraint::FiniteAndNonNegative,
                value: -0.01,
            },
            HawkesKernelParameterViolation {
                field: "rho.minus_minus",
                constraint: HawkesKernelParameterConstraint::FiniteAndNonNegative,
                value: -0.001,
            },
        ]
    );
    assert_eq!(
        report.first_violation(),
        Some(HawkesKernelParameterViolation {
            field: "lambda_bar",
            constraint: HawkesKernelParameterConstraint::FiniteAndStrictlyPositive,
            value: 0.0,
        })
    );
}

#[test]
fn kernel_parameter_construction_rejects_non_positive_or_non_finite_lambda_bar() {
    let loadings = HawkesKernelLoadings {
        plus_plus: 0.0,
        plus_minus: 0.0,
        minus_plus: 0.0,
        minus_minus: 0.0,
    };

    let zero_lambda_bar_error = HawkesKernelParameters::try_new(0.0, 1.0, loadings, loadings)
        .expect_err("lambda_bar=0 must fail");
    assert_eq!(
        zero_lambda_bar_error,
        HawkesKernelParameterError::InvalidLambdaBar { value: 0.0 }
    );

    let nan_lambda_bar_error = HawkesKernelParameters::try_new(f64::NAN, 1.0, loadings, loadings)
        .expect_err("non-finite lambda_bar must fail");
    assert!(matches!(
        nan_lambda_bar_error,
        HawkesKernelParameterError::InvalidLambdaBar { value } if value.is_nan()
    ));
}

#[test]
fn kernel_parameter_construction_rejects_non_positive_or_non_finite_beta() {
    let loadings = HawkesKernelLoadings {
        plus_plus: 0.0,
        plus_minus: 0.0,
        minus_plus: 0.0,
        minus_minus: 0.0,
    };

    let zero_beta_error = HawkesKernelParameters::try_new(1.0, 0.0, loadings, loadings)
        .expect_err("beta=0 must fail");
    assert_eq!(
        zero_beta_error,
        HawkesKernelParameterError::InvalidBeta { value: 0.0 }
    );

    let nan_beta_error = HawkesKernelParameters::try_new(1.0, f64::NAN, loadings, loadings)
        .expect_err("non-finite beta must fail");
    assert!(matches!(
        nan_beta_error,
        HawkesKernelParameterError::InvalidBeta { value } if value.is_nan()
    ));
}

#[test]
fn kernel_parameter_construction_rejects_unstable_spectral_radius() {
    let alpha = HawkesKernelLoadings {
        plus_plus: 0.9,
        plus_minus: 0.2,
        minus_plus: 0.1,
        minus_minus: 0.8,
    };
    let rho = HawkesKernelLoadings {
        plus_plus: 0.0,
        plus_minus: 0.0,
        minus_plus: 0.0,
        minus_minus: 0.0,
    };

    let error = HawkesKernelParameters::try_new(1.0, 0.8, alpha, rho)
        .expect_err("unstable spectral radius must fail construction");

    assert_eq!(
        error,
        HawkesKernelParameterError::UnstableSpectralRadius {
            spectral_radius_alpha_over_beta: 1.25,
            strict_stationarity_threshold: 1.0,
            numeric_tolerance: 0.0,
            effective_stationarity_threshold: 1.0,
        }
    );
}

#[test]
fn kernel_parameter_construction_rejects_invalid_alpha_component() {
    let alpha = HawkesKernelLoadings {
        plus_plus: 0.2,
        plus_minus: -0.01,
        minus_plus: 0.1,
        minus_minus: 0.1,
    };
    let rho = HawkesKernelLoadings {
        plus_plus: 0.0,
        plus_minus: 0.0,
        minus_plus: 0.0,
        minus_minus: 0.0,
    };

    let error = HawkesKernelParameters::try_new(1.0, 0.9, alpha, rho)
        .expect_err("negative alpha must fail");

    assert_eq!(
        error,
        HawkesKernelParameterError::InvalidLoadingComponent {
            field: "alpha.plus_minus",
            value: -0.01,
        }
    );
}

#[test]
fn kernel_parameter_construction_rejects_invalid_rho_component() {
    let alpha = HawkesKernelLoadings {
        plus_plus: 0.2,
        plus_minus: 0.1,
        minus_plus: 0.1,
        minus_minus: 0.1,
    };
    let rho = HawkesKernelLoadings {
        plus_plus: 0.0,
        plus_minus: 0.0,
        minus_plus: 0.0,
        minus_minus: f64::INFINITY,
    };

    let error = HawkesKernelParameters::try_new(1.0, 0.9, alpha, rho)
        .expect_err("non-finite rho must fail");

    assert_eq!(
        error,
        HawkesKernelParameterError::InvalidLoadingComponent {
            field: "rho.minus_minus",
            value: f64::INFINITY,
        }
    );
}

#[test]
fn kernel_parameter_builder_from_target_components_preserves_mapping() {
    let parameters = HawkesKernelParameters::try_new_from_target_components(
        0.85,
        1.25,
        HawkesSourceComponents::new(0.6, 0.1),
        HawkesSourceComponents::new(0.2, 0.7),
        HawkesSourceComponents::new(0.0, 0.05),
        HawkesSourceComponents::new(0.12, 0.0),
    )
    .expect("typed component groups must construct canonical parameters");

    assert_eq!(parameters.lambda_bar, 0.85);
    assert_eq!(parameters.beta, 1.25);
    assert_eq!(
        parameters.alpha,
        HawkesKernelLoadings {
            plus_plus: 0.6,
            plus_minus: 0.1,
            minus_plus: 0.2,
            minus_minus: 0.7,
        }
    );
    assert_eq!(
        parameters.rho,
        HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.05,
            minus_plus: 0.12,
            minus_minus: 0.0,
        }
    );
}

#[test]
fn kernel_parameter_serialization_round_trip_preserves_canonical_payload() {
    let parameters = HawkesKernelParameters::try_new_from_target_components(
        0.85,
        1.25,
        HawkesSourceComponents::new(0.6, 0.1),
        HawkesSourceComponents::new(0.2, 0.7),
        HawkesSourceComponents::new(0.0, 0.05),
        HawkesSourceComponents::new(0.12, 0.0),
    )
    .expect("valid typed parameters must serialize");

    let serialized =
        serde_json::to_string(&parameters).expect("canonical parameter payload should serialize");
    let parsed: HawkesKernelParameters =
        serde_json::from_str(&serialized).expect("serialized payload should deserialize");

    assert_eq!(parsed, parameters);
}

#[test]
fn kernel_parameter_deserialization_reports_missing_required_field() {
    let payload = json!({
        "lambda_bar": 0.85,
        "beta": 1.25,
        "alpha": {
            "plus_plus": 0.6,
            "plus_minus": 0.1,
            "minus_plus": 0.2,
            "minus_minus": 0.7
        }
    });

    let parse_result = serde_json::from_value::<HawkesKernelParameters>(payload);
    let error_text = parse_result
        .expect_err("missing rho field must fail")
        .to_string();

    assert!(
        error_text.contains("missing field `rho`"),
        "unexpected error message: {error_text}"
    );
}

#[test]
fn kernel_parameter_deserialization_reports_malformed_field_type() {
    let payload = json!({
        "lambda_bar": 0.85,
        "beta": "fast",
        "alpha": {
            "plus_plus": 0.6,
            "plus_minus": 0.1,
            "minus_plus": 0.2,
            "minus_minus": 0.7
        },
        "rho": {
            "plus_plus": 0.0,
            "plus_minus": 0.05,
            "minus_plus": 0.12,
            "minus_minus": 0.0
        }
    });

    let parse_result = serde_json::from_value::<HawkesKernelParameters>(payload);
    let error_text = parse_result
        .expect_err("malformed beta field type must fail")
        .to_string();

    assert!(
        error_text.contains("invalid type"),
        "unexpected error message: {error_text}"
    );
}
