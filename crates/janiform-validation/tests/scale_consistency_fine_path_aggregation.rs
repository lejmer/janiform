use janiform_engine_sufficient_statistics::{
    SufficientStatisticsIntervalStatistics, SufficientStatisticsPath,
    SufficientStatisticsPathRecord, SufficientStatisticsReturnComponents,
};
use janiform_model_kernels::hawkes::HawkesExcitationState;
use janiform_validation::{
    SCALE_CONSISTENCY_FINE_PATH_ALIGNMENT_BOUNDARY_POLICY,
    SCALE_CONSISTENCY_FINE_PATH_ALIGNMENT_METADATA_SCHEMA_VERSION,
    ScaleConsistencyFinePathAggregationError, ScaleConsistencyFinePathAlignmentStatus,
    ScaleConsistencyFinePathPartialWindowPolicy,
    aggregate_fine_sufficient_statistics_path_to_coarse_intervals,
};

#[test]
fn fine_path_aggregation_matches_exact_coarse_boundaries() {
    let fine_path = build_fine_path(
        0.25,
        &[
            (0.1, 0.2, 1, 2, 0.01, -0.02, -0.01, 0.00, 0.00, -0.01),
            (0.3, 0.4, 3, 4, 0.03, -0.01, 0.02, 0.01, 0.00, 0.03),
            (0.5, 0.6, 5, 6, 0.02, -0.04, -0.02, 0.00, -0.01, -0.03),
            (0.7, 0.8, 7, 8, 0.04, -0.03, 0.01, 0.00, 0.02, 0.03),
        ],
    );

    let aggregated = aggregate_fine_sufficient_statistics_path_to_coarse_intervals(
        &fine_path,
        0.5,
        ScaleConsistencyFinePathPartialWindowPolicy::Reject,
    )
    .expect("exact interval partition should aggregate cleanly");

    assert_eq!(
        aggregated.alignment_metadata.schema_version,
        SCALE_CONSISTENCY_FINE_PATH_ALIGNMENT_METADATA_SCHEMA_VERSION
    );
    assert_eq!(
        aggregated.alignment_metadata.boundary_alignment_policy,
        SCALE_CONSISTENCY_FINE_PATH_ALIGNMENT_BOUNDARY_POLICY
    );
    assert_eq!(
        aggregated.alignment_metadata.alignment_status,
        ScaleConsistencyFinePathAlignmentStatus::ExactBoundaryMatch
    );
    assert_eq!(aggregated.alignment_metadata.source_fine_interval_count, 4);
    assert_eq!(
        aggregated
            .alignment_metadata
            .aggregated_coarse_interval_count,
        2
    );
    assert_eq!(
        aggregated
            .alignment_metadata
            .fine_subinterval_count_per_coarse_interval,
        2
    );
    assert_eq!(
        aggregated
            .alignment_metadata
            .trailing_partial_fine_interval_count,
        0
    );
    assert_eq!(aggregated.aggregated_path.interval_delta, 0.5);
    assert_eq!(aggregated.aggregated_path.simulated_interval_count, 2);
    assert_eq!(aggregated.aggregated_path.records.len(), 2);

    let first_record = &aggregated.aggregated_path.records[0];
    assert_eq!(first_record.interval_index, 0);
    assert_close(first_record.sufficient_statistics.a_plus, 0.4);
    assert_close(first_record.sufficient_statistics.a_minus, 0.6);
    assert_eq!(first_record.sufficient_statistics.n_plus, 4);
    assert_eq!(first_record.sufficient_statistics.n_minus, 6);
    assert_close(first_record.return_components.buy_side_return, 0.04);
    assert_close(first_record.return_components.sell_side_return, -0.03);
    assert_close(first_record.return_components.trading_time_return, 0.01);
    assert_close(first_record.return_components.open_jump_return, 0.01);
    assert_close(first_record.return_components.closed_gap_return, 0.0);
    assert_close(first_record.return_components.total_return, 0.02);

    let second_record = &aggregated.aggregated_path.records[1];
    assert_eq!(second_record.interval_index, 1);
    assert_close(second_record.sufficient_statistics.a_plus, 1.2);
    assert_close(second_record.sufficient_statistics.a_minus, 1.4);
    assert_eq!(second_record.sufficient_statistics.n_plus, 12);
    assert_eq!(second_record.sufficient_statistics.n_minus, 14);
    assert_close(second_record.return_components.buy_side_return, 0.06);
    assert_close(second_record.return_components.sell_side_return, -0.07);
    assert_close(second_record.return_components.trading_time_return, -0.01);
    assert_close(second_record.return_components.open_jump_return, 0.0);
    assert_close(second_record.return_components.closed_gap_return, 0.01);
    assert_close(second_record.return_components.total_return, 0.0);
}

