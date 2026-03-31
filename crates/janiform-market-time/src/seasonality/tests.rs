use super::{
    DayOfWeekSeasonalityEstimatorSettings, DayOfYearSeasonalityEstimatorSettings,
    DeterministicSeasonalityCompositionError, IntradaySeasonalityEstimationError,
    IntradaySeasonalityEstimatorSettings, IntradaySmoothingKernel, ScheduledEventOverlayDefinition,
    SeasonalityDiagnosticsReportError, SeasonalityDiagnosticsReportInput,
    SeasonalityNormalizationError, compose_deterministic_seasonality_signal,
    estimate_day_of_week_seasonality, estimate_day_of_year_seasonality,
    estimate_intraday_seasonality, generate_seasonality_diagnostics_batch_report,
    generate_seasonality_diagnostics_report, normalize_seasonality_factors_for_identifiability,
    numerics::stable_sum_f64,
};

#[test]
fn intraday_estimator_recovers_non_parametric_bimodal_pattern() {
    let mut minute_of_day_events: Vec<u16> = Vec::new();

    for _ in 0..80 {
        minute_of_day_events.push(570);
    }
    for _ in 0..20 {
        minute_of_day_events.push(720);
    }
    for _ in 0..70 {
        minute_of_day_events.push(930);
    }

    let estimate = estimate_intraday_seasonality(
        &minute_of_day_events,
        IntradaySeasonalityEstimatorSettings {
            minutes_per_bin: 30,
            smoothing_kernel: IntradaySmoothingKernel::Gaussian,
            smoothing_bandwidth_bins: 1.2,
            minimum_bin_events: 1,
            pseudocount: 1e-6,
        },
    )
    .expect("intraday estimation should succeed");

    let open_bin = 570_usize / 30;
    let midday_bin = 720_usize / 30;
    let close_bin = 930_usize / 30;

    assert_eq!(estimate.minutes_per_bin, 30);
    assert!(
        estimate.intraday_factor_by_bin[open_bin] > estimate.intraday_factor_by_bin[midday_bin]
    );
    assert!(
        estimate.intraday_factor_by_bin[close_bin] > estimate.intraday_factor_by_bin[midday_bin]
    );
    assert_eq!(
        estimate.diagnostics.input_event_count,
        minute_of_day_events.len()
    );
}

#[test]
fn intraday_estimator_handles_sparse_bins_and_day_boundary_wrap() {
    let minute_of_day_events: Vec<u16> = vec![0, 0, 1, 1439, 1438, 1439];
    let estimate = estimate_intraday_seasonality(
        &minute_of_day_events,
        IntradaySeasonalityEstimatorSettings {
            minutes_per_bin: 1,
            smoothing_kernel: IntradaySmoothingKernel::Gaussian,
            smoothing_bandwidth_bins: 2.0,
            minimum_bin_events: 2,
            pseudocount: 1e-6,
        },
    )
    .expect("intraday estimation should succeed");

    assert_eq!(estimate.diagnostics.bin_count, 1_440);
    assert!(estimate.diagnostics.sparse_bin_count > 1_000);
    assert!(
        estimate
            .intraday_factor_by_bin
            .iter()
            .all(|value| *value > 0.0)
    );
    assert!(
        (estimate.intraday_factor_by_bin[0] - estimate.intraday_factor_by_bin[1_439]).abs() < 0.3
    );
    assert!(estimate.intraday_factor_by_bin[0] > estimate.intraday_factor_by_bin[720]);
}

#[test]
fn intraday_estimator_is_reproducible_for_identical_inputs() {
    let minute_of_day_events: Vec<u16> = vec![15, 16, 17, 540, 541, 542, 1000, 1001];
    let settings = IntradaySeasonalityEstimatorSettings {
        minutes_per_bin: 5,
        smoothing_kernel: IntradaySmoothingKernel::Uniform,
        smoothing_bandwidth_bins: 2.0,
        minimum_bin_events: 1,
        pseudocount: 1e-6,
    };

    let first_estimate = estimate_intraday_seasonality(&minute_of_day_events, settings)
        .expect("intraday estimation should succeed");
    let second_estimate = estimate_intraday_seasonality(&minute_of_day_events, settings)
        .expect("intraday estimation should succeed");

    assert_eq!(first_estimate, second_estimate);
}

