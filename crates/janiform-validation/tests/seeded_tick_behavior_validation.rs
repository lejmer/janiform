mod support;

use chrono::{DateTime, Utc};
use janiform_engine_simulation::{
    MeanReversionBoundaryState, MeanReversionFittedParameters, MeanReversionRuntimeIntervalAnchors,
    SEEDED_TICK_EVENT_ORDERING_POLICY, SeededTickEventDebugTraceRecord,
    SeededTickEventDebugTraceTransitionKind, SeededTickEventRecord, SeededTickEventSide,
    SeededTickSimulationDebugTraceOptions, SeededTickSimulationModelInputs,
    SeededTickSimulationRunConfig, SeededTickSimulationRunOptions, SeededTickSimulationRunResult,
    run_seeded_tick_event_simulation, run_seeded_tick_event_simulation_with_randomness_factory,
};
use janiform_foundations::randomness::{
    SimulationRandomnessFactory, SimulationRandomnessStream, SimulationRandomnessSubstreamDomain,
};
use janiform_foundations::time::ONE_HOUR_NANOS;
use janiform_market_time::calendar::{ExchangeCalendarRegistry, ExchangeCalendarRegistryLoader};
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
use support::hawkes_invariant_catalog::{
    HawkesInvariantCheckClass, assert_hawkes_invariant_catalog_ids_have_check_class,
};

