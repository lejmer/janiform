use janiform_affine_oracle::affine_oracle::ExponentialAffineTransformEquationSpecification;
use janiform_affine_oracle::sufficient_statistics::{
    SufficientStatisticsIntervalFactors, SufficientStatisticsModelInputs,
};
use janiform_engine_simulation::{
    COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION, CoarseSimulationModelInputs,
    CoarseSimulationRunError, CoarseSimulationRunRequest, run_coarse_simulation,
};
use janiform_market_time::calendar::{
    CalendarRuleMetadataArtifact, ExchangeCalendarArtifact, ExchangeCalendarRegistryArtifact,
    RegularSessionArtifact, SessionWindowArtifact, TradingWeekday,
};
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
};
use janiform_model_kernels::mean_reversion::{
    MeanReversionBoundaryState, MeanReversionFittedParameters, MeanReversionRuntimeIntervalAnchors,
};
use janiform_model_kernels::news::{
    CoupledGapMarkConfiguration, DoubleExponentialBaseShockConfiguration,
    GammaSeverityMarkConfiguration, KnownNewsArrivalConfiguration, NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration, NewsProcessConfiguration, SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
};
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
use serde_json::json;

#[test]
fn coarse_simulation_direct_jump_generation_is_seed_deterministic() {
    let run_request = build_run_request(
        3_001,
        1_704_204_000_000_000_000,
        1,
        1.0,
        vec![1_704_206_700_000_000_000],
        vec![1.0],
        vec![0.0],
    );

    let first_path = run_coarse_simulation(run_request.clone())
        .expect("first coarse simulation run should succeed");
    let second_path =
        run_coarse_simulation(run_request).expect("second coarse simulation run should succeed");

    assert_eq!(first_path, second_path);
    assert!(
        first_path.records[0]
            .return_components
            .open_jump_return
            .abs()
            > 0.0,
        "known open-window event should produce a non-zero direct jump component"
    );
}

#[test]
fn coarse_simulation_single_interval_runner_emits_canonical_non_hourly_path_artifact() {
    let start_time_utc_nanos = 1_704_205_800_000_000_000;
    let run_request = build_run_request(
        3_010,
        start_time_utc_nanos,
        1,
        0.5,
        vec![1_704_206_700_000_000_000],
        vec![2.0],
        vec![0.0],
    );

    let path = run_coarse_simulation(run_request).expect("single-interval coarse run should work");

    assert_eq!(path.simulated_interval_count, 1);
    assert_eq!(path.records.len(), 1);
    assert_eq!(path.start_time_utc_nanos, start_time_utc_nanos);
    assert_eq!(path.end_time_utc_nanos, 1_704_207_600_000_000_000);
    assert_eq!(path.records[0].interval_index, 0);
    assert_eq!(
        path.records[0].interval_start_utc_nanos,
        start_time_utc_nanos
    );
    assert_eq!(
        path.records[0].interval_end_utc_nanos,
        1_704_207_600_000_000_000
    );
    assert!(
        path.records[0].return_components.open_jump_return.abs() > 0.0,
        "open-window direct jump should flow through the coarse runner"
    );

    let serialized = serde_json::to_value(&path).expect("path should serialize");
    assert_eq!(
        serialized["output_schema_metadata"]["required_provenance_fields"],
        json!([
            "schema_version",
            "artifact_workflow_policy",
            "environment_metadata_policy",
            "interval_timing_policy",
            "interval_identifier_policy",
            "interval_factor_alignment_policy",
            "compatible_interval_grid_policy",
            "interval_factor_policy",
            "transform_slice_sampling_policy",
            "boundary_state_update_policy",
            "interval_return_generation_policy",
            "interval_return_component_routing_policy",
            "batch_seed_partition_policy"
        ])
    );

    path.validate()
        .expect("single-interval coarse path should satisfy the artifact contract");
}