#[test]
fn intraday_estimator_rejects_empty_input() {
    let result = estimate_intraday_seasonality(
        &[],
        IntradaySeasonalityEstimatorSettings {
            minutes_per_bin: 1,
            smoothing_kernel: IntradaySmoothingKernel::Gaussian,
            smoothing_bandwidth_bins: 1.0,
            minimum_bin_events: 1,
            pseudocount: 1e-6,
        },
    );

    assert_eq!(
        result,
        Err(IntradaySeasonalityEstimationError::EmptyInputEvents)
    );
}

#[test]
fn day_of_week_estimator_recovers_synthetic_weekday_pattern() {
    let mut day_of_week_events: Vec<u8> = Vec::new();

    for _ in 0..60 {
        day_of_week_events.push(1);
    }
    for _ in 0..30 {
        day_of_week_events.push(2);
    }
    for _ in 0..10 {
        day_of_week_events.push(3);
    }
    for _ in 0..20 {
        day_of_week_events.push(4);
    }
    for _ in 0..50 {
        day_of_week_events.push(5);
    }

    let estimate = estimate_day_of_week_seasonality(
        &day_of_week_events,
        DayOfWeekSeasonalityEstimatorSettings {
            minimum_bucket_events: 2,
            pseudocount: 1e-6,
        },
    )
    .expect("day-of-week estimation should succeed");

    assert!(estimate.day_of_week_factor[0] > estimate.day_of_week_factor[2]);
    assert!(estimate.day_of_week_factor[4] > estimate.day_of_week_factor[2]);
    assert_eq!(estimate.sparse_bucket_flags[5], 1);
    assert_eq!(estimate.sparse_bucket_flags[6], 1);

    let mean_factor =
        estimate.day_of_week_factor.iter().sum::<f64>() / estimate.day_of_week_factor.len() as f64;

    assert!((mean_factor - 1.0).abs() < 1e-12);
    assert_eq!(
        estimate.diagnostics.input_event_count,
        day_of_week_events.len()
    );
}

#[test]
fn day_of_year_estimator_handles_sparse_day_buckets_with_defined_fallback() {
    let mut day_of_year_events: Vec<u16> = Vec::new();

    for _ in 0..60 {
        day_of_year_events.push(60);
    }
    for _ in 0..20 {
        day_of_year_events.push(200);
    }

    day_of_year_events.push(330);

    let estimate = estimate_day_of_year_seasonality(
        &day_of_year_events,
        DayOfYearSeasonalityEstimatorSettings {
            minimum_bucket_events: 5,
            pseudocount: 1e-6,
        },
    )
    .expect("day-of-year estimation should succeed");

    assert!(estimate.day_of_year_factor[59] > estimate.day_of_year_factor[199]);
    assert_eq!(estimate.sparse_bucket_flags[329], 1);
    assert_eq!(estimate.sparse_bucket_flags[149], 1);
    assert!((estimate.day_of_year_factor[329] - estimate.day_of_year_factor[149]).abs() < 1e-12);

    let mean_factor =
        estimate.day_of_year_factor.iter().sum::<f64>() / estimate.day_of_year_factor.len() as f64;

    assert!((mean_factor - 1.0).abs() < 1e-12);
    assert!(estimate.diagnostics.fallback_bucket_count > 300);
}

