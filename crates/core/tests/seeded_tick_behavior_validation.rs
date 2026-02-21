mod support;

use chrono::{DateTime, Utc};
use janiform_core::hawkes::{HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters};
use janiform_core::hourly_factors::{
    DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD, DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD,
    HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION, HOURLY_FACTOR_FALL_BACK_POLICY,
    HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY, HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
    HOURLY_FACTOR_SPRING_FORWARD_POLICY, HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
    HourlyFactorContractDiagnostics, HourlyFactorContractOutput,
    HourlyFactorMaterializationMetadata,
};
use janiform_core::simulation::{
    SeededTickEventRecord, SeededTickEventSide, SeededTickSimulationModelInputs,
    SeededTickSimulationRunConfig, run_seeded_tick_event_simulation,
};
use support::hawkes_invariant_catalog::{
    HawkesInvariantCheckClass, assert_hawkes_invariant_catalog_ids_have_check_class,
};

const START_TIME_UTC_NANOS: i64 = 1_704_067_200_000_000_000;
const FIFTEEN_MINUTE_NANOS: i64 = 900_000_000_000;
const ONE_HOUR_NANOS: i64 = 3_600_000_000_000;
const ONE_DAY_NANOS: i64 = 86_400_000_000_000;

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
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct AsymmetryThresholds {
    symmetric_sell_share_tolerance: f64,
    minimum_sell_share_gap: f64,
    minimum_sell_to_buy_log_ratio_gap: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct HorizonSummary {
    mean_total_event_count: f64,
    mean_buy_share: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ScaleConsistencyMetrics {
    aggregated_summary: HorizonSummary,
    direct_summary: HorizonSummary,
    relative_total_event_count_delta: f64,
    absolute_buy_share_delta: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ScaleConsistencyThresholds {
    maximum_relative_total_event_count_delta: f64,
    maximum_absolute_buy_share_delta: f64,
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

    assert_eq!(
        first_run_result.event_path, second_run_result.event_path,
        "determinism failure for repeated seeded runs"
    );
    assert_eq!(
        first_run_result.final_excitation_state, second_run_result.final_excitation_state,
        "determinism failure for repeated seeded final states"
    );
}

#[test]
fn seeded_behavior_validation_exhibits_clustering_and_non_grid_event_time_signatures() {
    assert_hawkes_invariant_catalog_ids_have_check_class(
        &["HK-SAMP-001"],
        HawkesInvariantCheckClass::StatisticalProperty,
    );

    let seed_values = seed_values(20_000, 32);
    let clustering_thresholds = ClusteringThresholds {
        minimum_fano_factor_gap: 0.35,
        minimum_lag_one_autocorrelation_gap: 0.45,
        minimum_cluster_lag_one_autocorrelation: 0.80,
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
        minimum_sell_share_gap: 0.08,
        minimum_sell_to_buy_log_ratio_gap: 0.20,
    };

    let symmetric_metrics = compute_batch_asymmetry_metrics(
        &seed_values,
        symmetric_kernel_parameters(),
        72,
        ONE_HOUR_NANOS,
        1.0,
        0.01,
    );
    let sell_asymmetric_metrics = compute_batch_asymmetry_metrics(
        &seed_values,
        sell_asymmetric_kernel_parameters(),
        72,
        ONE_HOUR_NANOS,
        1.0,
        0.01,
    );

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
    let fine_interval_count = usize::try_from(total_duration_nanos / fine_interval_duration_nanos)
        .expect("fifteen-minute interval count should fit in usize");
    let kernel_parameters = symmetric_kernel_parameters();

    let horizons = [
        (
            "15m",
            FIFTEEN_MINUTE_NANOS,
            ScaleConsistencyThresholds {
                maximum_relative_total_event_count_delta: 0.0,
                maximum_absolute_buy_share_delta: 0.0,
            },
        ),
        (
            "1h",
            ONE_HOUR_NANOS,
            ScaleConsistencyThresholds {
                maximum_relative_total_event_count_delta: 0.15,
                maximum_absolute_buy_share_delta: 0.08,
            },
        ),
        (
            "1d",
            ONE_DAY_NANOS,
            ScaleConsistencyThresholds {
                maximum_relative_total_event_count_delta: 0.65,
                maximum_absolute_buy_share_delta: 0.12,
            },
        ),
    ];

    for (horizon_label, coarse_interval_duration_nanos, thresholds) in horizons {
        let coarse_interval_count =
            usize::try_from(total_duration_nanos / coarse_interval_duration_nanos)
                .expect("coarse interval count should fit in usize");
        let mut aggregated_summaries: Vec<HorizonSummary> = Vec::with_capacity(seed_values.len());
        let mut direct_summaries: Vec<HorizonSummary> = Vec::with_capacity(seed_values.len());

        for seed in &seed_values {
            let fine_run_result = run_seeded_tick_event_simulation(build_run_config(
                *seed,
                fine_interval_duration_nanos,
                fine_interval_count,
                kernel_parameters,
                HawkesExcitationState {
                    y_plus_plus: 0.0,
                    y_plus_minus: 0.0,
                    y_minus_plus: 0.0,
                    y_minus_minus: 0.0,
                },
                1.0,
                0.01,
            ))
            .expect("fine run should complete");

            let direct_coarse_run_result = run_seeded_tick_event_simulation(build_run_config(
                *seed,
                coarse_interval_duration_nanos,
                coarse_interval_count,
                kernel_parameters,
                HawkesExcitationState {
                    y_plus_plus: 0.0,
                    y_plus_minus: 0.0,
                    y_minus_plus: 0.0,
                    y_minus_minus: 0.0,
                },
                1.0,
                0.01,
            ))
            .expect("direct coarse run should complete");

            aggregated_summaries.push(summarize_event_path(
                &fine_run_result.event_path,
                START_TIME_UTC_NANOS,
                coarse_interval_duration_nanos,
                coarse_interval_count,
            ));
            direct_summaries.push(summarize_event_path(
                &direct_coarse_run_result.event_path,
                START_TIME_UTC_NANOS,
                coarse_interval_duration_nanos,
                coarse_interval_count,
            ));
        }

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
    }
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

        let interval_event_counts = collect_interval_event_counts(
            &run_result.event_path,
            START_TIME_UTC_NANOS,
            interval_duration_nanos,
            interval_count,
        );

        fano_factors.push(calculate_fano_factor(&interval_event_counts));
        lag_one_autocorrelations.push(calculate_lag_one_autocorrelation(&interval_event_counts));

        if has_non_grid_event_time(
            &run_result.event_path,
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
            .event_path
            .iter()
            .filter(|event_record| event_record.side == SeededTickEventSide::Buy)
            .count();
        let sell_event_count = run_result
            .event_path
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
    }

    AsymmetryMetrics {
        mean_sell_share: calculate_mean_f64(&sell_shares),
        mean_log_sell_to_buy_ratio: calculate_mean_f64(&log_sell_to_buy_ratios),
    }
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

    let buy_share = if total_event_count == 0 {
        0.0
    } else {
        buy_event_count as f64 / total_event_count as f64
    };

    HorizonSummary {
        mean_total_event_count: total_event_count as f64,
        mean_buy_share: buy_share,
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
    };

    let relative_total_event_count_delta = if direct_summary.mean_total_event_count <= 0.0 {
        0.0
    } else {
        (aggregated_summary.mean_total_event_count - direct_summary.mean_total_event_count).abs()
            / direct_summary.mean_total_event_count
    };

    let absolute_buy_share_delta =
        (aggregated_summary.mean_buy_share - direct_summary.mean_buy_share).abs();

    ScaleConsistencyMetrics {
        aggregated_summary,
        direct_summary,
        relative_total_event_count_delta,
        absolute_buy_share_delta,
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
    SeededTickSimulationRunConfig::try_new(
        seed,
        DateTime::<Utc>::from_timestamp_nanos(START_TIME_UTC_NANOS),
        interval_count,
        SeededTickSimulationModelInputs {
            kernel_parameters,
            initial_excitation_state,
            hourly_factor_contract: build_uniform_factor_contract(
                interval_duration_nanos,
                interval_count,
                seasonality_factor,
                news_intensity,
            ),
        },
        None,
    )
    .expect("seeded run config should initialize")
}

fn build_uniform_factor_contract(
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
        let interval_start = START_TIME_UTC_NANOS
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
        request_start_utc_nanos: START_TIME_UTC_NANOS,
        request_end_utc_nanos,
        interval_indices,
        interval_start_utc_nanos: interval_start_utc_nanos.clone(),
        interval_end_utc_nanos: interval_end_utc_nanos.clone(),
        interval_start_exchange_local,
        interval_end_exchange_local,
        s_h: vec![seasonality_factor; interval_count],
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

fn seed_values(start_seed: u64, seed_count: usize) -> Vec<u64> {
    (0..seed_count)
        .map(|seed_offset| start_seed + seed_offset as u64)
        .collect()
}