#[test]
fn coarse_simulation_multi_interval_runner_carries_boundary_state_across_intervals() {
    let run_request = build_run_request(
        3_011,
        1_704_205_800_000_000_000,
        2,
        0.5,
        vec![1_704_206_700_000_000_000, 1_704_208_500_000_000_000],
        vec![2.0, 2.0],
        vec![0.0, 0.0],
    );

    let path = run_coarse_simulation(run_request).expect("multi-interval coarse run should work");

    assert_eq!(path.records.len(), 2);
    assert_eq!(
        path.records[0].interval_end_utc_nanos,
        path.records[1].interval_start_utc_nanos
    );
    assert_eq!(path.end_time_utc_nanos, 1_704_209_400_000_000_000);
    assert_ne!(
        path.initial_excitation_state,
        path.terminal_excitation_state
    );
    assert_eq!(path.diagnostics.simulated_interval_count, 2);
    assert_eq!(path.diagnostics.first_interval_index, 0);
    assert_eq!(path.diagnostics.last_interval_index, 1);

    path.validate()
        .expect("multi-interval coarse path should satisfy the artifact contract");
}

#[test]
fn coarse_simulation_routes_open_window_and_closed_window_news_into_separate_components() {
    let run_request = build_run_request(
        3_002,
        1_704_200_400_000_000_000,
        1,
        2.0,
        vec![1_704_202_200_000_000_000, 1_704_206_700_000_000_000],
        vec![1.0],
        vec![0.0],
    );

    let path = run_coarse_simulation(run_request).expect("coarse simulation run should succeed");
    let return_components = path.records[0].return_components;

    assert!(
        return_components.open_jump_return.abs() > 0.0,
        "open-window known news must contribute to M_t"
    );
    assert!(
        return_components.closed_gap_return.abs() > 0.0,
        "closed-window known news must contribute to G_t"
    );
}

#[test]
fn coarse_simulation_rejects_known_news_factor_misalignment_at_runner_boundary() {
    let error = run_coarse_simulation(build_run_request(
        3_012,
        1_704_205_800_000_000_000,
        1,
        0.5,
        vec![1_704_206_700_000_000_000],
        vec![0.0],
        vec![0.0],
    ))
    .expect_err("known-news intensity mismatch must fail during coarse-run orchestration");

    assert!(matches!(
        error,
        CoarseSimulationRunError::KnownNewsFactorAlignmentFailure {
            interval_index: 0,
            expected_known_nu_h: 0.0,
            derived_known_nu_h: 2.0,
        }
    ));
}

#[test]
fn coarse_simulation_surfaces_first_moment_projection_failures() {
    let model_inputs = build_model_inputs_with_components(
        build_sufficient_statistics_model_inputs(
            HawkesKernelParameters::try_new(
                0.8,
                1.0e308,
                HawkesKernelLoadings {
                    plus_plus: 0.0,
                    plus_minus: 0.0,
                    minus_plus: 0.0,
                    minus_minus: 0.0,
                },
                HawkesKernelLoadings {
                    plus_plus: 0.08,
                    plus_minus: 0.04,
                    minus_plus: 0.05,
                    minus_minus: 0.1,
                },
            )
            .expect("solver-failure kernel parameters should construct"),
            GammaSeverityMarkConfiguration::try_new(1.8, 0.7)
                .expect("solver-failure gamma configuration should construct"),
            1,
            1.0,
        ),
        build_news_process_configuration(
            vec![],
            GammaSeverityMarkConfiguration::try_new(1.5, 0.9)
                .expect("news gamma configuration should construct"),
            default_known_component_mark_scales(),
            default_surprise_component_mark_scales(),
        ),
    );
    let run_request = build_run_request_with_model_inputs(
        3_013,
        1_704_205_800_000_000_000,
        1.0,
        model_inputs,
        SufficientStatisticsIntervalFactors::try_new(vec![1.0], vec![0.0], vec![0.2], Some(1.0))
            .expect("solver-failure interval factors should construct"),
    );

    let error = run_coarse_simulation(run_request)
        .expect_err("extreme first-moment inputs must surface a runner-level projection failure");

    assert!(matches!(
        error,
        CoarseSimulationRunError::EndogenousFirstMomentProjectionFailure {
            interval_index: 0,
            ..
        }
    ));
}