#[test]
fn deterministic_seasonality_composition_combines_components_deterministically() {
    let mut intraday_log_component_by_bin: Vec<f64> = vec![0.0; 24];
    intraday_log_component_by_bin[9] = 2.0_f64.ln();
    intraday_log_component_by_bin[15] = 1.5_f64.ln();

    let mut day_of_week_log_component: Vec<f64> = vec![0.0; 7];
    day_of_week_log_component[0] = 1.2_f64.ln();
    day_of_week_log_component[4] = 0.8_f64.ln();

    let mut day_of_year_log_component: Vec<f64> = vec![0.0; 366];
    day_of_year_log_component[31] = 1.1_f64.ln();
    day_of_year_log_component[200] = 0.9_f64.ln();

    let first_output = compose_deterministic_seasonality_signal(
        &[1_000, 2_000],
        &[570, 930],
        &[1, 5],
        &[32, 201],
        60,
        &intraday_log_component_by_bin,
        &day_of_week_log_component,
        &day_of_year_log_component,
        &[],
    )
    .expect("composition should succeed");

    let second_output = compose_deterministic_seasonality_signal(
        &[1_000, 2_000],
        &[570, 930],
        &[1, 5],
        &[32, 201],
        60,
        &intraday_log_component_by_bin,
        &day_of_week_log_component,
        &day_of_year_log_component,
        &[],
    )
    .expect("composition should succeed");

    assert_eq!(first_output, second_output);
    assert_eq!(first_output.intraday_bin_index_by_event, vec![9, 15]);
    assert!((first_output.combined_factor_by_event[0] - (2.64 / 1.86)).abs() < 1e-12);
    assert!((first_output.combined_factor_by_event[1] - (1.08 / 1.86)).abs() < 1e-12);
    assert_eq!(
        first_output
            .diagnostics
            .normalization_metadata
            .normalization_policy,
        "mean_factor_anchor.v1"
    );
    assert_eq!(
        first_output
            .diagnostics
            .normalization_metadata
            .fit_window_event_count,
        2
    );
    assert!(
        (first_output
            .diagnostics
            .normalization_metadata
            .pre_normalization_mean_factor
            - 1.86)
            .abs()
            < 1e-12
    );
    assert!(
        (first_output
            .diagnostics
            .normalization_metadata
            .post_normalization_mean_factor
            - 1.0)
            .abs()
            < 1e-12
    );
}

#[test]
fn deterministic_seasonality_composition_applies_single_and_multiple_overlay_windows() {
    let output = compose_deterministic_seasonality_signal(
        &[1_000, 1_500, 5_000],
        &[570, 600, 630],
        &[1, 1, 1],
        &[32, 32, 32],
        60,
        &vec![0.0; 24],
        &vec![0.0; 7],
        &vec![0.0; 366],
        &[
            ScheduledEventOverlayDefinition {
                overlay_id: "cpi_release".to_string(),
                start_timestamp_utc_nanos: 900,
                end_timestamp_utc_nanos: 1_600,
                log_adjustment: 0.2,
                precedence: 1,
                enabled: true,
                source_label: "macro".to_string(),
            },
            ScheduledEventOverlayDefinition {
                overlay_id: "earnings_call".to_string(),
                start_timestamp_utc_nanos: 4_800,
                end_timestamp_utc_nanos: 5_100,
                log_adjustment: -0.1,
                precedence: 1,
                enabled: true,
                source_label: "corporate".to_string(),
            },
        ],
    )
    .expect("composition with overlays should succeed");

    assert_eq!(output.special_log_component_by_event, vec![0.2, 0.2, -0.1]);
    assert_eq!(output.special_overlay_active_flags_by_event, vec![1, 1, 1]);
    assert_eq!(
        output.active_overlay_ids_by_event,
        vec![
            "cpi_release".to_string(),
            "cpi_release".to_string(),
            "earnings_call".to_string(),
        ]
    );
    assert_eq!(
        output
            .diagnostics
            .overlay_provenance_metadata
            .configured_overlay_count,
        2
    );
    assert_eq!(
        output
            .diagnostics
            .overlay_provenance_metadata
            .active_overlay_event_count,
        3
    );
    assert_eq!(
        output
            .diagnostics
            .overlay_provenance_metadata
            .applied_overlay_ids_in_order,
        vec!["cpi_release".to_string(), "earnings_call".to_string()]
    );
}

