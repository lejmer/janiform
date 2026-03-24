use chrono::{DateTime, Utc};
use serde_json::Value;

use janiform_affine_oracle::affine_oracle::ExponentialAffineTransformEquationSpecification;
use janiform_engine_risk::{
    SufficientStatisticsRiskIntervalFactors, SufficientStatisticsRiskModelInputs,
    SufficientStatisticsRiskReturnComponents,
};
use janiform_engine_simulation::{
    CoarseSimulationModelInputs, CoarseSimulationPath, CoarseSimulationPathRecord,
    CoarseSimulationRunRequest, MeanReversionBoundaryState, MeanReversionFittedParameters,
    MeanReversionRuntimeIntervalAnchors, SeededTickEventStateSnapshotCapture,
    SeededTickSimulationDebugTraceOptions, SeededTickSimulationModelInputs,
    SeededTickSimulationRunConfig, SeededTickSimulationRunOptions, run_coarse_simulation,
    run_seeded_tick_event_simulation,
};
use janiform_market_time::calendar::{
    CalendarRuleMetadataArtifact, ExchangeCalendarArtifact, ExchangeCalendarRegistry,
    ExchangeCalendarRegistryArtifact, RegularSessionArtifact, SessionWindowArtifact,
    TradingWeekday,
};
use janiform_market_time::hourly_factors::{
    DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD, DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD,
    HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION, HOURLY_FACTOR_FALL_BACK_POLICY,
    HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY, HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
    HOURLY_FACTOR_SPRING_FORWARD_POLICY, HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
    HourlyFactorContractDiagnostics, HourlyFactorContractOutput,
    HourlyFactorMaterializationMetadata,
};
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
};
use janiform_model_kernels::news::{
    CoupledGapMarkConfiguration, DoubleExponentialBaseShockConfiguration,
    GammaSeverityMarkConfiguration, KnownNewsArrivalConfiguration, NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration, NewsProcessConfiguration, SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
};
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
use janiform_validation::{
    COARSE_SEEDED_TICK_CONSISTENCY_REPORT_SCHEMA_VERSION,
    COARSE_SEEDED_TICK_CONSISTENCY_THRESHOLDS_SCHEMA_VERSION,
    CoarseSeededTickConsistencyScenarioInput, CoarseSeededTickConsistencyThresholds,
    CoarseSeededTickDiagnosticBucket, CoarseSeededTickExpectationClass,
    CoarseSeededTickPairedPathInput, RegressionCaseStatus,
    aggregate_seeded_tick_run_to_coarse_comparable_path,
    build_coarse_seeded_tick_consistency_report,
};

const ONE_HOUR_NANOS: i64 = 3_600_000_000_000;

#[test]
fn seeded_tick_aggregation_matches_coarse_interval_boundaries_and_routing_presence() {
    let paired_path = build_paired_path(91, 1.0, 0.25, 4, 0.0);

    assert_eq!(
        paired_path.coarse_path.simulated_interval_count,
        paired_path.aggregated_fine_path.simulated_interval_count
    );
    assert_eq!(
        paired_path.aggregated_fine_path.coarse_interval_delta,
        paired_path.coarse_path.interval_delta
    );

    for (coarse_record, fine_record) in paired_path
        .coarse_path
        .records
        .iter()
        .zip(paired_path.aggregated_fine_path.records.iter())
    {
        assert_eq!(
            coarse_record.interval_start_utc_nanos,
            fine_record.interval_start_utc_nanos
        );
        assert_eq!(
            coarse_record.interval_end_utc_nanos,
            fine_record.interval_end_utc_nanos
        );
    }

    let open_presence = paired_path
        .aggregated_fine_path
        .records
        .iter()
        .map(|record| record.open_jump_return.abs() > 0.0)
        .collect::<Vec<_>>();
    let gap_presence = paired_path
        .aggregated_fine_path
        .records
        .iter()
        .map(|record| record.closed_gap_return.abs() > 0.0)
        .collect::<Vec<_>>();
    assert_eq!(open_presence, vec![true, false, false, false]);
    assert_eq!(gap_presence, vec![false, true, false, false]);

    let coarse_open_presence = paired_path
        .coarse_path
        .records
        .iter()
        .map(|record| record.return_components.open_jump_return.abs() > 0.0)
        .collect::<Vec<_>>();
    let coarse_gap_presence = paired_path
        .coarse_path
        .records
        .iter()
        .map(|record| record.return_components.closed_gap_return.abs() > 0.0)
        .collect::<Vec<_>>();
    assert_eq!(coarse_open_presence, open_presence);
    assert_eq!(coarse_gap_presence, gap_presence);
}