const START_TIME_UTC_NANOS: i64 = 1_704_067_200_000_000_000;
const FIFTEEN_MINUTE_NANOS: i64 = 900_000_000_000;
const ONE_DAY_NANOS: i64 = 86_400_000_000_000;
const FLOAT_TOLERANCE: f64 = 1e-12;
const OPEN_MARKET_START_TIME_UTC_NANOS: i64 = 1_705_935_600_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq)]
struct ClusteringMetrics {
    mean_fano_factor: f64,
    mean_lag_one_autocorrelation: f64,
    non_grid_run_ratio: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ClusteringThresholds {
    minimum_fano_factor_gap: f64,
    minimum_lag_one_autocorrelation_gap: f64,
    minimum_cluster_lag_one_autocorrelation: f64,
    minimum_non_grid_run_ratio: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct AsymmetryMetrics {
    mean_sell_share: f64,
    mean_log_sell_to_buy_ratio: f64,
    mean_signed_sell_minus_buy_per_interval: f64,
    mean_buy_fano_factor: f64,
    mean_sell_fano_factor: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct AsymmetryThresholds {
    symmetric_sell_share_tolerance: f64,
    symmetric_signed_drift_tolerance: f64,
    symmetric_side_fano_factor_gap_tolerance: f64,
    minimum_sell_share_gap: f64,
    minimum_sell_to_buy_log_ratio_gap: f64,
    minimum_sell_minus_buy_signed_drift_gap: f64,
    minimum_sell_side_fano_factor_gap: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AsymmetryExpectedQualitativeBehavior {
    NearSymmetricControl,
    SellSideStress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AsymmetryScenarioMetadata {
    scenario_id: &'static str,
    regime_intent: &'static str,
    expected_qualitative_behavior: AsymmetryExpectedQualitativeBehavior,
}

#[derive(Debug, Clone, PartialEq)]
struct AsymmetryRegressionScenario {
    metadata: AsymmetryScenarioMetadata,
    kernel_parameters: HawkesKernelParameters,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct HorizonSummary {
    mean_total_event_count: f64,
    mean_buy_share: f64,
    mean_sell_share: f64,
    mean_log_sell_to_buy_ratio: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ScaleConsistencyMetrics {
    aggregated_summary: HorizonSummary,
    direct_summary: HorizonSummary,
    relative_total_event_count_delta: f64,
    absolute_buy_share_delta: f64,
    absolute_sell_share_delta: f64,
    absolute_log_sell_to_buy_ratio_delta: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ScaleConsistencyThresholds {
    maximum_relative_total_event_count_delta: f64,
    maximum_absolute_buy_share_delta: f64,
    maximum_absolute_sell_share_delta: f64,
    maximum_absolute_log_sell_to_buy_ratio_delta: f64,
}

#[test]
fn seeded_behavior_validation_is_strictly_deterministic_for_repeated_seeded_runs() {
    assert_hawkes_invariant_catalog_ids_have_check_class(
        &["HK-BND-002"],
        HawkesInvariantCheckClass::DeterministicEquation,
    );

    let run_config = build_run_config(
        91_006,
        FIFTEEN_MINUTE_NANOS,
        96,
        symmetric_kernel_parameters(),
        HawkesExcitationState {
            y_plus_plus: 0.0,
            y_plus_minus: 0.0,
            y_minus_plus: 0.0,
            y_minus_minus: 0.0,
        },
        1.0,
        0.01,
    );

    let first_run_result = run_seeded_tick_event_simulation(run_config.clone())
        .expect("first seeded run should complete");
    let second_run_result =
        run_seeded_tick_event_simulation(run_config).expect("second seeded run should complete");
    let first_event_path = first_run_result.projected_event_path();
    let second_event_path = second_run_result.projected_event_path();

    assert_eq!(
        first_event_path, second_event_path,
        "determinism failure for repeated seeded runs"
    );
    assert_eq!(
        first_run_result.final_excitation_state, second_run_result.final_excitation_state,
        "determinism failure for repeated seeded final states"
    );
}

#[test]
fn seeded_behavior_validation_preserves_full_4d_state_carryover_across_multi_hour_boundaries() {
    assert_hawkes_invariant_catalog_ids_have_check_class(
        &["HK-BND-003"],
        HawkesInvariantCheckClass::DeterministicEquation,
    );

    let run_config = SeededTickSimulationRunConfig::try_new(
        91_007,
        DateTime::<Utc>::from_timestamp_nanos(START_TIME_UTC_NANOS),
        3,
        build_multi_hour_zero_event_carryover_model_inputs(3),
        Some(SeededTickSimulationRunOptions {
            debug_trace_enabled: true,
            debug_trace_options: SeededTickSimulationDebugTraceOptions {
                sample_every_nth_transition: 4,
                max_records_per_chunk: 16,
            },
            ..SeededTickSimulationRunOptions::default()
        }),
    )
    .expect("multi-hour carryover run config should initialize");

    let run_result = run_seeded_tick_event_simulation(run_config)
        .expect("multi-hour carryover run should complete");
    assert_eq!(run_result.event_count(), 0);

    let trace_records = collect_debug_trace_records(&run_result);
    let boundary_records: Vec<&SeededTickEventDebugTraceRecord> = trace_records
        .iter()
        .copied()
        .filter(|trace_record| {
            trace_record.transition_kind
                == SeededTickEventDebugTraceTransitionKind::IntervalBoundaryDrift
        })
        .collect();
    assert_eq!(boundary_records.len(), 3);

    for boundary_record_window in boundary_records.windows(2) {
        let preceding_boundary_record = boundary_record_window[0];
        let following_boundary_record = boundary_record_window[1];

        assert_eq!(
            preceding_boundary_record.interval_index + 1,
            following_boundary_record.interval_index
        );
        assert_float_close(
            preceding_boundary_record.state_after_transition.y_plus_plus,
            following_boundary_record
                .state_before_transition
                .y_plus_plus,
        );
        assert_float_close(
            preceding_boundary_record
                .state_after_transition
                .y_plus_minus,
            following_boundary_record
                .state_before_transition
                .y_plus_minus,
        );
        assert_float_close(
            preceding_boundary_record
                .state_after_transition
                .y_minus_plus,
            following_boundary_record
                .state_before_transition
                .y_minus_plus,
        );
        assert_float_close(
            preceding_boundary_record
                .state_after_transition
                .y_minus_minus,
            following_boundary_record
                .state_before_transition
                .y_minus_minus,
        );
    }
}

#[test]
fn seeded_behavior_validation_boundary_adjacent_scenarios_are_repeatable_with_fixed_seed() {
    assert_hawkes_invariant_catalog_ids_have_check_class(
        &["HK-BND-003"],
        HawkesInvariantCheckClass::DeterministicEquation,
    );

    let run_config = SeededTickSimulationRunConfig::try_new(
        91_008,
        DateTime::<Utc>::from_timestamp_nanos(OPEN_MARKET_START_TIME_UTC_NANOS),
        2,
        build_two_interval_boundary_adjacent_model_inputs(OPEN_MARKET_START_TIME_UTC_NANOS),
        None,
    )
    .expect("boundary-adjacent run config should initialize");

    let first_run_result = run_seeded_tick_event_simulation_with_randomness_factory(
        run_config.clone(),
        &BoundaryAdjacentRandomnessFactory,
    )
    .expect("first boundary-adjacent run should complete");
    let second_run_result = run_seeded_tick_event_simulation_with_randomness_factory(
        run_config,
        &BoundaryAdjacentRandomnessFactory,
    )
    .expect("second boundary-adjacent run should complete");
    let first_event_path = first_run_result.projected_event_path();
    let second_event_path = second_run_result.projected_event_path();

    assert_eq!(first_event_path, second_event_path);
    assert_eq!(
        first_run_result.final_excitation_state,
        second_run_result.final_excitation_state
    );

    assert_eq!(first_event_path.len(), 2);

    let first_timestamp_utc_nanos = first_event_path[0].timestamp_utc_nanos;
    let second_timestamp_utc_nanos = first_event_path[1].timestamp_utc_nanos;

    let boundary_utc_nanos = OPEN_MARKET_START_TIME_UTC_NANOS + ONE_HOUR_NANOS;
    assert!(first_timestamp_utc_nanos < boundary_utc_nanos);
    assert!(second_timestamp_utc_nanos > boundary_utc_nanos);
    assert_ne!(first_timestamp_utc_nanos, boundary_utc_nanos);
    assert_ne!(second_timestamp_utc_nanos, boundary_utc_nanos);
    assert!(has_non_grid_event_time(
        &first_event_path,
        OPEN_MARKET_START_TIME_UTC_NANOS,
        ONE_HOUR_NANOS,
    ));
}

#[test]
fn seeded_behavior_validation_tied_timestamps_follow_documented_event_ordering_policy() {
    assert_hawkes_invariant_catalog_ids_have_check_class(
        &["HK-BND-004"],
        HawkesInvariantCheckClass::DeterministicEquation,
    );

    let run_config = SeededTickSimulationRunConfig::try_new(
        91_009,
        DateTime::<Utc>::from_timestamp_nanos(OPEN_MARKET_START_TIME_UTC_NANOS),
        1,
        build_one_interval_ordering_model_inputs(OPEN_MARKET_START_TIME_UTC_NANOS, 1.0),
        Some(SeededTickSimulationRunOptions {
            debug_trace_enabled: true,
            debug_trace_options: SeededTickSimulationDebugTraceOptions {
                sample_every_nth_transition: 1,
                max_records_per_chunk: 8,
            },
            ..SeededTickSimulationRunOptions::default()
        }),
    )
    .expect("tied-ordering run config should initialize");

    let tied_offset_nanos = ((ONE_HOUR_NANOS as f64) * 0.3).floor() as i64;
    let tied_hours = tied_offset_nanos as f64 / ONE_HOUR_NANOS as f64;
    let tied_wait_draw = (-2.0 * tied_hours).exp();

    let scripted_factory = ScriptedRandomnessFactory::new(
        vec![0.0],
        vec![0.0],
        vec![tied_wait_draw],
        vec![0.0],
        vec![0.0],
        vec![0.3],
        vec![0.5, 0.25, 0.5],
        vec![0.25, 0.5],
    );

    let first_run_result = run_seeded_tick_event_simulation_with_randomness_factory(
        run_config.clone(),
        &scripted_factory,
    )
    .expect("first tied-ordering run should complete");
    let second_run_result =
        run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
            .expect("second tied-ordering run should complete");
    let first_event_path = first_run_result.projected_event_path();
    let second_event_path = second_run_result.projected_event_path();

    assert_eq!(first_event_path, second_event_path);
    assert_eq!(
        first_run_result.event_ordering_policy,
        SEEDED_TICK_EVENT_ORDERING_POLICY
    );
    assert!(first_event_path.is_empty());

    let trace_records = collect_debug_trace_records(&first_run_result);
    assert_eq!(trace_records.len(), 2);

    assert_eq!(
        trace_records[0].transition_kind,
        SeededTickEventDebugTraceTransitionKind::ExogenousNews
    );
    assert_eq!(
        trace_records[1].transition_kind,
        SeededTickEventDebugTraceTransitionKind::IntervalBoundaryDrift
    );

    let tied_timestamp_utc_nanos = OPEN_MARKET_START_TIME_UTC_NANOS + tied_offset_nanos;
    assert_eq!(
        trace_records[0].timestamp_utc_nanos,
        tied_timestamp_utc_nanos
    );
}

#[test]
fn seeded_behavior_validation_near_tied_timestamps_follow_timestamp_priority_ordering() {
    assert_hawkes_invariant_catalog_ids_have_check_class(
        &["HK-BND-004"],
        HawkesInvariantCheckClass::DeterministicEquation,
    );

    let run_config = SeededTickSimulationRunConfig::try_new(
        91_010,
        DateTime::<Utc>::from_timestamp_nanos(OPEN_MARKET_START_TIME_UTC_NANOS),
        1,
        build_one_interval_ordering_model_inputs(OPEN_MARKET_START_TIME_UTC_NANOS, 0.0),
        None,
    )
    .expect("near-tie ordering run config should initialize");

    let scripted_factory = ScriptedRandomnessFactory::new(
        vec![0.9, 0.1],
        vec![0.0],
        vec![0.95, 0.949_999_999],
        vec![0.0, 0.0],
        vec![1.0],
        vec![],
        vec![],
        vec![],
    );

    let run_result =
        run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
            .expect("near-tie ordering run should complete");
    let projected_event_path = run_result.projected_event_path();
    assert_eq!(projected_event_path.len(), 2);

    let first_timestamp_utc_nanos = projected_event_path[0].timestamp_utc_nanos;
    let second_timestamp_utc_nanos = projected_event_path[1].timestamp_utc_nanos;

    assert!(first_timestamp_utc_nanos < second_timestamp_utc_nanos);
    assert_eq!(projected_event_path[0].side, SeededTickEventSide::Sell);
    assert_eq!(projected_event_path[1].side, SeededTickEventSide::Buy);
    assert_eq!(
        run_result.event_ordering_policy,
        SEEDED_TICK_EVENT_ORDERING_POLICY
    );
    assert!(second_timestamp_utc_nanos - first_timestamp_utc_nanos > 0);
}

#[test]
fn seeded_behavior_validation_exhibits_clustering_and_non_grid_event_time_signatures() {
    assert_hawkes_invariant_catalog_ids_have_check_class(
        &["HK-SAMP-001"],
        HawkesInvariantCheckClass::StatisticalProperty,
    );

    let seed_values = seed_values(20_000, 32);
    let clustering_thresholds = ClusteringThresholds {
        minimum_fano_factor_gap: 4.5,
        minimum_lag_one_autocorrelation_gap: 0.12,
        minimum_cluster_lag_one_autocorrelation: 0.65,
        minimum_non_grid_run_ratio: 0.95,
    };

    let baseline_metrics = compute_batch_clustering_metrics(
        &seed_values,
        symmetric_kernel_parameters(),
        72,
        ONE_HOUR_NANOS,
        1.0,
        0.02,
    );
    let clustered_metrics = compute_batch_clustering_metrics(
        &seed_values,
        clustered_kernel_parameters(),
        72,
        ONE_HOUR_NANOS,
        1.0,
        0.02,
    );

    let fano_factor_gap = clustered_metrics.mean_fano_factor - baseline_metrics.mean_fano_factor;
    let lag_one_autocorrelation_gap = clustered_metrics.mean_lag_one_autocorrelation
        - baseline_metrics.mean_lag_one_autocorrelation;

    assert!(
        fano_factor_gap >= clustering_thresholds.minimum_fano_factor_gap,
        "clustered fano-factor gap below threshold: observed_gap={fano_factor_gap:.6}, \
         thresholds={clustering_thresholds:?}, baseline={baseline_metrics:?}, clustered={clustered_metrics:?}",
    );
    assert!(
        lag_one_autocorrelation_gap >= clustering_thresholds.minimum_lag_one_autocorrelation_gap,
        "clustered lag-one autocorrelation gap below threshold: observed_gap={lag_one_autocorrelation_gap:.6}, \
         thresholds={clustering_thresholds:?}, baseline={baseline_metrics:?}, clustered={clustered_metrics:?}",
    );
    assert!(
        clustered_metrics.mean_lag_one_autocorrelation
            >= clustering_thresholds.minimum_cluster_lag_one_autocorrelation,
        "clustered lag-one autocorrelation below threshold: thresholds={clustering_thresholds:?}, \
         clustered={clustered_metrics:?}",
    );
    assert!(
        clustered_metrics.non_grid_run_ratio >= clustering_thresholds.minimum_non_grid_run_ratio,
        "non-grid event-time coverage below threshold: thresholds={clustering_thresholds:?}, \
         clustered={clustered_metrics:?}",
    );
}

#[test]
fn seeded_behavior_validation_exhibits_expected_buy_sell_asymmetry() {
    assert_hawkes_invariant_catalog_ids_have_check_class(
        &["HK-SAMP-002"],
        HawkesInvariantCheckClass::StatisticalProperty,
    );

    let seed_values = seed_values(30_000, 32);
    let asymmetry_thresholds = AsymmetryThresholds {
        symmetric_sell_share_tolerance: 0.04,
        symmetric_signed_drift_tolerance: 0.15,
        symmetric_side_fano_factor_gap_tolerance: 0.20,
        minimum_sell_share_gap: 0.07,
        minimum_sell_to_buy_log_ratio_gap: 0.20,
        minimum_sell_minus_buy_signed_drift_gap: 0.08,
        minimum_sell_side_fano_factor_gap: 0.04,
    };

    let scenarios = asymmetry_regression_scenarios();
    let mut observed_metrics: Vec<(AsymmetryScenarioMetadata, AsymmetryMetrics)> =
        Vec::with_capacity(scenarios.len());

    for scenario in scenarios {
        let first_metrics = compute_batch_asymmetry_metrics(
            &seed_values,
            scenario.kernel_parameters.clone(),
            72,
            ONE_HOUR_NANOS,
            1.0,
            0.01,
        );
        let second_metrics = compute_batch_asymmetry_metrics(
            &seed_values,
            scenario.kernel_parameters,
            72,
            ONE_HOUR_NANOS,
            1.0,
            0.01,
        );

        assert_eq!(
            first_metrics, second_metrics,
            "asymmetry scenario replay mismatch for metadata={:?}",
            scenario.metadata,
        );

        observed_metrics.push((scenario.metadata, first_metrics));
    }

    let symmetric_metrics = observed_metrics
        .iter()
        .find_map(|(metadata, metrics)| {
            (metadata.expected_qualitative_behavior
                == AsymmetryExpectedQualitativeBehavior::NearSymmetricControl)
                .then_some(*metrics)
        })
        .expect("asymmetry scenario set must include near-symmetric control");
    let sell_asymmetric_metrics = observed_metrics
        .iter()
        .find_map(|(metadata, metrics)| {
            (metadata.expected_qualitative_behavior
                == AsymmetryExpectedQualitativeBehavior::SellSideStress)
                .then_some(*metrics)
        })
        .expect("asymmetry scenario set must include sell-side stress regime");

    let symmetric_distance_from_half = (symmetric_metrics.mean_sell_share - 0.5).abs();
    let sell_share_gap =
        sell_asymmetric_metrics.mean_sell_share - symmetric_metrics.mean_sell_share;
    let sell_to_buy_log_ratio_gap = sell_asymmetric_metrics.mean_log_sell_to_buy_ratio
        - symmetric_metrics.mean_log_sell_to_buy_ratio;

    assert!(
        symmetric_distance_from_half <= asymmetry_thresholds.symmetric_sell_share_tolerance,
        "symmetric sell-share drift exceeds tolerance: observed={symmetric_distance_from_half:.6}, \
         thresholds={asymmetry_thresholds:?}, symmetric_metrics={symmetric_metrics:?}",
    );
    assert!(
        sell_share_gap >= asymmetry_thresholds.minimum_sell_share_gap,
        "sell-share asymmetry gap below threshold: observed={sell_share_gap:.6}, \
         thresholds={asymmetry_thresholds:?}, symmetric_metrics={symmetric_metrics:?}, \
         sell_asymmetric_metrics={sell_asymmetric_metrics:?}",
    );
    assert!(
        sell_to_buy_log_ratio_gap >= asymmetry_thresholds.minimum_sell_to_buy_log_ratio_gap,
        "sell-to-buy log-ratio gap below threshold: observed={sell_to_buy_log_ratio_gap:.6}, \
         thresholds={asymmetry_thresholds:?}, symmetric_metrics={symmetric_metrics:?}, \
         sell_asymmetric_metrics={sell_asymmetric_metrics:?}",
    );
    assert!(
        symmetric_metrics
            .mean_signed_sell_minus_buy_per_interval
            .abs()
            <= asymmetry_thresholds.symmetric_signed_drift_tolerance,
        "symmetric side-specific drift exceeded tolerance: thresholds={asymmetry_thresholds:?}, \
         symmetric_metrics={symmetric_metrics:?}",
    );
    assert!(
        (symmetric_metrics.mean_sell_fano_factor - symmetric_metrics.mean_buy_fano_factor).abs()
            <= asymmetry_thresholds.symmetric_side_fano_factor_gap_tolerance,
        "symmetric side-specific clustering gap exceeded tolerance: thresholds={asymmetry_thresholds:?}, \
         symmetric_metrics={symmetric_metrics:?}",
    );
    assert!(
        sell_asymmetric_metrics.mean_sell_share > 0.5,
        "sell-side stress scenario inverted expected side bias: \
         sell_asymmetric_metrics={sell_asymmetric_metrics:?}, symmetric_metrics={symmetric_metrics:?}",
    );
    assert!(
        sell_asymmetric_metrics.mean_signed_sell_minus_buy_per_interval
            > symmetric_metrics.mean_signed_sell_minus_buy_per_interval
                + asymmetry_thresholds.minimum_sell_minus_buy_signed_drift_gap,
        "sell-side stress scenario drift dampening detected: thresholds={asymmetry_thresholds:?}, \
         symmetric_metrics={symmetric_metrics:?}, sell_asymmetric_metrics={sell_asymmetric_metrics:?}",
    );
    assert!(
        sell_asymmetric_metrics.mean_sell_fano_factor
            > sell_asymmetric_metrics.mean_buy_fano_factor
                + asymmetry_thresholds.minimum_sell_side_fano_factor_gap,
        "sell-side clustering signature dampened under stress scenario: thresholds={asymmetry_thresholds:?}, \
         symmetric_metrics={symmetric_metrics:?}, sell_asymmetric_metrics={sell_asymmetric_metrics:?}",
    );
}

#[test]
fn seeded_behavior_validation_compares_aggregated_fine_and_direct_coarse_summaries() {
    assert_hawkes_invariant_catalog_ids_have_check_class(
        &["HK-SCALE-001"],
        HawkesInvariantCheckClass::StatisticalProperty,
    );

    let seed_values = seed_values(40_000, 24);
    let total_duration_nanos = ONE_DAY_NANOS;
    let fine_interval_duration_nanos = FIFTEEN_MINUTE_NANOS;

    let horizons = [
        (
            "15m",
            FIFTEEN_MINUTE_NANOS,
            ScaleConsistencyThresholds {
                maximum_relative_total_event_count_delta: 0.0,
                maximum_absolute_buy_share_delta: 0.0,
                maximum_absolute_sell_share_delta: 0.0,
                maximum_absolute_log_sell_to_buy_ratio_delta: 0.0,
            },
        ),
        (
            "1h",
            ONE_HOUR_NANOS,
            ScaleConsistencyThresholds {
                maximum_relative_total_event_count_delta: 0.15,
                maximum_absolute_buy_share_delta: 0.08,
                maximum_absolute_sell_share_delta: 0.08,
                maximum_absolute_log_sell_to_buy_ratio_delta: 0.25,
            },
        ),
        (
            "1d",
            ONE_DAY_NANOS,
            ScaleConsistencyThresholds {
                maximum_relative_total_event_count_delta: 0.65,
                maximum_absolute_buy_share_delta: 0.12,
                maximum_absolute_sell_share_delta: 0.12,
                maximum_absolute_log_sell_to_buy_ratio_delta: 0.60,
            },
        ),
    ];

    for (horizon_label, coarse_interval_duration_nanos, thresholds) in horizons {
        let (aggregated_summaries, direct_summaries) = collect_scale_consistency_summaries(
            &seed_values,
            symmetric_kernel_parameters(),
            total_duration_nanos,
            fine_interval_duration_nanos,
            coarse_interval_duration_nanos,
            1.0,
            0.01,
        );

        let metrics = compute_scale_consistency_metrics(&aggregated_summaries, &direct_summaries);

        assert!(
            metrics.relative_total_event_count_delta
                <= thresholds.maximum_relative_total_event_count_delta,
            "coarse consistency total-event delta exceeded threshold for {horizon_label}: \
             thresholds={thresholds:?}, metrics={metrics:?}",
        );
        assert!(
            metrics.absolute_buy_share_delta <= thresholds.maximum_absolute_buy_share_delta,
            "coarse consistency buy-share delta exceeded threshold for {horizon_label}: \
             thresholds={thresholds:?}, metrics={metrics:?}",
        );
        assert!(
            metrics.absolute_sell_share_delta <= thresholds.maximum_absolute_sell_share_delta,
            "coarse consistency sell-share delta exceeded threshold for {horizon_label}: \
             thresholds={thresholds:?}, metrics={metrics:?}",
        );
        assert!(
            metrics.absolute_log_sell_to_buy_ratio_delta
                <= thresholds.maximum_absolute_log_sell_to_buy_ratio_delta,
            "coarse consistency sell-to-buy log-ratio delta exceeded threshold for {horizon_label}: \
             thresholds={thresholds:?}, metrics={metrics:?}",
        );
    }

    let coarse_asymmetry_thresholds = ScaleConsistencyThresholds {
        maximum_relative_total_event_count_delta: 0.20,
        maximum_absolute_buy_share_delta: 0.10,
        maximum_absolute_sell_share_delta: 0.10,
        maximum_absolute_log_sell_to_buy_ratio_delta: 0.30,
    };

    let (aggregated_summaries, direct_summaries) = collect_scale_consistency_summaries(
        &seed_values,
        sell_asymmetric_kernel_parameters(),
        total_duration_nanos,
        fine_interval_duration_nanos,
        ONE_HOUR_NANOS,
        1.0,
        0.01,
    );

    let asymmetry_metrics =
        compute_scale_consistency_metrics(&aggregated_summaries, &direct_summaries);

    assert!(
        asymmetry_metrics.direct_summary.mean_sell_share > 0.5,
        "direct coarse asymmetry summary lost expected sell-side bias: metrics={asymmetry_metrics:?}",
    );
    assert!(
        asymmetry_metrics.aggregated_summary.mean_sell_share > 0.5,
        "aggregated fine asymmetry summary lost expected sell-side bias: metrics={asymmetry_metrics:?}",
    );
    assert!(
        asymmetry_metrics.relative_total_event_count_delta
            <= coarse_asymmetry_thresholds.maximum_relative_total_event_count_delta,
        "coarse asymmetry consistency total-event delta exceeded threshold: \
         thresholds={coarse_asymmetry_thresholds:?}, metrics={asymmetry_metrics:?}",
    );
    assert!(
        asymmetry_metrics.absolute_buy_share_delta
            <= coarse_asymmetry_thresholds.maximum_absolute_buy_share_delta,
        "coarse asymmetry consistency buy-share delta exceeded threshold: \
         thresholds={coarse_asymmetry_thresholds:?}, metrics={asymmetry_metrics:?}",
    );
    assert!(
        asymmetry_metrics.absolute_sell_share_delta
            <= coarse_asymmetry_thresholds.maximum_absolute_sell_share_delta,
        "coarse asymmetry consistency sell-share delta exceeded threshold: \
         thresholds={coarse_asymmetry_thresholds:?}, metrics={asymmetry_metrics:?}",
    );
    assert!(
        asymmetry_metrics.absolute_log_sell_to_buy_ratio_delta
            <= coarse_asymmetry_thresholds.maximum_absolute_log_sell_to_buy_ratio_delta,
        "coarse asymmetry consistency sell-to-buy log-ratio delta exceeded threshold: \
         thresholds={coarse_asymmetry_thresholds:?}, metrics={asymmetry_metrics:?}",
    );
}

fn compute_batch_clustering_metrics(
    seed_values: &[u64],
    kernel_parameters: HawkesKernelParameters,
    interval_count: usize,
    interval_duration_nanos: i64,
    seasonality_factor: f64,
    news_intensity: f64,
) -> ClusteringMetrics {
    let mut fano_factors: Vec<f64> = Vec::with_capacity(seed_values.len());
    let mut lag_one_autocorrelations: Vec<f64> = Vec::with_capacity(seed_values.len());
    let mut non_grid_run_count: usize = 0;

    for seed in seed_values {
        let run_result = run_seeded_tick_event_simulation(build_run_config(
            *seed,
            interval_duration_nanos,
            interval_count,
            kernel_parameters,
            HawkesExcitationState {
                y_plus_plus: 0.0,
                y_plus_minus: 0.0,
                y_minus_plus: 0.0,
                y_minus_minus: 0.0,
            },
            seasonality_factor,
            news_intensity,
        ))
        .expect("batch clustering run should complete");
        let projected_event_path = run_result.projected_event_path();

        let interval_event_counts = collect_interval_event_counts(
            &projected_event_path,
            START_TIME_UTC_NANOS,
            interval_duration_nanos,
            interval_count,
        );

        fano_factors.push(calculate_fano_factor(&interval_event_counts));
        lag_one_autocorrelations.push(calculate_lag_one_autocorrelation(&interval_event_counts));

        if has_non_grid_event_time(
            &projected_event_path,
            START_TIME_UTC_NANOS,
            interval_duration_nanos,
        ) {
            non_grid_run_count = non_grid_run_count.saturating_add(1);
        }
    }

    ClusteringMetrics {
        mean_fano_factor: calculate_mean_f64(&fano_factors),
        mean_lag_one_autocorrelation: calculate_mean_f64(&lag_one_autocorrelations),
        non_grid_run_ratio: non_grid_run_count as f64 / seed_values.len() as f64,
    }
}

fn compute_batch_asymmetry_metrics(
    seed_values: &[u64],
    kernel_parameters: HawkesKernelParameters,
    interval_count: usize,
    interval_duration_nanos: i64,
    seasonality_factor: f64,
    news_intensity: f64,
) -> AsymmetryMetrics {
    let mut sell_shares: Vec<f64> = Vec::with_capacity(seed_values.len());
    let mut log_sell_to_buy_ratios: Vec<f64> = Vec::with_capacity(seed_values.len());
    let mut signed_sell_minus_buy_drifts: Vec<f64> = Vec::with_capacity(seed_values.len());
    let mut buy_fano_factors: Vec<f64> = Vec::with_capacity(seed_values.len());
    let mut sell_fano_factors: Vec<f64> = Vec::with_capacity(seed_values.len());

    for seed in seed_values {
        let run_result = run_seeded_tick_event_simulation(build_run_config(
            *seed,
            interval_duration_nanos,
            interval_count,
            kernel_parameters,
            HawkesExcitationState {
                y_plus_plus: 0.0,
                y_plus_minus: 0.0,
                y_minus_plus: 0.0,
                y_minus_minus: 0.0,
            },
            seasonality_factor,
            news_intensity,
        ))
        .expect("batch asymmetry run should complete");

        let buy_event_count = run_result
            .projected_event_path()
            .iter()
            .filter(|event_record| event_record.side == SeededTickEventSide::Buy)
            .count();
        let sell_event_count = run_result
            .projected_event_path()
            .iter()
            .filter(|event_record| event_record.side == SeededTickEventSide::Sell)
            .count();

        let total_event_count = buy_event_count + sell_event_count;
        if total_event_count == 0 {
            continue;
        }

        sell_shares.push(sell_event_count as f64 / total_event_count as f64);
        log_sell_to_buy_ratios
            .push(((sell_event_count as f64 + 1.0) / (buy_event_count as f64 + 1.0)).ln());

        signed_sell_minus_buy_drifts
            .push((sell_event_count as f64 - buy_event_count as f64) / interval_count as f64);

        let projected_event_path = run_result.projected_event_path();
        let interval_side_event_counts = collect_interval_buy_event_counts(
            &projected_event_path,
            START_TIME_UTC_NANOS,
            interval_duration_nanos,
            interval_count,
        );
        let interval_buy_counts: Vec<usize> = interval_side_event_counts
            .iter()
            .map(|(buy_event_count, _sell_event_count)| *buy_event_count)
            .collect();
        let interval_sell_counts: Vec<usize> = interval_side_event_counts
            .iter()
            .map(|(_buy_event_count, sell_event_count)| *sell_event_count)
            .collect();

        buy_fano_factors.push(calculate_fano_factor(&interval_buy_counts));
        sell_fano_factors.push(calculate_fano_factor(&interval_sell_counts));
    }

    AsymmetryMetrics {
        mean_sell_share: calculate_mean_f64(&sell_shares),
        mean_log_sell_to_buy_ratio: calculate_mean_f64(&log_sell_to_buy_ratios),
        mean_signed_sell_minus_buy_per_interval: calculate_mean_f64(&signed_sell_minus_buy_drifts),
        mean_buy_fano_factor: calculate_mean_f64(&buy_fano_factors),
        mean_sell_fano_factor: calculate_mean_f64(&sell_fano_factors),
    }
}

fn collect_scale_consistency_summaries(
    seed_values: &[u64],
    kernel_parameters: HawkesKernelParameters,
    total_duration_nanos: i64,
    fine_interval_duration_nanos: i64,
    coarse_interval_duration_nanos: i64,
    seasonality_factor: f64,
    news_intensity: f64,
) -> (Vec<HorizonSummary>, Vec<HorizonSummary>) {
    let fine_interval_count = usize::try_from(total_duration_nanos / fine_interval_duration_nanos)
        .expect("fine interval count should fit in usize");
    let coarse_interval_count =
        usize::try_from(total_duration_nanos / coarse_interval_duration_nanos)
            .expect("coarse interval count should fit in usize");

    let mut aggregated_summaries: Vec<HorizonSummary> = Vec::with_capacity(seed_values.len());
    let mut direct_summaries: Vec<HorizonSummary> = Vec::with_capacity(seed_values.len());

    for seed in seed_values {
        let fine_run_result = run_seeded_tick_event_simulation(build_run_config(
            *seed,
            fine_interval_duration_nanos,
            fine_interval_count,
            kernel_parameters.clone(),
            HawkesExcitationState {
                y_plus_plus: 0.0,
                y_plus_minus: 0.0,
                y_minus_plus: 0.0,
                y_minus_minus: 0.0,
            },
            seasonality_factor,
            news_intensity,
        ))
        .expect("fine run should complete");

        let direct_coarse_run_result = run_seeded_tick_event_simulation(build_run_config(
            *seed,
            coarse_interval_duration_nanos,
            coarse_interval_count,
            kernel_parameters.clone(),
            HawkesExcitationState {
                y_plus_plus: 0.0,
                y_plus_minus: 0.0,
                y_minus_plus: 0.0,
                y_minus_minus: 0.0,
            },
            seasonality_factor,
            news_intensity,
        ))
        .expect("direct coarse run should complete");
        let fine_event_path = fine_run_result.projected_event_path();
        let direct_coarse_event_path = direct_coarse_run_result.projected_event_path();

        aggregated_summaries.push(summarize_event_path(
            &fine_event_path,
            START_TIME_UTC_NANOS,
            coarse_interval_duration_nanos,
            coarse_interval_count,
        ));
        direct_summaries.push(summarize_event_path(
            &direct_coarse_event_path,
            START_TIME_UTC_NANOS,
            coarse_interval_duration_nanos,
            coarse_interval_count,
        ));
    }

    (aggregated_summaries, direct_summaries)
}

fn summarize_event_path(
    event_path: &[SeededTickEventRecord],
    start_time_utc_nanos: i64,
    horizon_duration_nanos: i64,
    horizon_interval_count: usize,
) -> HorizonSummary {
    let interval_buy_event_counts = collect_interval_buy_event_counts(
        event_path,
        start_time_utc_nanos,
        horizon_duration_nanos,
        horizon_interval_count,
    );

    let total_event_count: usize = interval_buy_event_counts
        .iter()
        .map(|(buy_event_count, sell_event_count)| buy_event_count + sell_event_count)
        .sum();
    let buy_event_count: usize = interval_buy_event_counts
        .iter()
        .map(|(buy_event_count, _sell_event_count)| *buy_event_count)
        .sum();
    let sell_event_count = total_event_count.saturating_sub(buy_event_count);

    let (buy_share, sell_share, log_sell_to_buy_ratio) = if total_event_count == 0 {
        (0.0, 0.0, 0.0)
    } else {
        (
            buy_event_count as f64 / total_event_count as f64,
            sell_event_count as f64 / total_event_count as f64,
            ((sell_event_count as f64 + 1.0) / (buy_event_count as f64 + 1.0)).ln(),
        )
    };

    HorizonSummary {
        mean_total_event_count: total_event_count as f64,
        mean_buy_share: buy_share,
        mean_sell_share: sell_share,
        mean_log_sell_to_buy_ratio: log_sell_to_buy_ratio,
    }
}

fn compute_scale_consistency_metrics(
    aggregated_summaries: &[HorizonSummary],
    direct_summaries: &[HorizonSummary],
) -> ScaleConsistencyMetrics {
    let aggregated_summary = HorizonSummary {
        mean_total_event_count: calculate_mean_f64(
            &aggregated_summaries
                .iter()
                .map(|summary| summary.mean_total_event_count)
                .collect::<Vec<f64>>(),
        ),
        mean_buy_share: calculate_mean_f64(
            &aggregated_summaries
                .iter()
                .map(|summary| summary.mean_buy_share)
                .collect::<Vec<f64>>(),
        ),
        mean_sell_share: calculate_mean_f64(
            &aggregated_summaries
                .iter()
                .map(|summary| summary.mean_sell_share)
                .collect::<Vec<f64>>(),
        ),
        mean_log_sell_to_buy_ratio: calculate_mean_f64(
            &aggregated_summaries
                .iter()
                .map(|summary| summary.mean_log_sell_to_buy_ratio)
                .collect::<Vec<f64>>(),
        ),
    };

    let direct_summary = HorizonSummary {
        mean_total_event_count: calculate_mean_f64(
            &direct_summaries
                .iter()
                .map(|summary| summary.mean_total_event_count)
                .collect::<Vec<f64>>(),
        ),
        mean_buy_share: calculate_mean_f64(
            &direct_summaries
                .iter()
                .map(|summary| summary.mean_buy_share)
                .collect::<Vec<f64>>(),
        ),
        mean_sell_share: calculate_mean_f64(
            &direct_summaries
                .iter()
                .map(|summary| summary.mean_sell_share)
                .collect::<Vec<f64>>(),
        ),
        mean_log_sell_to_buy_ratio: calculate_mean_f64(
            &direct_summaries
                .iter()
                .map(|summary| summary.mean_log_sell_to_buy_ratio)
                .collect::<Vec<f64>>(),
        ),
    };

    let relative_total_event_count_delta = if direct_summary.mean_total_event_count <= 0.0 {
        0.0
    } else {
        (aggregated_summary.mean_total_event_count - direct_summary.mean_total_event_count).abs()
            / direct_summary.mean_total_event_count
    };

    let absolute_buy_share_delta =
        (aggregated_summary.mean_buy_share - direct_summary.mean_buy_share).abs();
    let absolute_sell_share_delta =
        (aggregated_summary.mean_sell_share - direct_summary.mean_sell_share).abs();
    let absolute_log_sell_to_buy_ratio_delta = (aggregated_summary.mean_log_sell_to_buy_ratio
        - direct_summary.mean_log_sell_to_buy_ratio)
        .abs();

    ScaleConsistencyMetrics {
        aggregated_summary,
        direct_summary,
        relative_total_event_count_delta,
        absolute_buy_share_delta,
        absolute_sell_share_delta,
        absolute_log_sell_to_buy_ratio_delta,
    }
}

fn collect_interval_event_counts(
    event_path: &[SeededTickEventRecord],
    start_time_utc_nanos: i64,
    interval_duration_nanos: i64,
    interval_count: usize,
) -> Vec<usize> {
    let mut interval_event_counts: Vec<usize> = vec![0; interval_count];

    for event_record in event_path {
        let interval_position = event_interval_position(
            event_record.timestamp_utc_nanos,
            start_time_utc_nanos,
            interval_duration_nanos,
            interval_count,
        );

        if let Some(interval_position) = interval_position {
            interval_event_counts[interval_position] =
                interval_event_counts[interval_position].saturating_add(1);
        }
    }

    interval_event_counts
}

fn collect_interval_buy_event_counts(
    event_path: &[SeededTickEventRecord],
    start_time_utc_nanos: i64,
    interval_duration_nanos: i64,
    interval_count: usize,
) -> Vec<(usize, usize)> {
    let mut interval_side_event_counts: Vec<(usize, usize)> = vec![(0, 0); interval_count];

    for event_record in event_path {
        let interval_position = event_interval_position(
            event_record.timestamp_utc_nanos,
            start_time_utc_nanos,
            interval_duration_nanos,
            interval_count,
        );

        if let Some(interval_position) = interval_position {
            let (buy_event_count, sell_event_count) = interval_side_event_counts[interval_position];

            interval_side_event_counts[interval_position] = match event_record.side {
                SeededTickEventSide::Buy => (buy_event_count.saturating_add(1), sell_event_count),
                SeededTickEventSide::Sell => (buy_event_count, sell_event_count.saturating_add(1)),
            };
        }
    }

    interval_side_event_counts
}

fn event_interval_position(
    timestamp_utc_nanos: i64,
    start_time_utc_nanos: i64,
    interval_duration_nanos: i64,
    interval_count: usize,
) -> Option<usize> {
    if timestamp_utc_nanos < start_time_utc_nanos {
        return None;
    }

    let elapsed_nanos = timestamp_utc_nanos - start_time_utc_nanos;
    let interval_position = elapsed_nanos / interval_duration_nanos;

    if interval_position < 0 || interval_position >= interval_count as i64 {
        return None;
    }

    usize::try_from(interval_position).ok()
}

fn has_non_grid_event_time(
    event_path: &[SeededTickEventRecord],
    start_time_utc_nanos: i64,
    interval_duration_nanos: i64,
) -> bool {
    event_path.iter().any(|event_record| {
        let offset_nanos = event_record.timestamp_utc_nanos - start_time_utc_nanos;
        offset_nanos >= 0 && offset_nanos % interval_duration_nanos != 0
    })
}

fn calculate_fano_factor(interval_event_counts: &[usize]) -> f64 {
    if interval_event_counts.is_empty() {
        return 0.0;
    }

    let mean_event_count =
        interval_event_counts.iter().sum::<usize>() as f64 / interval_event_counts.len() as f64;
    if mean_event_count <= 0.0 {
        return 0.0;
    }

    let variance = interval_event_counts
        .iter()
        .map(|event_count| {
            let centered_count = *event_count as f64 - mean_event_count;
            centered_count * centered_count
        })
        .sum::<f64>()
        / interval_event_counts.len() as f64;

    variance / mean_event_count
}

fn calculate_lag_one_autocorrelation(interval_event_counts: &[usize]) -> f64 {
    if interval_event_counts.len() < 2 {
        return 0.0;
    }

    let left_interval_counts: Vec<f64> = interval_event_counts[..interval_event_counts.len() - 1]
        .iter()
        .map(|event_count| *event_count as f64)
        .collect();
    let right_interval_counts: Vec<f64> = interval_event_counts[1..]
        .iter()
        .map(|event_count| *event_count as f64)
        .collect();

    let left_mean = calculate_mean_f64(&left_interval_counts);
    let right_mean = calculate_mean_f64(&right_interval_counts);

    let numerator = left_interval_counts
        .iter()
        .zip(right_interval_counts.iter())
        .map(|(left_count, right_count)| (left_count - left_mean) * (right_count - right_mean))
        .sum::<f64>();

    let left_variance = left_interval_counts
        .iter()
        .map(|left_count| {
            let centered_left_count = left_count - left_mean;
            centered_left_count * centered_left_count
        })
        .sum::<f64>();
    let right_variance = right_interval_counts
        .iter()
        .map(|right_count| {
            let centered_right_count = right_count - right_mean;
            centered_right_count * centered_right_count
        })
        .sum::<f64>();

    let denominator = (left_variance * right_variance).sqrt();

    if denominator == 0.0 {
        return 0.0;
    }

    numerator / denominator
}

fn calculate_mean_f64(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }

    values.iter().sum::<f64>() / values.len() as f64
}

fn build_run_config(
    seed: u64,
    interval_duration_nanos: i64,
    interval_count: usize,
    kernel_parameters: HawkesKernelParameters,
    initial_excitation_state: HawkesExcitationState,
    seasonality_factor: f64,
    news_intensity: f64,
) -> SeededTickSimulationRunConfig {
    build_run_config_with_options(
        seed,
        interval_duration_nanos,
        interval_count,
        kernel_parameters,
        initial_excitation_state,
        seasonality_factor,
        news_intensity,
        None,
    )
}

fn build_run_config_with_options(
    seed: u64,
    interval_duration_nanos: i64,
    interval_count: usize,
    kernel_parameters: HawkesKernelParameters,
    initial_excitation_state: HawkesExcitationState,
    seasonality_factor: f64,
    news_intensity: f64,
    options: Option<SeededTickSimulationRunOptions>,
) -> SeededTickSimulationRunConfig {
    let hourly_factor_contract = build_uniform_factor_contract(
        START_TIME_UTC_NANOS,
        interval_duration_nanos,
        interval_count,
        seasonality_factor,
        news_intensity,
    );

    let (
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
    ) = disabled_mean_reversion_runtime_inputs(&hourly_factor_contract);

    SeededTickSimulationRunConfig::try_new(
        seed,
        DateTime::<Utc>::from_timestamp_nanos(START_TIME_UTC_NANOS),
        interval_count,
        SeededTickSimulationModelInputs {
            kernel_parameters,
            initial_excitation_state,
            mean_reversion_fitted_parameters,
            mean_reversion_runtime_interval_anchors,
            initial_mean_reversion_boundary_state,
            hourly_factor_contract,
            calendar_registry: build_seeded_simulation_calendar_registry(),
            nig_shared_plus_shift_configuration: valid_nig_shared_plus_shift_configuration(),
            news_process_configuration: build_news_process_configuration(Vec::new()),
        },
        options,
    )
    .expect("seeded run config should initialize")
}

fn build_uniform_factor_contract(
    start_time_utc_nanos: i64,
    interval_duration_nanos: i64,
    interval_count: usize,
    seasonality_factor: f64,
    news_intensity: f64,
) -> HourlyFactorContractOutput {
    let mut interval_indices: Vec<u64> = Vec::with_capacity(interval_count);
    let mut interval_start_utc_nanos: Vec<i64> = Vec::with_capacity(interval_count);
    let mut interval_end_utc_nanos: Vec<i64> = Vec::with_capacity(interval_count);
    let mut interval_start_exchange_local: Vec<String> = Vec::with_capacity(interval_count);
    let mut interval_end_exchange_local: Vec<String> = Vec::with_capacity(interval_count);

    for interval_position in 0..interval_count {
        let interval_start = start_time_utc_nanos
            + i64::try_from(interval_position).expect("interval position should fit in i64")
                * interval_duration_nanos;
        let interval_end = interval_start + interval_duration_nanos;

        interval_indices
            .push(u64::try_from(interval_position).expect("interval position should fit in u64"));
        interval_start_utc_nanos.push(interval_start);
        interval_end_utc_nanos.push(interval_end);
        interval_start_exchange_local
            .push(DateTime::<Utc>::from_timestamp_nanos(interval_start).to_rfc3339());
        interval_end_exchange_local
            .push(DateTime::<Utc>::from_timestamp_nanos(interval_end).to_rfc3339());
    }

    let request_end_utc_nanos = interval_end_utc_nanos
        .last()
        .copied()
        .expect("uniform factor contract should include at least one interval");

    HourlyFactorContractOutput {
        schema_version: HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
        exchange_code: "XNYS".to_owned(),
        exchange_timezone_name: "America/New_York".to_owned(),
        request_start_utc_nanos: start_time_utc_nanos,
        request_end_utc_nanos,
        interval_duration_nanos,
        interval_indices,
        interval_start_utc_nanos: interval_start_utc_nanos.clone(),
        interval_end_utc_nanos: interval_end_utc_nanos.clone(),
        interval_start_exchange_local,
        interval_end_exchange_local,
        s_h: vec![seasonality_factor; interval_count],
        known_component_nu_h: vec![0.0; interval_count],
        surprise_component_nu_h: vec![news_intensity; interval_count],
        nu_h: vec![news_intensity; interval_count],
        metadata: HourlyFactorMaterializationMetadata {
            seasonality_materialization_method: DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD
                .to_owned(),
            news_intensity_materialization_method: DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD
                .to_owned(),
            interval_boundary_policy: HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
            interval_alignment_policy: HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY,
            timezone_transition_policy: HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
            spring_forward_policy: HOURLY_FACTOR_SPRING_FORWARD_POLICY,
            fall_back_policy: HOURLY_FACTOR_FALL_BACK_POLICY,
        },
        diagnostics: HourlyFactorContractDiagnostics {
            interval_count,
            interval_duration_nanos,
            first_interval_start_utc_nanos: interval_start_utc_nanos[0],
            last_interval_end_utc_nanos: request_end_utc_nanos,
            skipped_local_hour_interval_count: 0,
            repeated_local_hour_interval_count: 0,
        },
    }
}

fn symmetric_kernel_parameters() -> HawkesKernelParameters {
    HawkesKernelParameters::try_new(
        0.9,
        1.6,
        HawkesKernelLoadings {
            plus_plus: 0.35,
            plus_minus: 0.20,
            minus_plus: 0.20,
            minus_minus: 0.35,
        },
        HawkesKernelLoadings {
            plus_plus: 0.02,
            plus_minus: 0.01,
            minus_plus: 0.01,
            minus_minus: 0.02,
        },
    )
    .expect("symmetric kernel parameters should be valid")
}

fn clustered_kernel_parameters() -> HawkesKernelParameters {
    HawkesKernelParameters::try_new(
        0.8,
        1.2,
        HawkesKernelLoadings {
            plus_plus: 0.75,
            plus_minus: 0.35,
            minus_plus: 0.32,
            minus_minus: 0.70,
        },
        HawkesKernelLoadings {
            plus_plus: 0.12,
            plus_minus: 0.05,
            minus_plus: 0.04,
            minus_minus: 0.10,
        },
    )
    .expect("clustered kernel parameters should be valid")
}

fn sell_asymmetric_kernel_parameters() -> HawkesKernelParameters {
    HawkesKernelParameters::try_new(
        0.8,
        1.4,
        HawkesKernelLoadings {
            plus_plus: 0.25,
            plus_minus: 0.10,
            minus_plus: 0.30,
            minus_minus: 0.55,
        },
        HawkesKernelLoadings {
            plus_plus: 0.02,
            plus_minus: 0.01,
            minus_plus: 0.07,
            minus_minus: 0.10,
        },
    )
    .expect("sell-asymmetric kernel parameters should be valid")
}

fn asymmetry_regression_scenarios() -> Vec<AsymmetryRegressionScenario> {
    vec![
        AsymmetryRegressionScenario {
            metadata: AsymmetryScenarioMetadata {
                scenario_id: "symmetric_baseline_control",
                regime_intent: "control_fixture_for_near_symmetric_buy_sell_balance",
                expected_qualitative_behavior:
                    AsymmetryExpectedQualitativeBehavior::NearSymmetricControl,
            },
            kernel_parameters: symmetric_kernel_parameters(),
        },
        AsymmetryRegressionScenario {
            metadata: AsymmetryScenarioMetadata {
                scenario_id: "sell_side_stress_regime",
                regime_intent: "stress_fixture_with_structured_sell_side_excitation",
                expected_qualitative_behavior: AsymmetryExpectedQualitativeBehavior::SellSideStress,
            },
            kernel_parameters: sell_asymmetric_kernel_parameters(),
        },
    ]
}

fn seed_values(start_seed: u64, seed_count: usize) -> Vec<u64> {
    (0..seed_count)
        .map(|seed_offset| start_seed + seed_offset as u64)
        .collect()
}

fn collect_debug_trace_records(
    run_result: &SeededTickSimulationRunResult,
) -> Vec<&SeededTickEventDebugTraceRecord> {
    run_result
        .debug_trace_output
        .as_ref()
        .expect("debug trace output should exist")
        .trace_chunks
        .iter()
        .flat_map(|trace_chunk| trace_chunk.records.iter())
        .collect()
}

fn build_multi_hour_zero_event_carryover_model_inputs(
    interval_count: usize,
) -> SeededTickSimulationModelInputs {
    let hourly_factor_contract = build_uniform_factor_contract(
        START_TIME_UTC_NANOS,
        ONE_HOUR_NANOS,
        interval_count,
        0.0,
        0.0,
    );

    let (
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
    ) = disabled_mean_reversion_runtime_inputs(&hourly_factor_contract);

    SeededTickSimulationModelInputs {
        kernel_parameters: HawkesKernelParameters::try_new(
            0.5,
            1.25,
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
        .expect("carryover kernel parameter fixture should be valid"),
        initial_excitation_state: HawkesExcitationState {
            y_plus_plus: 1.6,
            y_plus_minus: 0.8,
            y_minus_plus: 1.2,
            y_minus_minus: 0.4,
        },
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
        hourly_factor_contract,
        calendar_registry: build_seeded_simulation_calendar_registry(),
        nig_shared_plus_shift_configuration: valid_nig_shared_plus_shift_configuration(),
        news_process_configuration: build_news_process_configuration(Vec::new()),
    }
}

fn build_one_interval_ordering_model_inputs(
    start_time_utc_nanos: i64,
    news_intensity: f64,
) -> SeededTickSimulationModelInputs {
    let hourly_factor_contract =
        build_uniform_factor_contract(start_time_utc_nanos, ONE_HOUR_NANOS, 1, 1.0, news_intensity);

    let (
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
    ) = disabled_mean_reversion_runtime_inputs(&hourly_factor_contract);

    SeededTickSimulationModelInputs {
        kernel_parameters: HawkesKernelParameters::try_new(
            0.5,
            1.5,
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
        .expect("ordering kernel parameter fixture should be valid"),
        initial_excitation_state: HawkesExcitationState {
            y_plus_plus: 0.5,
            y_plus_minus: 0.0,
            y_minus_plus: 0.5,
            y_minus_minus: 0.0,
        },
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
        hourly_factor_contract,
        calendar_registry: build_seeded_simulation_calendar_registry(),
        nig_shared_plus_shift_configuration: valid_nig_shared_plus_shift_configuration(),
        news_process_configuration: build_news_process_configuration(Vec::new()),
    }
}

fn build_two_interval_boundary_adjacent_model_inputs(
    start_time_utc_nanos: i64,
) -> SeededTickSimulationModelInputs {
    let hourly_factor_contract =
        build_uniform_factor_contract(start_time_utc_nanos, ONE_HOUR_NANOS, 2, 1.0, 0.0);

    let (
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
    ) = disabled_mean_reversion_runtime_inputs(&hourly_factor_contract);

    SeededTickSimulationModelInputs {
        kernel_parameters: HawkesKernelParameters::try_new(
            0.5,
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
        .expect("boundary-adjacent kernel parameter fixture should be valid"),
        initial_excitation_state: HawkesExcitationState {
            y_plus_plus: 0.5,
            y_plus_minus: 0.0,
            y_minus_plus: 0.0,
            y_minus_minus: 0.0,
        },
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
        hourly_factor_contract,
        calendar_registry: build_seeded_simulation_calendar_registry(),
        nig_shared_plus_shift_configuration: valid_nig_shared_plus_shift_configuration(),
        news_process_configuration: build_news_process_configuration(Vec::new()),
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
        MeanReversionFittedParameters::endogenous_core_expectation_one_interval(0.0)
            .expect("zero-kappa endogenous mean reversion should construct"),
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

fn valid_nig_shared_plus_shift_configuration() -> NigSharedPlusShiftConfiguration {
    NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("nig configuration should construct")
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
        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: 0.6,
            return_jump_scale: 1.2,
        },
        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: 0.6,
            return_jump_scale: 1.2,
        },
    )
    .expect("news process configuration should construct")
}

fn build_seeded_simulation_calendar_registry() -> ExchangeCalendarRegistry {
    ExchangeCalendarRegistryLoader::load_from_json(
        r#"
        {
          "schema_version": "calendar.registry.v1",
          "artifact_date": "2026-01-31",
          "exchanges": [
            {
              "schema_version": "calendar.v1",
              "exchange_code": "XNYS",
              "exchange_name": "New York Stock Exchange",
              "timezone_name": "America/New_York",
              "rule_metadata": {
                "holiday_calendar_code": "US_EQUITIES",
                "early_close_rule_set": "US_EQUITIES_EARLY_CLOSE"
              },
              "regular_sessions": [
                {
                  "weekday": "Monday",
                  "windows": [
                    {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
                  ]
                },
                {
                  "weekday": "Tuesday",
                  "windows": [
                    {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
                  ]
                },
                {
                  "weekday": "Wednesday",
                  "windows": [
                    {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
                  ]
                },
                {
                  "weekday": "Thursday",
                  "windows": [
                    {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
                  ]
                },
                {
                  "weekday": "Friday",
                  "windows": [
                    {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
                  ]
                }
              ],
              "holiday_dates": ["2024-12-25"],
              "early_close_sessions": [
                {
                  "date": "2024-11-29",
                  "windows": [
                    {"label": "post_thanksgiving", "open_time": "09:30", "close_time": "13:00"}
                  ]
                }
              ]
            }
          ]
        }
        "#,
    )
    .expect("calendar registry fixture should load")
}

fn assert_float_close(observed: f64, expected: f64) {
    assert!(
        (observed - expected).abs() <= FLOAT_TOLERANCE,
        "float mismatch, expected {expected}, observed {observed}",
    );
}

#[derive(Debug, Clone, PartialEq)]
struct ScriptedRandomnessFactory {
    buy_count_draws: Vec<f64>,
    sell_count_draws: Vec<f64>,
    buy_timestamp_draws: Vec<f64>,
    sell_timestamp_draws: Vec<f64>,
    news_count_draws: Vec<f64>,
    news_timestamp_draws: Vec<f64>,
    news_severity_draws: Vec<f64>,
    news_return_jump_draws: Vec<f64>,
}

impl ScriptedRandomnessFactory {
    fn new(
        buy_count_draws: Vec<f64>,
        sell_count_draws: Vec<f64>,
        buy_timestamp_draws: Vec<f64>,
        sell_timestamp_draws: Vec<f64>,
        news_count_draws: Vec<f64>,
        news_timestamp_draws: Vec<f64>,
        news_severity_draws: Vec<f64>,
        news_return_jump_draws: Vec<f64>,
    ) -> Self {
        Self {
            buy_count_draws,
            sell_count_draws,
            buy_timestamp_draws,
            sell_timestamp_draws,
            news_count_draws,
            news_timestamp_draws,
            news_severity_draws,
            news_return_jump_draws,
        }
    }

    fn combined_news_mark_draws(&self) -> Vec<f64> {
        const SEVERITY_DRAWS_PER_EVENT: usize = 3;
        const BASE_SHOCK_DRAWS_PER_EVENT: usize = 2;

        let severity_chunks = self.news_severity_draws.chunks(SEVERITY_DRAWS_PER_EVENT);
        let base_shock_chunks = self
            .news_return_jump_draws
            .chunks(BASE_SHOCK_DRAWS_PER_EVENT);
        let combined_capacity = self
            .news_severity_draws
            .len()
            .saturating_add(self.news_return_jump_draws.len());
        let mut combined_draws = Vec::with_capacity(combined_capacity);

        for (severity_chunk, base_shock_chunk) in severity_chunks.zip(base_shock_chunks) {
            combined_draws.extend_from_slice(severity_chunk);
            combined_draws.extend_from_slice(base_shock_chunk);
        }

        let paired_event_count = self
            .news_severity_draws
            .len()
            .div_ceil(SEVERITY_DRAWS_PER_EVENT)
            .min(
                self.news_return_jump_draws
                    .len()
                    .div_ceil(BASE_SHOCK_DRAWS_PER_EVENT),
            );
        let paired_severity_draw_count = paired_event_count * SEVERITY_DRAWS_PER_EVENT;
        let paired_base_shock_draw_count = paired_event_count * BASE_SHOCK_DRAWS_PER_EVENT;

        if paired_severity_draw_count < self.news_severity_draws.len() {
            combined_draws
                .extend_from_slice(&self.news_severity_draws[paired_severity_draw_count..]);
        }
        if paired_base_shock_draw_count < self.news_return_jump_draws.len() {
            combined_draws
                .extend_from_slice(&self.news_return_jump_draws[paired_base_shock_draw_count..]);
        }

        combined_draws
    }
}

impl SimulationRandomnessFactory for ScriptedRandomnessFactory {
    type Stream = ScriptedRandomnessStream;

    fn create_substream(
        &self,
        _root_seed: u64,
        substream_domain: SimulationRandomnessSubstreamDomain,
        _substream_index: u64,
    ) -> Self::Stream {
        let draws = match substream_domain {
            SimulationRandomnessSubstreamDomain::EndogenousBuyEventCount => {
                self.buy_count_draws.clone()
            }
            SimulationRandomnessSubstreamDomain::EndogenousSellEventCount => {
                self.sell_count_draws.clone()
            }
            SimulationRandomnessSubstreamDomain::EndogenousBuyEventTimestamp => {
                self.buy_timestamp_draws.clone()
            }
            SimulationRandomnessSubstreamDomain::EndogenousSellEventTimestamp => {
                self.sell_timestamp_draws.clone()
            }
            SimulationRandomnessSubstreamDomain::ExogenousNewsCount => {
                self.news_count_draws.clone()
            }
            SimulationRandomnessSubstreamDomain::ExogenousNewsTimestamp => {
                self.news_timestamp_draws.clone()
            }
            SimulationRandomnessSubstreamDomain::ExogenousNewsSeverity => {
                self.news_severity_draws.clone()
            }
            SimulationRandomnessSubstreamDomain::ExogenousNewsReturnJump => {
                self.news_return_jump_draws.clone()
            }
            SimulationRandomnessSubstreamDomain::ExogenousNewsMarks => {
                self.combined_news_mark_draws()
            }
            SimulationRandomnessSubstreamDomain::CoarseTradingTimeReturn
            | SimulationRandomnessSubstreamDomain::CoarseStatisticsSampling
            | SimulationRandomnessSubstreamDomain::ScaleConsistencyPairedExperiment => Vec::new(),
        };

        ScriptedRandomnessStream::new(draws)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct ScriptedRandomnessStream {
    draws: Vec<f64>,
    position: usize,
}

impl ScriptedRandomnessStream {
    fn new(draws: Vec<f64>) -> Self {
        Self { draws, position: 0 }
    }
}

impl SimulationRandomnessStream for ScriptedRandomnessStream {
    fn next_u64(&mut self) -> u64 {
        let unit_interval_draw = self.next_unit_interval_f64();
        (unit_interval_draw.clamp(0.0, 1.0) * u64::MAX as f64).floor() as u64
    }

    fn next_unit_interval_f64(&mut self) -> f64 {
        let next_draw = self.draws.get(self.position).copied().unwrap_or(0.0);
        self.position = self.position.saturating_add(1);
        next_draw.clamp(0.0, 1.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BoundaryAdjacentRandomnessFactory;

impl SimulationRandomnessFactory for BoundaryAdjacentRandomnessFactory {
    type Stream = ScriptedRandomnessStream;

    fn create_substream(
        &self,
        _root_seed: u64,
        substream_domain: SimulationRandomnessSubstreamDomain,
        substream_index: u64,
    ) -> Self::Stream {
        let draws = match substream_domain {
            SimulationRandomnessSubstreamDomain::EndogenousBuyEventCount => vec![0.0],
            SimulationRandomnessSubstreamDomain::EndogenousSellEventCount => vec![0.0],
            SimulationRandomnessSubstreamDomain::EndogenousBuyEventTimestamp => {
                if substream_index == 0 {
                    vec![0.367_981_614_81]
                } else {
                    vec![0.999_999_999_999]
                }
            }
            SimulationRandomnessSubstreamDomain::EndogenousSellEventTimestamp => vec![],
            SimulationRandomnessSubstreamDomain::ExogenousNewsCount => vec![0.0],
            SimulationRandomnessSubstreamDomain::ExogenousNewsTimestamp => vec![],
            SimulationRandomnessSubstreamDomain::ExogenousNewsSeverity => vec![],
            SimulationRandomnessSubstreamDomain::ExogenousNewsReturnJump => vec![],
            SimulationRandomnessSubstreamDomain::ExogenousNewsMarks
            | SimulationRandomnessSubstreamDomain::CoarseTradingTimeReturn
            | SimulationRandomnessSubstreamDomain::CoarseStatisticsSampling
            | SimulationRandomnessSubstreamDomain::ScaleConsistencyPairedExperiment => vec![],
        };

        ScriptedRandomnessStream::new(draws)
    }
}