#[test]
fn coarse_simulation_surfaces_boundary_state_update_failures() {
    let model_inputs = build_model_inputs_with_components(
        build_sufficient_statistics_model_inputs(
            HawkesKernelParameters::try_new(
                0.8,
                1.6,
                HawkesKernelLoadings {
                    plus_plus: 0.2,
                    plus_minus: 0.1,
                    minus_plus: 0.1,
                    minus_minus: 0.25,
                },
                HawkesKernelLoadings {
                    plus_plus: 6.0e307,
                    plus_minus: 6.0e307,
                    minus_plus: 6.0e307,
                    minus_minus: 6.0e307,
                },
            )
            .expect("boundary-failure kernel parameters should construct"),
            GammaSeverityMarkConfiguration::try_new(1.8, 1.0e-308)
                .expect("boundary-failure gamma configuration should construct"),
            1,
            1.0,
        ),
        build_news_process_configuration(
            vec![1_704_206_700_000_000_000],
            GammaSeverityMarkConfiguration::try_new(1.5, 0.9)
                .expect("news gamma configuration should construct"),
            NewsComponentMarkScaleConfiguration {
                activity_coupling_scale: 8.0,
                return_jump_scale: 1.2,
            },
            default_surprise_component_mark_scales(),
        ),
    );
    let run_request = build_run_request_with_model_inputs(
        3_014,
        1_704_205_800_000_000_000,
        1.0,
        model_inputs,
        SufficientStatisticsIntervalFactors::try_new(vec![1.0], vec![1.0], vec![0.0], Some(1.0))
            .expect("boundary-failure interval factors should construct"),
    );

    let error = run_coarse_simulation(run_request)
        .expect_err("overflowing boundary-state loadings must surface through the runner");

    assert!(
        matches!(error, CoarseSimulationRunError::InvalidPathArtifact { .. }),
        "unexpected boundary-update error: {error:?}"
    );
    assert!(
        error.to_string().contains("terminal_excitation_state"),
        "boundary update should surface through the invalid terminal-state artifact guard: {error}"
    );
}

#[test]
fn coarse_run_request_deserialization_rejects_invalid_base_shock_configuration() {
    let mut payload = serde_json::to_value(build_run_request(
        3_003,
        1_704_204_000_000_000_000,
        1,
        1.0,
        vec![1_704_206_700_000_000_000],
        vec![1.0],
        vec![0.0],
    ))
    .expect("valid run request should serialize");
    payload["model_inputs"]["news_process_configuration"]["shared_mark_law"]["double_exponential_base_shock"]
        ["probability_positive"] = json!(1.0);

    let error = serde_json::from_value::<CoarseSimulationRunRequest>(payload)
        .expect_err("invalid base shock configuration must fail deserialization");

    assert!(error.to_string().contains("probability_positive"));
}

#[test]
fn coarse_run_request_deserialization_rejects_invalid_coupled_gap_configuration() {
    let mut payload = serde_json::to_value(build_run_request(
        3_004,
        1_704_204_000_000_000_000,
        1,
        1.0,
        vec![1_704_206_700_000_000_000],
        vec![1.0],
        vec![0.0],
    ))
    .expect("valid run request should serialize");
    payload["model_inputs"]["news_process_configuration"]["shared_mark_law"]["coupled_gap_marks"]
        ["sigma_g"] = json!(-0.2);

    let error = serde_json::from_value::<CoarseSimulationRunRequest>(payload)
        .expect_err("invalid coupled gap configuration must fail deserialization");

    assert!(error.to_string().contains("sigma_g"));
}