#[test]
fn coarse_seeded_tick_regression_report_passes_for_hourly_and_non_hourly_reference_cases() {
    let thresholds = CoarseSeededTickConsistencyThresholds {
        schema_version: COARSE_SEEDED_TICK_CONSISTENCY_THRESHOLDS_SCHEMA_VERSION,
        threshold_policy: "exact_gate_checks_plus_absolute_mean_and_wasserstein_distance_tolerances.v1",
        mean_cumulative_a_plus_absolute_tolerance: 4.0,
        mean_cumulative_a_minus_absolute_tolerance: 4.0,
        mean_cumulative_n_plus_absolute_tolerance: 6.0,
        mean_cumulative_n_minus_absolute_tolerance: 6.0,
        mean_cumulative_trading_time_return_absolute_tolerance: 3.0,
        open_jump_wasserstein_distance_tolerance: 3.0,
        closed_gap_wasserstein_distance_tolerance: 3.0,
        total_return_wasserstein_distance_tolerance: 4.0,
        terminal_state_l1_mean_tolerance: 4.0,
    };
    let seed_values = [101_u64, 103, 107, 109, 113, 127, 131, 137];
    let hourly_scenario = build_scenario_input(
        "hourly_reference",
        "hourly_grid",
        1.0,
        0.25,
        4,
        &seed_values,
    );
    let non_hourly_scenario = build_scenario_input(
        "half_hour_reference",
        "subhour_grid",
        0.5,
        0.25,
        4,
        &seed_values,
    );

    let report = build_coarse_seeded_tick_consistency_report(
        &[hourly_scenario, non_hourly_scenario],
        thresholds,
    )
    .expect("cross-scale report should build");

    assert_eq!(
        report.schema_version,
        COARSE_SEEDED_TICK_CONSISTENCY_REPORT_SCHEMA_VERSION
    );
    assert_eq!(report.summary.total_scenario_count, 2);
    assert_eq!(report.summary.failed_scenario_count, 0);
    assert_eq!(report.summary.passed_scenario_count, 2);
    assert_eq!(report.scenario_reports.len(), 2);

    let observed_grid_labels = report
        .scenario_reports
        .iter()
        .map(|scenario_report| scenario_report.interval_grid_label.as_str())
        .collect::<Vec<_>>();
    assert_eq!(observed_grid_labels, vec!["hourly_grid", "subhour_grid"]);

    for scenario_report in &report.scenario_reports {
        assert_eq!(scenario_report.status, RegressionCaseStatus::Pass);
        assert_eq!(scenario_report.deterministic_failed_check_count, 0);
        assert_eq!(scenario_report.statistical_failed_check_count, 0);

        let exact_check_count = scenario_report
            .deterministic_checks
            .iter()
            .filter(|check| check.expectation_class == CoarseSeededTickExpectationClass::Exact)
            .count();
        assert_eq!(exact_check_count, 3);

        let distributional_buckets = scenario_report
            .statistical_checks
            .iter()
            .filter(|check| {
                check.expectation_class == CoarseSeededTickExpectationClass::Distributional
                    && check.diagnostic_bucket
                        == CoarseSeededTickDiagnosticBucket::DirectJumpGeneration
            })
            .count();
        assert_eq!(distributional_buckets, 2);
    }
}