#[test]
fn fine_path_aggregation_can_drop_trailing_partial_windows_explicitly() {
    let fine_path = build_fine_path(
        0.25,
        &[
            (0.2, 0.1, 1, 0, 0.01, -0.01, 0.00, 0.00, 0.00, 0.00),
            (0.4, 0.3, 2, 1, 0.02, -0.01, 0.01, 0.00, 0.00, 0.01),
            (0.6, 0.5, 3, 2, 0.03, -0.02, 0.01, 0.01, 0.00, 0.02),
            (0.8, 0.7, 4, 3, 0.04, -0.03, 0.01, 0.00, -0.01, 0.00),
            (1.0, 0.9, 5, 4, 0.05, -0.04, 0.01, 0.00, 0.00, 0.01),
        ],
    );

    let aggregated = aggregate_fine_sufficient_statistics_path_to_coarse_intervals(
        &fine_path,
        0.5,
        ScaleConsistencyFinePathPartialWindowPolicy::DropTrailing,
    )
    .expect("drop-trailing policy should keep only complete coarse windows");

    assert_eq!(
        aggregated.alignment_metadata.alignment_status,
        ScaleConsistencyFinePathAlignmentStatus::TrailingPartialWindowDropped
    );
    assert_eq!(
        aggregated
            .alignment_metadata
            .trailing_partial_fine_interval_count,
        1
    );
    assert_eq!(aggregated.aggregated_path.records.len(), 2);
    assert_close(
        aggregated.aggregated_path.diagnostics.cumulative_a_plus,
        2.0,
    );
    assert_close(
        aggregated.aggregated_path.diagnostics.cumulative_a_minus,
        1.6,
    );
    assert_eq!(aggregated.aggregated_path.diagnostics.cumulative_n_plus, 10);
    assert_eq!(aggregated.aggregated_path.diagnostics.cumulative_n_minus, 6);
    assert_close(
        aggregated
            .aggregated_path
            .diagnostics
            .cumulative_open_jump_return,
        0.01,
    );
    assert_close(
        aggregated
            .aggregated_path
            .diagnostics
            .cumulative_closed_gap_return,
        -0.01,
    );
}

#[test]
fn fine_path_aggregation_rejects_invalid_interval_pairs_and_rejected_partial_windows() {
    let fine_path = build_fine_path(
        0.25,
        &[
            (0.2, 0.1, 1, 0, 0.01, -0.01, 0.00, 0.00, 0.00, 0.00),
            (0.4, 0.3, 2, 1, 0.02, -0.01, 0.01, 0.00, 0.00, 0.01),
            (0.6, 0.5, 3, 2, 0.03, -0.02, 0.01, 0.01, 0.00, 0.02),
        ],
    );

    let invalid_interval_pair_error =
        aggregate_fine_sufficient_statistics_path_to_coarse_intervals(
            &fine_path,
            0.6,
            ScaleConsistencyFinePathPartialWindowPolicy::Reject,
        )
        .expect_err("non-partitioning interval pair must fail");
    assert!(matches!(
        invalid_interval_pair_error,
        ScaleConsistencyFinePathAggregationError::IntervalDeltaRatioMismatch { .. }
    ));

    let partial_window_error = aggregate_fine_sufficient_statistics_path_to_coarse_intervals(
        &fine_path,
        0.5,
        ScaleConsistencyFinePathPartialWindowPolicy::Reject,
    )
    .expect_err("reject policy must fail on trailing partial windows");
    assert!(matches!(
        partial_window_error,
        ScaleConsistencyFinePathAggregationError::TrailingPartialWindowRejected {
            trailing_partial_fine_interval_count: 1,
            ..
        }
    ));
}