fn build_run_request(
    seed: u64,
    start_time_utc_nanos: i64,
    horizon_interval_count: usize,
    interval_delta: f64,
    known_news_timestamps_utc_nanos: Vec<i64>,
    nu_h_known: Vec<f64>,
    nu_h_surprise: Vec<f64>,
) -> CoarseSimulationRunRequest {
    build_run_request_with_model_inputs(
        seed,
        start_time_utc_nanos,
        interval_delta,
        build_model_inputs(
            known_news_timestamps_utc_nanos,
            horizon_interval_count,
            interval_delta,
        ),
        SufficientStatisticsIntervalFactors::try_new(
            vec![1.0; horizon_interval_count],
            nu_h_known,
            nu_h_surprise,
            Some(interval_delta),
        )
        .expect("interval factors should construct"),
    )
}

fn build_run_request_with_model_inputs(
    seed: u64,
    start_time_utc_nanos: i64,
    interval_delta: f64,
    model_inputs: CoarseSimulationModelInputs,
    interval_factors: SufficientStatisticsIntervalFactors,
) -> CoarseSimulationRunRequest {
    CoarseSimulationRunRequest::try_new(
        seed,
        start_time_utc_nanos,
        interval_factors.s_h.len(),
        interval_delta,
        model_inputs,
        interval_factors,
    )
    .expect("run request should construct")
}