#[test]
fn coarse_seeded_tick_regression_report_retains_statistical_agreement_with_surprise_intensity() {
    let thresholds = CoarseSeededTickConsistencyThresholds {
        schema_version: COARSE_SEEDED_TICK_CONSISTENCY_THRESHOLDS_SCHEMA_VERSION,
        threshold_policy: "exact_gate_checks_plus_absolute_mean_and_wasserstein_distance_tolerances.v1",
        mean_cumulative_a_plus_absolute_tolerance: 4.0,
        mean_cumulative_a_minus_absolute_tolerance: 4.0,
        mean_cumulative_n_plus_absolute_tolerance: 6.0,
        mean_cumulative_n_minus_absolute_tolerance: 6.0,
        mean_cumulative_trading_time_return_absolute_tolerance: 3.0,
        open_jump_wasserstein_distance_tolerance: 3.0,
        closed_gap_wasserstein_distance_tolerance: 3.0,
        total_return_wasserstein_distance_tolerance: 4.0,
        terminal_state_l1_mean_tolerance: 4.0,
    };
    let paired_paths = [151_u64, 157, 163, 167, 173, 179]
        .into_iter()
        .map(|seed| build_paired_path(seed, 1.0, 0.25, 4, 0.25))
        .collect::<Vec<_>>();

    let report = build_coarse_seeded_tick_consistency_report(
        &[CoarseSeededTickConsistencyScenarioInput {
            scenario_id: "hourly_surprise_reference".to_string(),
            interval_grid_label: "hourly_grid".to_string(),
            paired_paths,
        }],
        thresholds,
    )
    .expect("cross-scale report should build for surprise-intensity scenarios");

    assert_eq!(report.summary.total_scenario_count, 1, "{report:#?}");
    assert_eq!(
        report.summary.statistical_failed_check_count, 0,
        "{report:#?}"
    );
    assert_eq!(
        report.scenario_reports[0].statistical_failed_check_count, 0,
        "{report:#?}"
    );
    assert!(
        report.scenario_reports[0].deterministic_failed_check_count <= 1,
        "{report:#?}"
    );
}

#[test]
fn coarse_fifteen_minute_returns_do_not_exhibit_surrogate_sampling_dispersion_blowup() {
    let paired_paths = [
        211_u64, 223, 227, 229, 233, 239, 241, 251, 257, 263, 269, 271, 277, 281, 283, 293,
    ]
    .into_iter()
    .map(|seed| build_paired_path(seed, 0.25, 1.0 / 12.0, 96, 0.05))
    .collect::<Vec<_>>();

    let coarse_returns = paired_paths
        .iter()
        .flat_map(|paired_path| {
            paired_path
                .coarse_path
                .records
                .iter()
                .map(|record| record.return_components.total_return)
        })
        .collect::<Vec<_>>();
    let aggregated_fine_returns = paired_paths
        .iter()
        .flat_map(|paired_path| {
            paired_path
                .aggregated_fine_path
                .records
                .iter()
                .map(|record| record.total_return)
        })
        .collect::<Vec<_>>();

    let coarse_standard_deviation = sample_standard_deviation(&coarse_returns);
    let aggregated_fine_standard_deviation = sample_standard_deviation(&aggregated_fine_returns);
    let standard_deviation_ratio = coarse_standard_deviation / aggregated_fine_standard_deviation;

    assert!(
        standard_deviation_ratio <= 2.0,
        "direct coarse 15m returns should stay within a reasonable dispersion band of aggregated seeded ticks, coarse_std={coarse_standard_deviation}, aggregated_fine_std={aggregated_fine_standard_deviation}, ratio={standard_deviation_ratio}"
    );
}