#[test]
fn fine_path_aggregation_is_deterministic_across_repeated_runs_and_interval_pairs() {
    let fine_path = build_fine_path(
        0.125,
        &[
            (0.1, 0.05, 1, 0, 0.01, -0.01, 0.00, 0.00, 0.00, 0.00),
            (0.2, 0.15, 2, 1, 0.01, -0.01, 0.00, 0.01, 0.00, 0.01),
            (0.3, 0.25, 3, 2, 0.02, -0.01, 0.01, 0.00, 0.00, 0.01),
            (0.4, 0.35, 4, 3, 0.02, -0.02, 0.00, 0.00, -0.01, -0.01),
            (0.5, 0.45, 5, 4, 0.03, -0.02, 0.01, 0.00, 0.00, 0.01),
            (0.6, 0.55, 6, 5, 0.03, -0.03, 0.00, 0.01, 0.00, 0.01),
        ],
    );

    let first_run = aggregate_fine_sufficient_statistics_path_to_coarse_intervals(
        &fine_path,
        0.375,
        ScaleConsistencyFinePathPartialWindowPolicy::Reject,
    )
    .expect("valid three-to-one interval pair should aggregate");
    let second_run = aggregate_fine_sufficient_statistics_path_to_coarse_intervals(
        &fine_path,
        0.375,
        ScaleConsistencyFinePathPartialWindowPolicy::Reject,
    )
    .expect("repeated aggregation should produce the same result");

    assert_eq!(first_run, second_run);
    assert_eq!(
        first_run
            .alignment_metadata
            .fine_subinterval_count_per_coarse_interval,
        3
    );
    assert_eq!(first_run.aggregated_path.records.len(), 2);
}

fn build_fine_path(
    interval_delta: f64,
    records: &[(f64, f64, u64, u64, f64, f64, f64, f64, f64, f64)],
) -> SufficientStatisticsPath {
    let records = records
        .iter()
        .enumerate()
        .map(|(interval_index, record)| build_record(interval_index, *record))
        .collect::<Vec<_>>();

    SufficientStatisticsPath::try_new(
        17,
        interval_delta,
        records.len(),
        build_state(),
        build_state(),
        records,
    )
    .expect("fixture path should validate")
}

fn build_record(
    interval_index: usize,
    record: (f64, f64, u64, u64, f64, f64, f64, f64, f64, f64),
) -> SufficientStatisticsPathRecord {
    let (
        a_plus,
        a_minus,
        n_plus,
        n_minus,
        buy_side_return,
        sell_side_return,
        trading_time_return,
        open_jump_return,
        closed_gap_return,
        total_return,
    ) = record;

    SufficientStatisticsPathRecord::try_new(
        interval_index,
        SufficientStatisticsIntervalStatistics::try_new(a_plus, a_minus, n_plus, n_minus)
            .expect("fixture statistics should validate"),
        SufficientStatisticsReturnComponents::try_new(
            buy_side_return,
            sell_side_return,
            trading_time_return,
            open_jump_return,
            closed_gap_return,
            total_return,
        )
        .expect("fixture return components should validate"),
        total_return,
        total_return,
        total_return,
        0.0,
        0.0,
        total_return,
    )
    .expect("fixture record should validate")
}

fn build_state() -> HawkesExcitationState {
    HawkesExcitationState {
        y_plus_plus: 0.1,
        y_plus_minus: 0.05,
        y_minus_plus: 0.08,
        y_minus_minus: 0.06,
    }
}

fn assert_close(actual: f64, expected: f64) {
    let tolerance = 1.0e-12;
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected}, found {actual}"
    );
}