#[test]
fn deterministic_seasonality_composition_applies_overlap_precedence_deterministically() {
    let output = compose_deterministic_seasonality_signal(
        &[1_200],
        &[570],
        &[1],
        &[32],
        60,
        &vec![0.0; 24],
        &vec![0.0; 7],
        &vec![0.0; 366],
        &[
            ScheduledEventOverlayDefinition {
                overlay_id: "low_priority".to_string(),
                start_timestamp_utc_nanos: 900,
                end_timestamp_utc_nanos: 1_500,
                log_adjustment: 0.15,
                precedence: 1,
                enabled: true,
                source_label: "macro".to_string(),
            },
            ScheduledEventOverlayDefinition {
                overlay_id: "high_priority".to_string(),
                start_timestamp_utc_nanos: 1_100,
                end_timestamp_utc_nanos: 1_300,
                log_adjustment: 0.3,
                precedence: 2,
                enabled: true,
                source_label: "macro".to_string(),
            },
            ScheduledEventOverlayDefinition {
                overlay_id: "high_priority_peer".to_string(),
                start_timestamp_utc_nanos: 1_100,
                end_timestamp_utc_nanos: 1_300,
                log_adjustment: 0.2,
                precedence: 2,
                enabled: true,
                source_label: "macro".to_string(),
            },
        ],
    )
    .expect("composition with overlap should succeed");

    assert_eq!(output.special_overlay_active_flags_by_event, vec![1]);
    assert_eq!(output.special_log_component_by_event, vec![0.5]);
    assert_eq!(
        output.active_overlay_ids_by_event,
        vec!["high_priority,high_priority_peer".to_string()]
    );
    assert_eq!(
        output
            .diagnostics
            .overlay_provenance_metadata
            .applied_overlay_ids_in_order,
        vec![
            "high_priority".to_string(),
            "high_priority_peer".to_string()
        ]
    );
}

#[test]
fn deterministic_seasonality_composition_is_noop_when_overlays_are_disabled() {
    let without_overlays = compose_deterministic_seasonality_signal(
        &[1_000, 2_000],
        &[570, 630],
        &[1, 1],
        &[32, 32],
        60,
        &vec![0.0; 24],
        &vec![0.0; 7],
        &vec![0.0; 366],
        &[],
    )
    .expect("base composition should succeed");
    let with_disabled_overlay = compose_deterministic_seasonality_signal(
        &[1_000, 2_000],
        &[570, 630],
        &[1, 1],
        &[32, 32],
        60,
        &vec![0.0; 24],
        &vec![0.0; 7],
        &vec![0.0; 366],
        &[ScheduledEventOverlayDefinition {
            overlay_id: "disabled_overlay".to_string(),
            start_timestamp_utc_nanos: 900,
            end_timestamp_utc_nanos: 2_100,
            log_adjustment: 2.0,
            precedence: 99,
            enabled: false,
            source_label: "test".to_string(),
        }],
    )
    .expect("composition with disabled overlay should succeed");

    assert_eq!(
        without_overlays.special_log_component_by_event,
        with_disabled_overlay.special_log_component_by_event
    );
    assert_eq!(
        without_overlays.special_overlay_active_flags_by_event,
        with_disabled_overlay.special_overlay_active_flags_by_event
    );
    assert_eq!(
        without_overlays.combined_log_component_by_event,
        with_disabled_overlay.combined_log_component_by_event
    );
    assert_eq!(
        without_overlays.combined_factor_by_event,
        with_disabled_overlay.combined_factor_by_event
    );
    assert_eq!(
        with_disabled_overlay
            .diagnostics
            .overlay_provenance_metadata
            .active_overlay_event_count,
        0
    );
}