#[test]
fn coarse_seeded_tick_regression_report_is_machine_readable_stable_and_diagnostic() {
    let thresholds = CoarseSeededTickConsistencyThresholds::default();
    let paired_path = build_paired_path(211, 1.0, 0.25, 4, 0.0);
    let drifted_coarse_path = with_open_jump_override(&paired_path.coarse_path, 0, 0.0);
    let failing_report = build_coarse_seeded_tick_consistency_report(
        &[CoarseSeededTickConsistencyScenarioInput {
            scenario_id: "direct_jump_drift".to_string(),
            interval_grid_label: "hourly_grid".to_string(),
            paired_paths: vec![CoarseSeededTickPairedPathInput {
                coarse_path: drifted_coarse_path,
                aggregated_fine_path: paired_path.aggregated_fine_path.clone(),
            }],
        }],
        thresholds,
    )
    .expect("failing report should build");
    let repeated_report = build_coarse_seeded_tick_consistency_report(
        &[CoarseSeededTickConsistencyScenarioInput {
            scenario_id: "direct_jump_drift".to_string(),
            interval_grid_label: "hourly_grid".to_string(),
            paired_paths: vec![CoarseSeededTickPairedPathInput {
                coarse_path: with_open_jump_override(&paired_path.coarse_path, 0, 0.0),
                aggregated_fine_path: paired_path.aggregated_fine_path,
            }],
        }],
        thresholds,
    )
    .expect("repeated failing report should build");

    assert_eq!(failing_report, repeated_report);
    assert_eq!(failing_report.summary.failed_scenario_count, 1);
    assert_eq!(
        failing_report.scenario_reports[0].status,
        RegressionCaseStatus::Fail
    );
    assert!(
        failing_report.scenario_reports[0]
            .deterministic_checks
            .iter()
            .any(|check| {
                check.check_id == "component_routing_presence_alignment"
                    && !check.passed
                    && check.diagnostic_bucket
                        == CoarseSeededTickDiagnosticBucket::DirectJumpGeneration
            })
    );

    let serialized = serde_json::to_value(&failing_report).expect("report should serialize");
    let serialized_object = serialized
        .as_object()
        .expect("report should serialize as an object");
    for required_field in &failing_report.output_schema_metadata.required_report_fields {
        assert!(serialized_object.contains_key(*required_field));
    }

    let first_scenario = serialized
        .get("scenario_reports")
        .and_then(Value::as_array)
        .and_then(|scenario_reports| scenario_reports.first())
        .and_then(Value::as_object)
        .expect("first scenario report should serialize as an object");
    for required_field in &failing_report
        .output_schema_metadata
        .required_scenario_fields
    {
        assert!(first_scenario.contains_key(*required_field));
    }
}

fn build_scenario_input(
    scenario_id: &str,
    interval_grid_label: &str,
    coarse_interval_delta: f64,
    fine_interval_delta: f64,
    coarse_interval_count: usize,
    seed_values: &[u64],
) -> CoarseSeededTickConsistencyScenarioInput {
    CoarseSeededTickConsistencyScenarioInput {
        scenario_id: scenario_id.to_string(),
        interval_grid_label: interval_grid_label.to_string(),
        paired_paths: seed_values
            .iter()
            .map(|seed| {
                build_paired_path(
                    *seed,
                    coarse_interval_delta,
                    fine_interval_delta,
                    coarse_interval_count,
                    0.0,
                )
            })
            .collect(),
    }
}

fn build_paired_path(
    seed: u64,
    coarse_interval_delta: f64,
    fine_interval_delta: f64,
    coarse_interval_count: usize,
    surprise_intensity: f64,
) -> CoarseSeededTickPairedPathInput {
    let start_time_utc_nanos = timestamp_utc_nanos("2024-01-22T20:00:00Z");
    let known_news_timestamps_utc_nanos = vec![
        timestamp_utc_nanos("2024-01-22T20:15:00Z"),
        timestamp_utc_nanos("2024-01-22T21:15:00Z"),
    ];
    let interval_fixture = build_interval_fixture(
        start_time_utc_nanos,
        coarse_interval_delta,
        fine_interval_delta,
        coarse_interval_count,
        surprise_intensity,
        &known_news_timestamps_utc_nanos,
    );
    let coarse_run_request = CoarseSimulationRunRequest::try_new(
        seed,
        start_time_utc_nanos,
        coarse_interval_count,
        coarse_interval_delta,
        build_coarse_model_inputs(known_news_timestamps_utc_nanos.clone()),
        interval_fixture.coarse_interval_factors,
    )
    .expect("coarse run request should construct");
    let coarse_path =
        run_coarse_simulation(coarse_run_request).expect("coarse simulation should run");

    let seeded_run_config = SeededTickSimulationRunConfig::try_new(
        seed,
        DateTime::<Utc>::from_timestamp_nanos(start_time_utc_nanos),
        interval_fixture.fine_interval_count,
        build_seeded_model_inputs(
            interval_fixture.hourly_factor_contract,
            known_news_timestamps_utc_nanos,
        ),
        Some(SeededTickSimulationRunOptions {
            debug_trace_enabled: true,
            debug_trace_options: SeededTickSimulationDebugTraceOptions {
                sample_every_nth_transition: 1,
                max_records_per_chunk: 256,
            },
            state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
        }),
    )
    .expect("seeded run config should construct");
    let seeded_run_result = run_seeded_tick_event_simulation(seeded_run_config.clone())
        .expect("seeded run should complete");
    let aggregated_fine_path = aggregate_seeded_tick_run_to_coarse_comparable_path(
        &seeded_run_config,
        &seeded_run_result,
        coarse_interval_delta,
    )
    .expect("fine run should aggregate onto the coarse grid");

    CoarseSeededTickPairedPathInput {
        coarse_path,
        aggregated_fine_path,
    }
}

