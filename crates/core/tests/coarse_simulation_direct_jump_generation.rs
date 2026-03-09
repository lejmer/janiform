use janiform_core::affine_oracle::ExponentialAffineTransformEquationSpecification;
use janiform_core::calendar::{
    CalendarRuleMetadataArtifact, ExchangeCalendarArtifact, ExchangeCalendarRegistryArtifact,
    RegularSessionArtifact, SessionWindowArtifact, TradingWeekday,
};
use janiform_core::hawkes::{HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters};
use janiform_core::news::{
    CoupledGapMarkConfiguration, DoubleExponentialBaseShockConfiguration,
    GammaSeverityMarkConfiguration, KnownNewsArrivalConfiguration, NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration, NewsProcessConfiguration, SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
};
use janiform_core::nig::NigSharedPlusShiftConfiguration;
use janiform_core::risk::{
    SufficientStatisticsRiskIntervalFactors, SufficientStatisticsRiskModelInputs,
};
use janiform_core::simulation::{
    COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION, CoarseSimulationModelInputs,
    CoarseSimulationRunRequest, run_coarse_simulation,
};
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
    CoarseSimulationRunRequest::try_new(
        seed,
        start_time_utc_nanos,
        horizon_interval_count,
        interval_delta,
        build_model_inputs(known_news_timestamps_utc_nanos),
        SufficientStatisticsRiskIntervalFactors::try_new(
            vec![1.0; horizon_interval_count],
            nu_h_known,
            nu_h_surprise,
            Some(interval_delta),
        )
        .expect("interval factors should construct"),
    )
    .expect("run request should construct")
}

fn build_model_inputs(known_news_timestamps_utc_nanos: Vec<i64>) -> CoarseSimulationModelInputs {
    let risk_model_inputs = SufficientStatisticsRiskModelInputs::try_new(
        ExponentialAffineTransformEquationSpecification::try_new(
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
        )
        .expect("equation specification should construct"),
        NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("nig configuration should construct"),
        HawkesExcitationState {
            y_plus_plus: 0.2,
            y_plus_minus: 0.1,
            y_minus_plus: 0.1,
            y_minus_minus: 0.3,
        },
    )
    .expect("risk model inputs should construct");

    CoarseSimulationModelInputs::try_new(
        risk_model_inputs,
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
                gamma_severity_marks: GammaSeverityMarkConfiguration::try_new(1.5, 0.9)
                    .expect("gamma severity configuration should construct"),
                double_exponential_base_shock: DoubleExponentialBaseShockConfiguration::try_new(
                    0.4, 1.3, 1.1,
                )
                .expect("base shock configuration should construct"),
                coupled_gap_marks: CoupledGapMarkConfiguration::new(0.75),
            },
            NewsComponentMarkScaleConfiguration {
                activity_coupling_scale: 0.6,
                return_jump_scale: 1.2,
            },
            NewsComponentMarkScaleConfiguration {
                activity_coupling_scale: 0.5,
                return_jump_scale: 0.8,
            },
        )
        .expect("news process configuration should construct"),
        build_calendar_registry_artifact(),
        "XNYS",
    )
    .expect("coarse simulation model inputs should construct")
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