#[test]
fn deterministic_seasonality_composition_rejects_mismatched_input_lengths() {
    let error = compose_deterministic_seasonality_signal(
        &[1_000, 2_000],
        &[570, 930],
        &[1],
        &[32, 201],
        60,
        &vec![0.0; 24],
        &vec![0.0; 7],
        &vec![0.0; 366],
        &[],
    )
    .expect_err("composition should fail for mismatched input lengths");

    assert_eq!(
        error,
        DeterministicSeasonalityCompositionError::MismatchedInputLengths {
            timestamp_utc_nanos_count: 2,
            minute_of_day_count: 2,
            day_of_week_count: 1,
            day_of_year_count: 2,
        }
    );
}

#[test]
fn deterministic_seasonality_composition_rejects_non_finite_log_components() {
    let mut intraday_log_component_by_bin: Vec<f64> = vec![0.0; 24];
    intraday_log_component_by_bin[9] = f64::NAN;

    let error = compose_deterministic_seasonality_signal(
        &[1_000],
        &[570],
        &[1],
        &[32],
        60,
        &intraday_log_component_by_bin,
        &vec![0.0; 7],
        &vec![0.0; 366],
        &[],
    )
    .expect_err("composition should fail for non-finite log components");

    assert_eq!(
        error,
        DeterministicSeasonalityCompositionError::InvalidCombinedLogComponent {
            record_position: 0,
        }
    );
}

#[test]
fn seasonality_normalization_anchors_mean_and_emits_metadata() {
    let factors = vec![2.64, 1.08];
    let logs = vec![2.64_f64.ln(), 1.08_f64.ln()];

    let normalized = normalize_seasonality_factors_for_identifiability(&factors, &logs)
        .expect("normalization should succeed");

    let anchored_mean = normalized.normalized_factor_by_event.iter().sum::<f64>()
        / normalized.normalized_factor_by_event.len() as f64;

    assert!((anchored_mean - 1.0).abs() < 1e-12);
    assert_eq!(
        normalized.normalization_metadata.normalization_policy,
        "mean_factor_anchor.v1"
    );
    assert_eq!(normalized.normalization_metadata.fit_window_event_count, 2);
}

#[test]
fn seasonality_normalization_rejects_invalid_inputs_and_degenerate_windows() {
    let empty_result = normalize_seasonality_factors_for_identifiability(&[], &[]);
    assert_eq!(
        empty_result,
        Err(SeasonalityNormalizationError::EmptyFitWindow)
    );

    let invalid_factor_result =
        normalize_seasonality_factors_for_identifiability(&[1.0, f64::NAN], &[0.0, 0.0]);
    assert_eq!(
        invalid_factor_result,
        Err(SeasonalityNormalizationError::InvalidInputFactor { record_position: 1 })
    );

    let invalid_log_component_result =
        normalize_seasonality_factors_for_identifiability(&[1.0, 2.0], &[0.0, f64::INFINITY]);
    assert_eq!(
        invalid_log_component_result,
        Err(SeasonalityNormalizationError::InvalidInputLogComponent { record_position: 1 })
    );
}

#[test]
fn seasonality_stable_sum_recovers_small_positive_increments_after_large_prefix() {
    let mut values = vec![(1_u64 << 53) as f64];
    values.extend(std::iter::repeat_n(1.0, 1_024));

    let naive_sum = values.iter().sum::<f64>();
    let stable_sum = stable_sum_f64(&values);
    let expected_sum = ((1_u64 << 53) as f64) + 1_024.0;

    assert!(stable_sum > naive_sum);
    assert_eq!(stable_sum, expected_sum);
}