struct IntervalFixture {
    fine_interval_count: usize,
    hourly_factor_contract: HourlyFactorContractOutput,
    coarse_interval_factors: SufficientStatisticsRiskIntervalFactors,
}

fn build_interval_fixture(
    start_time_utc_nanos: i64,
    coarse_interval_delta: f64,
    fine_interval_delta: f64,
    coarse_interval_count: usize,
    surprise_intensity: f64,
    known_news_timestamps_utc_nanos: &[i64],
) -> IntervalFixture {
    let fine_subinterval_count_per_coarse_interval =
        (coarse_interval_delta / fine_interval_delta).round() as usize;
    let fine_interval_count = coarse_interval_count * fine_subinterval_count_per_coarse_interval;
    let fine_interval_delta_nanos = interval_delta_to_nanos(fine_interval_delta);
    let coarse_interval_delta_nanos = interval_delta_to_nanos(coarse_interval_delta);

    let fine_interval_start_utc_nanos = (0..fine_interval_count)
        .map(|interval_index| {
            start_time_utc_nanos + (interval_index as i64).saturating_mul(fine_interval_delta_nanos)
        })
        .collect::<Vec<_>>();
    let fine_interval_end_utc_nanos = fine_interval_start_utc_nanos
        .iter()
        .map(|interval_start| interval_start + fine_interval_delta_nanos)
        .collect::<Vec<_>>();
    let combined_fine_nu_h = fine_interval_start_utc_nanos
        .iter()
        .zip(fine_interval_end_utc_nanos.iter())
        .map(|(interval_start, interval_end)| {
            surprise_intensity
                + count_known_news_events(
                    known_news_timestamps_utc_nanos,
                    *interval_start,
                    *interval_end,
                ) as f64
                    / fine_interval_delta
        })
        .collect::<Vec<_>>();
    let coarse_nu_known = (0..coarse_interval_count)
        .map(|interval_index| {
            let interval_start = start_time_utc_nanos
                + (interval_index as i64).saturating_mul(coarse_interval_delta_nanos);
            let interval_end = interval_start + coarse_interval_delta_nanos;
            count_known_news_events(
                known_news_timestamps_utc_nanos,
                interval_start,
                interval_end,
            ) as f64
                / coarse_interval_delta
        })
        .collect::<Vec<_>>();

    IntervalFixture {
        fine_interval_count,
        hourly_factor_contract: HourlyFactorContractOutput {
            schema_version: HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
            exchange_code: "XNYS".to_string(),
            exchange_timezone_name: "America/New_York".to_string(),
            request_start_utc_nanos: start_time_utc_nanos,
            request_end_utc_nanos: fine_interval_end_utc_nanos[fine_interval_count - 1],
            interval_duration_nanos: fine_interval_delta_nanos,
            interval_indices: (0..fine_interval_count)
                .map(|interval_index| interval_index as u64)
                .collect(),
            interval_start_utc_nanos: fine_interval_start_utc_nanos.clone(),
            interval_end_utc_nanos: fine_interval_end_utc_nanos.clone(),
            interval_start_exchange_local: fine_interval_start_utc_nanos
                .iter()
                .map(|timestamp_utc_nanos| {
                    DateTime::<Utc>::from_timestamp_nanos(*timestamp_utc_nanos).to_rfc3339()
                })
                .collect(),
            interval_end_exchange_local: fine_interval_end_utc_nanos
                .iter()
                .map(|timestamp_utc_nanos| {
                    DateTime::<Utc>::from_timestamp_nanos(*timestamp_utc_nanos).to_rfc3339()
                })
                .collect(),
            s_h: vec![1.0; fine_interval_count],
            nu_h: combined_fine_nu_h,
            metadata: HourlyFactorMaterializationMetadata {
                seasonality_materialization_method: DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD
                    .to_string(),
                news_intensity_materialization_method:
                    DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD.to_string(),
                interval_boundary_policy: HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
                interval_alignment_policy: HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY,
                timezone_transition_policy: HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
                spring_forward_policy: HOURLY_FACTOR_SPRING_FORWARD_POLICY,
                fall_back_policy: HOURLY_FACTOR_FALL_BACK_POLICY,
            },
            diagnostics: HourlyFactorContractDiagnostics {
                interval_count: fine_interval_count,
                interval_duration_nanos: fine_interval_delta_nanos,
                first_interval_start_utc_nanos: fine_interval_start_utc_nanos[0],
                last_interval_end_utc_nanos: fine_interval_end_utc_nanos[fine_interval_count - 1],
                skipped_local_hour_interval_count: 0,
                repeated_local_hour_interval_count: 0,
            },
        },
        coarse_interval_factors: SufficientStatisticsRiskIntervalFactors::try_new(
            vec![1.0; coarse_interval_count],
            coarse_nu_known,
            vec![surprise_intensity; coarse_interval_count],
            Some(coarse_interval_delta),
        )
        .expect("coarse interval factors should construct"),
    }
}