fn build_model_inputs(
    known_news_timestamps_utc_nanos: Vec<i64>,
    interval_count: usize,
    interval_delta: f64,
) -> CoarseSimulationModelInputs {
    build_model_inputs_with_components(
        build_sufficient_statistics_model_inputs(
            HawkesKernelParameters::try_new(
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
            .expect("kernel parameters should construct"),
            GammaSeverityMarkConfiguration::try_new(1.8, 0.7)
                .expect("gamma severity configuration should construct"),
            interval_count,
            interval_delta,
        ),
        build_news_process_configuration(
            known_news_timestamps_utc_nanos,
            GammaSeverityMarkConfiguration::try_new(1.5, 0.9)
                .expect("gamma severity configuration should construct"),
            default_known_component_mark_scales(),
            default_surprise_component_mark_scales(),
        ),
    )
}

fn build_model_inputs_with_components(
    sufficient_statistics_model_inputs: SufficientStatisticsModelInputs,
    news_process_configuration: NewsProcessConfiguration,
) -> CoarseSimulationModelInputs {
    CoarseSimulationModelInputs::try_new(
        sufficient_statistics_model_inputs,
        news_process_configuration,
        vec![1.0; 24 * 7],
        None,
        build_calendar_registry_artifact(),
        "XNYS",
    )
    .expect("coarse simulation model inputs should construct")
}

fn build_sufficient_statistics_model_inputs(
    kernel_parameters: HawkesKernelParameters,
    gamma_severity_configuration: GammaSeverityMarkConfiguration,
    interval_count: usize,
    interval_delta: f64,
) -> SufficientStatisticsModelInputs {
    SufficientStatisticsModelInputs::try_new(
        ExponentialAffineTransformEquationSpecification::try_new(
            kernel_parameters,
            gamma_severity_configuration,
        )
        .expect("equation specification should construct"),
        NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("nig configuration should construct"),
        default_initial_excitation_state(),
        MeanReversionFittedParameters::flat_anchor_mean_reversion(0.0)
            .expect("zero-gain mean reversion should construct"),
        MeanReversionRuntimeIntervalAnchors::try_new(
            "explicit_runtime_interval_grid_alignment.v1",
            0.0,
            0.0,
            vec![0.0; interval_count],
            vec![0.0; interval_count],
            vec![interval_delta; interval_count],
        )
        .expect("disabled mean-reversion runtime anchors should construct"),
        MeanReversionBoundaryState::try_new(0.0, 0.0, 0.0, 0.0)
            .expect("disabled mean-reversion boundary state should construct"),
    )
    .expect("sufficient-statistics model inputs should construct")
}

fn build_news_process_configuration(
    known_news_timestamps_utc_nanos: Vec<i64>,
    gamma_severity_configuration: GammaSeverityMarkConfiguration,
    known_component_mark_scales: NewsComponentMarkScaleConfiguration,
    surprise_component_mark_scales: NewsComponentMarkScaleConfiguration,
) -> NewsProcessConfiguration {
    NewsProcessConfiguration::try_new(
        NewsArrivalConfiguration {
            known: KnownNewsArrivalConfiguration {
                scheduled_event_timestamps_utc_nanos: known_news_timestamps_utc_nanos,
            },
            surprise: SurpriseNewsArrivalConfiguration {
                baseline_intensity: 0.0,
            },
        },
        SharedNewsMarkLawConfiguration {
            gamma_severity_marks: gamma_severity_configuration,
            double_exponential_base_shock: DoubleExponentialBaseShockConfiguration::try_new(
                0.4, 1.3, 1.1,
            )
            .expect("base shock configuration should construct"),
            coupled_gap_marks: CoupledGapMarkConfiguration::new(0.75),
        },
        known_component_mark_scales,
        surprise_component_mark_scales,
    )
    .expect("news process configuration should construct")
}

fn default_known_component_mark_scales() -> NewsComponentMarkScaleConfiguration {
    NewsComponentMarkScaleConfiguration {
        activity_coupling_scale: 0.6,
        return_jump_scale: 1.2,
    }
}

fn default_surprise_component_mark_scales() -> NewsComponentMarkScaleConfiguration {
    NewsComponentMarkScaleConfiguration {
        activity_coupling_scale: 0.5,
        return_jump_scale: 0.8,
    }
}

fn default_initial_excitation_state() -> HawkesExcitationState {
    HawkesExcitationState {
        y_plus_plus: 0.2,
        y_plus_minus: 0.1,
        y_minus_plus: 0.1,
        y_minus_minus: 0.3,
    }
}

fn build_calendar_registry_artifact() -> ExchangeCalendarRegistryArtifact {
    ExchangeCalendarRegistryArtifact {
        schema_version: "calendar.registry.v1".to_string(),
        artifact_date: "2024-01-02".to_string(),
        exchanges: vec![ExchangeCalendarArtifact {
            schema_version: "calendar.v1".to_string(),
            exchange_code: "XNYS".to_string(),
            exchange_name: "New York Stock Exchange".to_string(),
            timezone_name: Some("America/New_York".to_string()),
            rule_metadata: CalendarRuleMetadataArtifact {
                holiday_calendar_code: "US_EQUITIES".to_string(),
                early_close_rule_set: None,
            },
            regular_sessions: vec![
                RegularSessionArtifact {
                    weekday: TradingWeekday::Monday,
                    windows: vec![SessionWindowArtifact {
                        label: "regular".to_string(),
                        open_time: "09:30".to_string(),
                        close_time: "16:00".to_string(),
                    }],
                },
                RegularSessionArtifact {
                    weekday: TradingWeekday::Tuesday,
                    windows: vec![SessionWindowArtifact {
                        label: "regular".to_string(),
                        open_time: "09:30".to_string(),
                        close_time: "16:00".to_string(),
                    }],
                },
                RegularSessionArtifact {
                    weekday: TradingWeekday::Wednesday,
                    windows: vec![SessionWindowArtifact {
                        label: "regular".to_string(),
                        open_time: "09:30".to_string(),
                        close_time: "16:00".to_string(),
                    }],
                },
                RegularSessionArtifact {
                    weekday: TradingWeekday::Thursday,
                    windows: vec![SessionWindowArtifact {
                        label: "regular".to_string(),
                        open_time: "09:30".to_string(),
                        close_time: "16:00".to_string(),
                    }],
                },
                RegularSessionArtifact {
                    weekday: TradingWeekday::Friday,
                    windows: vec![SessionWindowArtifact {
                        label: "regular".to_string(),
                        open_time: "09:30".to_string(),
                        close_time: "16:00".to_string(),
                    }],
                },
            ],
            holiday_dates: vec![],
            early_close_sessions: vec![],
        }],
    }
}

#[test]
fn coarse_run_request_schema_constant_remains_stable() {
    assert_eq!(
        COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION,
        "simulation.coarse_interval.run_request.v1"
    );
}