#[test]
fn diagnostics_report_generates_component_metrics_and_reproducibility_metadata() {
    let fixture = build_diagnostics_fixture();
    let report = generate_seasonality_diagnostics_report(SeasonalityDiagnosticsReportInput {
        dataset_id: "dataset_alpha",
        input_schema_version: "seasonality.timestamp_event.v1",
        preprocessed_schema_version: "seasonality.timestamp_event.preprocessed.v1",
        exchange_code: "XNYS",
        exchange_timezone_name: "America/New_York",
        source_record_positions: &fixture.source_record_positions,
        timestamp_utc_nanos: &fixture.timestamp_utc_nanos,
        exchange_local_minute_of_day: &fixture.exchange_local_minute_of_day,
        exchange_local_day_of_week: &fixture.exchange_local_day_of_week,
        exchange_local_day_of_year: &fixture.exchange_local_day_of_year,
        market_state_codes: &fixture.market_state_codes,
        intraday_estimate: &fixture.intraday_estimate,
        day_of_week_estimate: &fixture.day_of_week_estimate,
        day_of_year_estimate: &fixture.day_of_year_estimate,
        composition: &fixture.composition,
        estimator_version: "seasonality_estimator.v1",
    })
    .expect("diagnostics report generation should succeed");

    assert_eq!(report.schema_version, "seasonality.diagnostics.report.v1");
    assert_eq!(report.dataset_id, "dataset_alpha".to_string());
    assert_eq!(report.intraday_component_summary.component_name, "intraday");
    assert_eq!(
        report.day_of_week_component_summary.component_name,
        "day_of_week"
    );
    assert_eq!(
        report.day_of_year_component_summary.component_name,
        "day_of_year"
    );
    assert!(report.fit_quality_metrics.combined_factor_mean > 0.0);
    assert!(report.fit_quality_metrics.normalization_anchor_error <= 1e-12);

    let overlay_summary = report
        .overlay_component_summary
        .expect("overlay summary should exist when overlays are configured");

    assert_eq!(overlay_summary.configured_overlay_count, 1);
    assert_eq!(overlay_summary.active_overlay_event_count, 3);

    assert_eq!(
        report
            .reproducibility_metadata
            .input_snapshot
            .snapshot_fingerprint_algorithm,
        "fnv1a64.v1"
    );
    assert_eq!(
        report.reproducibility_metadata.estimator_version,
        "seasonality_estimator.v1".to_string()
    );
    assert_eq!(
        report
            .reproducibility_metadata
            .input_snapshot
            .input_event_count,
        fixture.timestamp_utc_nanos.len()
    );
    assert_eq!(
        report
            .reproducibility_metadata
            .input_snapshot
            .snapshot_fingerprint_hex
            .len(),
        16
    );
}

#[test]
fn diagnostics_report_rejects_missing_required_metadata_fields() {
    let fixture = build_diagnostics_fixture();

    let error = generate_seasonality_diagnostics_report(SeasonalityDiagnosticsReportInput {
        dataset_id: "",
        input_schema_version: "seasonality.timestamp_event.v1",
        preprocessed_schema_version: "seasonality.timestamp_event.preprocessed.v1",
        exchange_code: "XNYS",
        exchange_timezone_name: "America/New_York",
        source_record_positions: &fixture.source_record_positions,
        timestamp_utc_nanos: &fixture.timestamp_utc_nanos,
        exchange_local_minute_of_day: &fixture.exchange_local_minute_of_day,
        exchange_local_day_of_week: &fixture.exchange_local_day_of_week,
        exchange_local_day_of_year: &fixture.exchange_local_day_of_year,
        market_state_codes: &fixture.market_state_codes,
        intraday_estimate: &fixture.intraday_estimate,
        day_of_week_estimate: &fixture.day_of_week_estimate,
        day_of_year_estimate: &fixture.day_of_year_estimate,
        composition: &fixture.composition,
        estimator_version: "seasonality_estimator.v1",
    })
    .expect_err("diagnostics report should fail when dataset id is empty");

    assert_eq!(error, SeasonalityDiagnosticsReportError::EmptyDatasetId);
}