fn build_coarse_model_inputs(
    known_news_timestamps_utc_nanos: Vec<i64>,
) -> CoarseSimulationModelInputs {
    CoarseSimulationModelInputs::try_new(
        build_risk_model_inputs(),
        build_news_process_configuration(known_news_timestamps_utc_nanos),
        build_calendar_registry_artifact(),
        "XNYS",
    )
    .expect("coarse model inputs should construct")
}

fn build_seeded_model_inputs(
    hourly_factor_contract: HourlyFactorContractOutput,
    known_news_timestamps_utc_nanos: Vec<i64>,
) -> SeededTickSimulationModelInputs {
    let (
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
    ) = disabled_mean_reversion_runtime_inputs(&hourly_factor_contract);

    SeededTickSimulationModelInputs {
        kernel_parameters: build_kernel_parameters(),
        initial_excitation_state: default_initial_excitation_state(),
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
        hourly_factor_contract,
        calendar_registry: ExchangeCalendarRegistry::from_artifact(
            build_calendar_registry_artifact(),
        )
        .expect("seeded calendar registry should load"),
        nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration::try_new(
            2.3, -0.8, 0.7, 0.1, -0.2,
        )
        .expect("nig configuration should construct"),
        news_process_configuration: build_news_process_configuration(
            known_news_timestamps_utc_nanos,
        ),
    }
}

fn disabled_mean_reversion_runtime_inputs(
    hourly_factor_contract: &HourlyFactorContractOutput,
) -> (
    MeanReversionFittedParameters,
    MeanReversionRuntimeIntervalAnchors,
    MeanReversionBoundaryState,
) {
    (
        MeanReversionFittedParameters::disabled(),
        MeanReversionRuntimeIntervalAnchors::try_new(
            hourly_factor_contract.metadata.interval_alignment_policy,
            0.0,
            vec![0.0; hourly_factor_contract.interval_indices.len()],
            vec![
                hourly_factor_contract.interval_duration_nanos as f64 / ONE_HOUR_NANOS as f64;
                hourly_factor_contract.interval_indices.len()
            ],
        )
        .expect("disabled mean-reversion runtime anchors should construct"),
        MeanReversionBoundaryState::try_new(0.0, 0.0, 0.0)
            .expect("disabled mean-reversion boundary state should construct"),
    )
}