#[test]
fn diagnostics_report_is_deterministic_for_fixed_inputs() {
    let fixture = build_diagnostics_fixture();

    let first_report = generate_seasonality_diagnostics_report(SeasonalityDiagnosticsReportInput {
        dataset_id: "dataset_alpha",
        input_schema_version: "seasonality.timestamp_event.v1",
        preprocessed_schema_version: "seasonality.timestamp_event.preprocessed.v1",
        exchange_code: "XNYS",
        exchange_timezone_name: "America/New_York",
        source_record_positions: &fixture.source_record_positions,
        timestamp_utc_nanos: &fixture.timestamp_utc_nanos,
        exchange_local_minute_of_day: &fixture.exchange_local_minute_of_day,
        exchange_local_day_of_week: &fixture.exchange_local_day_of_week,
        exchange_local_day_of_year: &fixture.exchange_local_day_of_year,
        market_state_codes: &fixture.market_state_codes,
        intraday_estimate: &fixture.intraday_estimate,
        day_of_week_estimate: &fixture.day_of_week_estimate,
        day_of_year_estimate: &fixture.day_of_year_estimate,
        composition: &fixture.composition,
        estimator_version: "seasonality_estimator.v1",
    })
    .expect("first diagnostics report generation should succeed");

    let second_report =
        generate_seasonality_diagnostics_report(SeasonalityDiagnosticsReportInput {
            dataset_id: "dataset_alpha",
            input_schema_version: "seasonality.timestamp_event.v1",
            preprocessed_schema_version: "seasonality.timestamp_event.preprocessed.v1",
            exchange_code: "XNYS",
            exchange_timezone_name: "America/New_York",
            source_record_positions: &fixture.source_record_positions,
            timestamp_utc_nanos: &fixture.timestamp_utc_nanos,
            exchange_local_minute_of_day: &fixture.exchange_local_minute_of_day,
            exchange_local_day_of_week: &fixture.exchange_local_day_of_week,
            exchange_local_day_of_year: &fixture.exchange_local_day_of_year,
            market_state_codes: &fixture.market_state_codes,
            intraday_estimate: &fixture.intraday_estimate,
            day_of_week_estimate: &fixture.day_of_week_estimate,
            day_of_year_estimate: &fixture.day_of_year_estimate,
            composition: &fixture.composition,
            estimator_version: "seasonality_estimator.v1",
        })
        .expect("second diagnostics report generation should succeed");

    assert_eq!(first_report, second_report);
}

#[test]
fn diagnostics_batch_report_supports_multiple_datasets_and_duplicate_guards() {
    let fixture = build_diagnostics_fixture();

    let first_input = SeasonalityDiagnosticsReportInput {
        dataset_id: "dataset_alpha",
        input_schema_version: "seasonality.timestamp_event.v1",
        preprocessed_schema_version: "seasonality.timestamp_event.preprocessed.v1",
        exchange_code: "XNYS",
        exchange_timezone_name: "America/New_York",
        source_record_positions: &fixture.source_record_positions,
        timestamp_utc_nanos: &fixture.timestamp_utc_nanos,
        exchange_local_minute_of_day: &fixture.exchange_local_minute_of_day,
        exchange_local_day_of_week: &fixture.exchange_local_day_of_week,
        exchange_local_day_of_year: &fixture.exchange_local_day_of_year,
        market_state_codes: &fixture.market_state_codes,
        intraday_estimate: &fixture.intraday_estimate,
        day_of_week_estimate: &fixture.day_of_week_estimate,
        day_of_year_estimate: &fixture.day_of_year_estimate,
        composition: &fixture.composition,
        estimator_version: "seasonality_estimator.v1",
    };

    let second_input = SeasonalityDiagnosticsReportInput {
        dataset_id: "dataset_beta",
        ..first_input
    };

    let batch_report = generate_seasonality_diagnostics_batch_report(&[first_input, second_input])
        .expect("batch diagnostics report generation should succeed");

    assert_eq!(
        batch_report.schema_version,
        "seasonality.diagnostics.batch.v1"
    );
    assert_eq!(batch_report.report_count, 2);
    assert_eq!(
        batch_report.dataset_ids_in_order,
        vec!["dataset_alpha".to_string(), "dataset_beta".to_string()]
    );

    let duplicate_dataset_error =
        generate_seasonality_diagnostics_batch_report(&[first_input, first_input])
            .expect_err("batch diagnostics should reject duplicate dataset ids");

    assert_eq!(
        duplicate_dataset_error,
        SeasonalityDiagnosticsReportError::DuplicateDatasetId {
            dataset_id: "dataset_alpha".to_string()
        }
    );
}