fn build_risk_model_inputs() -> SufficientStatisticsRiskModelInputs {
    SufficientStatisticsRiskModelInputs::try_new(
        ExponentialAffineTransformEquationSpecification::try_new(
            build_kernel_parameters(),
            GammaSeverityMarkConfiguration::try_new(1.8, 0.7)
                .expect("risk gamma severity configuration should construct"),
        )
        .expect("equation specification should construct"),
        NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("nig configuration should construct"),
        default_initial_excitation_state(),
    )
    .expect("risk model inputs should construct")
}

fn build_news_process_configuration(
    known_news_timestamps_utc_nanos: Vec<i64>,
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
            gamma_severity_marks: GammaSeverityMarkConfiguration::try_new(1.5, 0.9)
                .expect("news gamma severity configuration should construct"),
            double_exponential_base_shock: DoubleExponentialBaseShockConfiguration::try_new(
                0.4, 1.3, 1.1,
            )
            .expect("news base shock configuration should construct"),
            coupled_gap_marks: CoupledGapMarkConfiguration::new(0.75),
        },
        default_news_component_mark_scales(),
        default_news_component_mark_scales(),
    )
    .expect("news process configuration should construct")
}

fn build_kernel_parameters() -> HawkesKernelParameters {
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
    .expect("kernel parameters should construct")
}

fn default_initial_excitation_state() -> HawkesExcitationState {
    HawkesExcitationState {
        y_plus_plus: 0.2,
        y_plus_minus: 0.1,
        y_minus_plus: 0.1,
        y_minus_minus: 0.3,
    }
}

fn default_news_component_mark_scales() -> NewsComponentMarkScaleConfiguration {
    NewsComponentMarkScaleConfiguration {
        activity_coupling_scale: 0.6,
        return_jump_scale: 1.2,
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

fn count_known_news_events(
    known_news_timestamps_utc_nanos: &[i64],
    interval_start_utc_nanos: i64,
    interval_end_utc_nanos: i64,
) -> usize {
    known_news_timestamps_utc_nanos
        .iter()
        .filter(|timestamp_utc_nanos| {
            **timestamp_utc_nanos >= interval_start_utc_nanos
                && **timestamp_utc_nanos < interval_end_utc_nanos
        })
        .count()
}

fn interval_delta_to_nanos(interval_delta: f64) -> i64 {
    (interval_delta * ONE_HOUR_NANOS as f64).round() as i64
}

fn sample_standard_deviation(values: &[f64]) -> f64 {
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance = values
        .iter()
        .map(|value| {
            let centered = value - mean;
            centered * centered
        })
        .sum::<f64>()
        / values.len() as f64;

    variance.sqrt()
}

fn timestamp_utc_nanos(raw_timestamp_utc: &str) -> i64 {
    DateTime::parse_from_rfc3339(raw_timestamp_utc)
        .expect("fixture timestamp must be RFC3339")
        .with_timezone(&Utc)
        .timestamp_nanos_opt()
        .expect("fixture timestamp must be in nanosecond range")
}

fn with_open_jump_override(
    coarse_path: &CoarseSimulationPath,
    interval_index: usize,
    open_jump_return: f64,
) -> CoarseSimulationPath {
    let mut records = coarse_path.records.clone();
    let original_record = &records[interval_index];
    let original_components = original_record.return_components;
    let updated_total_return = original_components.trading_time_return
        + open_jump_return
        + original_components.closed_gap_return;
    let updated_return_components = SufficientStatisticsRiskReturnComponents::try_new(
        original_components.buy_side_return,
        original_components.sell_side_return,
        original_components.trading_time_return,
        open_jump_return,
        original_components.closed_gap_return,
        updated_total_return,
    )
    .expect("updated return components should validate");
    records[interval_index] = CoarseSimulationPathRecord::try_new(
        original_record.interval_index,
        original_record.interval_start_utc_nanos,
        original_record.interval_end_utc_nanos,
        original_record.sufficient_statistics,
        updated_return_components,
    )
    .expect("updated coarse path record should validate");

    CoarseSimulationPath::try_new(
        coarse_path.seed,
        coarse_path.start_time_utc_nanos,
        coarse_path.interval_delta,
        coarse_path.simulated_interval_count,
        coarse_path.initial_excitation_state,
        coarse_path.terminal_excitation_state,
        records,
    )
    .expect("updated coarse path should validate")
}