struct DiagnosticsFixture {
    source_record_positions: Vec<u64>,
    timestamp_utc_nanos: Vec<i64>,
    exchange_local_minute_of_day: Vec<u16>,
    exchange_local_day_of_week: Vec<u8>,
    exchange_local_day_of_year: Vec<u16>,
    market_state_codes: Vec<u8>,
    intraday_estimate: super::IntradaySeasonalityEstimateOutput,
    day_of_week_estimate: super::DayOfWeekSeasonalityEstimateOutput,
    day_of_year_estimate: super::DayOfYearSeasonalityEstimateOutput,
    composition: super::DeterministicSeasonalityCompositionOutput,
}

fn build_diagnostics_fixture() -> DiagnosticsFixture {
    let source_record_positions = vec![0, 1, 2, 3, 4, 5];
    let timestamp_utc_nanos = vec![1_000, 2_000, 3_000, 4_000, 5_000, 6_000];
    let exchange_local_minute_of_day = vec![570, 571, 572, 930, 931, 932];
    let exchange_local_day_of_week = vec![1, 1, 2, 2, 5, 5];
    let exchange_local_day_of_year = vec![32, 32, 33, 33, 36, 36];
    let market_state_codes = vec![1, 1, 1, 1, 1, 1];

    let intraday_estimate = estimate_intraday_seasonality(
        &exchange_local_minute_of_day,
        IntradaySeasonalityEstimatorSettings {
            minutes_per_bin: 60,
            smoothing_kernel: IntradaySmoothingKernel::Gaussian,
            smoothing_bandwidth_bins: 1.0,
            minimum_bin_events: 1,
            pseudocount: 1e-6,
        },
    )
    .expect("intraday estimation should succeed for diagnostics fixture");

    let day_of_week_estimate = estimate_day_of_week_seasonality(
        &exchange_local_day_of_week,
        DayOfWeekSeasonalityEstimatorSettings {
            minimum_bucket_events: 1,
            pseudocount: 1e-6,
        },
    )
    .expect("day-of-week estimation should succeed for diagnostics fixture");

    let day_of_year_estimate = estimate_day_of_year_seasonality(
        &exchange_local_day_of_year,
        DayOfYearSeasonalityEstimatorSettings {
            minimum_bucket_events: 1,
            pseudocount: 1e-6,
        },
    )
    .expect("day-of-year estimation should succeed for diagnostics fixture");

    let composition = compose_deterministic_seasonality_signal(
        &timestamp_utc_nanos,
        &exchange_local_minute_of_day,
        &exchange_local_day_of_week,
        &exchange_local_day_of_year,
        intraday_estimate.minutes_per_bin,
        &intraday_estimate.intraday_log_component_by_bin,
        &day_of_week_estimate.day_of_week_log_component,
        &day_of_year_estimate.day_of_year_log_component,
        &[ScheduledEventOverlayDefinition {
            overlay_id: "macro_release".to_string(),
            start_timestamp_utc_nanos: 2_500,
            end_timestamp_utc_nanos: 5_500,
            log_adjustment: 0.2,
            precedence: 1,
            enabled: true,
            source_label: "macro".to_string(),
        }],
    )
    .expect("composition should succeed for diagnostics fixture");

    DiagnosticsFixture {
        source_record_positions,
        timestamp_utc_nanos,
        exchange_local_minute_of_day,
        exchange_local_day_of_week,
        exchange_local_day_of_year,
        market_state_codes,
        intraday_estimate,
        day_of_week_estimate,
        day_of_year_estimate,
        composition,
    }
}
