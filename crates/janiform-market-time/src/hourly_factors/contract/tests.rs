use chrono::{DateTime, Utc};
use janiform_foundations::time::NANOS_PER_HOUR;

use crate::calendar::ExchangeCalendarRegistryLoader;
use crate::hourly_factors::{
    DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD, DEFAULT_KNOWN_NEWS_CALENDAR_ANCHOR_POLICY,
    DEFAULT_NEWS_INTENSITY_COMPONENT_AGGREGATION_POLICY,
    DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD, DEFAULT_SURPRISE_NEWS_INTENSITY_CONTROL_POLICY,
    HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION, HOURLY_FACTOR_FALL_BACK_POLICY,
    HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY, HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
    HOURLY_FACTOR_SPRING_FORWARD_POLICY, HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
    HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
    HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION, HourlyFactorContractError,
    HourlyFactorContractPayload, build_hourly_factor_contract, load_hourly_factor_contract,
    materialize_hourly_news_intensity_factors, materialize_hourly_seasonality_factors,
};

const ONE_HOUR_INTERVAL_NANOS: i64 = NANOS_PER_HOUR;

#[test]
fn hourly_factor_contract_builds_expected_schema_and_metadata() {
    let registry = build_calendar_registry();

    let output = build_hourly_factor_contract(
        &registry,
        "XNYS",
        parse_timestamp("2026-02-02T14:00:00Z"),
        parse_timestamp("2026-02-02T17:00:00Z"),
        NANOS_PER_HOUR,
        &[1.0, 1.1, 0.9],
        &[0.4, 0.0, 0.2],
        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
        "calendar_closed_window_baseline.v1",
    )
    .expect("hourly factor contract build should succeed");

    assert_eq!(output.schema_version, HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION);
    assert_eq!(output.exchange_code, "XNYS");
    assert_eq!(output.exchange_timezone_name, "America/New_York");
    assert_eq!(
        output.metadata.interval_boundary_policy,
        HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY
    );
    assert_eq!(
        output.metadata.interval_alignment_policy,
        HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY
    );
    assert_eq!(
        output.metadata.timezone_transition_policy,
        HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY
    );
    assert_eq!(
        output.metadata.spring_forward_policy,
        HOURLY_FACTOR_SPRING_FORWARD_POLICY
    );
    assert_eq!(
        output.metadata.fall_back_policy,
        HOURLY_FACTOR_FALL_BACK_POLICY
    );
    assert_eq!(
        output.metadata.seasonality_materialization_method,
        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD
    );
    assert_eq!(
        output.metadata.news_intensity_materialization_method,
        "calendar_closed_window_baseline.v1"
    );
    assert_eq!(output.diagnostics.interval_count, 3);
    assert_eq!(output.diagnostics.skipped_local_hour_interval_count, 0);
    assert_eq!(output.diagnostics.repeated_local_hour_interval_count, 0);
}

#[test]
fn hourly_factor_contract_intervals_are_consecutive_and_non_overlapping() {
    let registry = build_calendar_registry();

    let output = build_hourly_factor_contract(
        &registry,
        "XNYS",
        parse_timestamp("2026-02-02T14:00:00Z"),
        parse_timestamp("2026-02-02T18:00:00Z"),
        NANOS_PER_HOUR,
        &[1.0, 1.0, 1.0, 1.0],
        &[0.0, 0.0, 0.0, 0.0],
        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
    )
    .expect("hourly factor contract build should succeed");

    assert_eq!(output.interval_indices, vec![0, 1, 2, 3]);

    for interval_position in 1..output.interval_start_utc_nanos.len() {
        assert_eq!(
            output.interval_start_utc_nanos[interval_position],
            output.interval_end_utc_nanos[interval_position - 1]
        );
    }

    for interval_position in 0..output.interval_start_utc_nanos.len() {
        let interval_start = output.interval_start_utc_nanos[interval_position];
        let interval_end = output.interval_end_utc_nanos[interval_position];
        assert!(interval_start < interval_end);
        assert_eq!(interval_end - interval_start, NANOS_PER_HOUR);
    }
}

#[test]
fn hourly_factor_contract_spring_forward_policy_skips_nonexistent_local_hour_labels() {
    let registry = build_calendar_registry();

    let output = build_hourly_factor_contract(
        &registry,
        "XNYS",
        parse_timestamp("2026-03-08T06:00:00Z"),
        parse_timestamp("2026-03-08T09:00:00Z"),
        NANOS_PER_HOUR,
        &[1.0, 1.0, 1.0],
        &[0.0, 0.0, 0.0],
        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
    )
    .expect("hourly factor contract build should succeed");

    assert_eq!(
        output.interval_start_exchange_local,
        vec![
            "2026-03-08T01:00:00-05:00",
            "2026-03-08T03:00:00-04:00",
            "2026-03-08T04:00:00-04:00",
        ]
    );
    assert_eq!(
        output.interval_end_exchange_local,
        vec![
            "2026-03-08T03:00:00-04:00",
            "2026-03-08T04:00:00-04:00",
            "2026-03-08T05:00:00-04:00",
        ]
    );
    assert_eq!(
        output.metadata.spring_forward_policy,
        HOURLY_FACTOR_SPRING_FORWARD_POLICY
    );
    assert_eq!(output.diagnostics.skipped_local_hour_interval_count, 1);
    assert_eq!(output.diagnostics.repeated_local_hour_interval_count, 0);
}

#[test]
fn hourly_factor_contract_fall_back_policy_preserves_repeated_local_hour_labels() {
    let registry = build_calendar_registry();

    let output = build_hourly_factor_contract(
        &registry,
        "XNYS",
        parse_timestamp("2026-11-01T05:00:00Z"),
        parse_timestamp("2026-11-01T08:00:00Z"),
        ONE_HOUR_INTERVAL_NANOS,
        &[1.0, 1.0, 1.0],
        &[0.0, 0.0, 0.0],
        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
    )
    .expect("hourly factor contract build should succeed");

    assert_eq!(
        output.interval_start_exchange_local,
        vec![
            "2026-11-01T01:00:00-04:00",
            "2026-11-01T01:00:00-05:00",
            "2026-11-01T02:00:00-05:00",
        ]
    );
    assert_eq!(
        output.interval_end_exchange_local,
        vec![
            "2026-11-01T01:00:00-05:00",
            "2026-11-01T02:00:00-05:00",
            "2026-11-01T03:00:00-05:00",
        ]
    );
    assert_eq!(
        output.metadata.fall_back_policy,
        HOURLY_FACTOR_FALL_BACK_POLICY
    );
    assert_eq!(output.diagnostics.skipped_local_hour_interval_count, 0);
    assert_eq!(output.diagnostics.repeated_local_hour_interval_count, 1);
}

#[test]
fn hourly_factor_contract_rejects_invalid_ranges_and_unknown_exchange_ids() {
    let registry = build_calendar_registry();

    let non_increasing_range_error = build_hourly_factor_contract(
        &registry,
        "XNYS",
        parse_timestamp("2026-02-02T14:00:00Z"),
        parse_timestamp("2026-02-02T14:00:00Z"),
        ONE_HOUR_INTERVAL_NANOS,
        &[],
        &[],
        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
    )
    .expect_err("non-increasing range must fail");

    assert!(matches!(
        non_increasing_range_error,
        HourlyFactorContractError::NonIncreasingRange { .. }
    ));

    let invalid_interval_grid_error = build_hourly_factor_contract(
        &registry,
        "XNYS",
        parse_timestamp("2026-02-02T14:30:00Z"),
        parse_timestamp("2026-02-02T16:00:00Z"),
        ONE_HOUR_INTERVAL_NANOS,
        &[1.0],
        &[0.0],
        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
    )
    .expect_err("range must be divisible by interval width");

    assert!(matches!(
        invalid_interval_grid_error,
        HourlyFactorContractError::RangeDurationNotDivisibleByInterval { .. }
    ));

    let missing_exchange_error = build_hourly_factor_contract(
        &registry,
        "MISSING",
        parse_timestamp("2026-02-02T14:00:00Z"),
        parse_timestamp("2026-02-02T16:00:00Z"),
        ONE_HOUR_INTERVAL_NANOS,
        &[1.0, 1.0],
        &[0.0, 0.0],
        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
    )
    .expect_err("missing exchange id must fail");

    assert!(matches!(
        missing_exchange_error,
        HourlyFactorContractError::MissingExchangeId { .. }
    ));
}

#[test]
fn seasonality_materializer_handles_sparse_and_missing_intervals_with_fallback_policy() {
    let registry = build_calendar_registry();

    let output = materialize_hourly_seasonality_factors(
        &registry,
        "XNYS",
        parse_timestamp("2026-02-02T13:00:00Z"),
        parse_timestamp("2026-02-02T18:00:00Z"),
        ONE_HOUR_INTERVAL_NANOS,
        &[
            parse_timestamp("2026-02-02T14:05:00Z")
                .timestamp_nanos_opt()
                .unwrap(),
            parse_timestamp("2026-02-02T14:20:00Z")
                .timestamp_nanos_opt()
                .unwrap(),
            parse_timestamp("2026-02-02T15:10:00Z")
                .timestamp_nanos_opt()
                .unwrap(),
            parse_timestamp("2026-02-02T17:15:00Z")
                .timestamp_nanos_opt()
                .unwrap(),
        ],
        &[1.2, 1.0, 0.8, 1.4],
        "previous_observation_carry_forward.v1",
        "mean_of_observations.v1",
        "previous_materialized_then_global_mean.v1",
        2,
    )
    .expect("seasonality materialization should succeed");

    assert_eq!(
        output.schema_version,
        HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION
    );
    assert_eq!(output.s_h, vec![1.1, 1.1, 0.8, 0.8, 1.4]);
    assert_eq!(output.diagnostics.source_point_count, 4);
    assert_eq!(output.diagnostics.interval_count, 5);
    assert_eq!(output.diagnostics.sparse_interval_count, 4);
    assert_eq!(output.diagnostics.interpolated_interval_count, 3);
    assert_eq!(output.diagnostics.fallback_interval_count, 1);
    assert_eq!(
        output.metadata.sparse_fallback_policy,
        "previous_materialized_then_global_mean.v1"
    );
    assert_eq!(
        output.metadata.timezone_transition_policy,
        HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY
    );
    assert_eq!(output.diagnostics.skipped_local_hour_interval_count, 0);
    assert_eq!(output.diagnostics.repeated_local_hour_interval_count, 0);
}

#[test]
fn seasonality_materializer_is_deterministic_for_identical_inputs() {
    let registry = build_calendar_registry();

    let source_timestamp_utc_nanos = vec![
        parse_timestamp("2026-02-02T15:05:00Z")
            .timestamp_nanos_opt()
            .unwrap(),
        parse_timestamp("2026-02-02T14:20:00Z")
            .timestamp_nanos_opt()
            .unwrap(),
        parse_timestamp("2026-02-02T16:10:00Z")
            .timestamp_nanos_opt()
            .unwrap(),
        parse_timestamp("2026-02-02T15:30:00Z")
            .timestamp_nanos_opt()
            .unwrap(),
        parse_timestamp("2026-02-02T14:05:00Z")
            .timestamp_nanos_opt()
            .unwrap(),
        parse_timestamp("2026-02-02T16:50:00Z")
            .timestamp_nanos_opt()
            .unwrap(),
    ];
    let source_seasonality_factors = vec![0.9, 1.2, 1.4, 1.1, 1.0, 1.6];

    let first_output = materialize_hourly_seasonality_factors(
        &registry,
        "XNYS",
        parse_timestamp("2026-02-02T14:00:00Z"),
        parse_timestamp("2026-02-02T17:00:00Z"),
        ONE_HOUR_INTERVAL_NANOS,
        &source_timestamp_utc_nanos,
        &source_seasonality_factors,
        "linear_between_observations.v1",
        "last_observation.v1",
        "global_mean.v1",
        2,
    )
    .expect("seasonality materialization should succeed");

    let second_output = materialize_hourly_seasonality_factors(
        &registry,
        "XNYS",
        parse_timestamp("2026-02-02T14:00:00Z"),
        parse_timestamp("2026-02-02T17:00:00Z"),
        ONE_HOUR_INTERVAL_NANOS,
        &source_timestamp_utc_nanos,
        &source_seasonality_factors,
        "linear_between_observations.v1",
        "last_observation.v1",
        "global_mean.v1",
        2,
    )
    .expect("seasonality materialization should succeed");

    assert_eq!(first_output.s_h, vec![1.2, 1.1, 1.6]);
    assert_eq!(first_output, second_output);
}

#[test]
fn seasonality_materializer_assigns_transition_boundary_observations_deterministically() {
    let registry = build_calendar_registry();

    let spring_forward_output = materialize_hourly_seasonality_factors(
        &registry,
        "XNYS",
        parse_timestamp("2026-03-08T06:00:00Z"),
        parse_timestamp("2026-03-08T09:00:00Z"),
        ONE_HOUR_INTERVAL_NANOS,
        &[
            parse_timestamp("2026-03-08T06:30:00Z")
                .timestamp_nanos_opt()
                .expect("timestamp fixture must be in range"),
            parse_timestamp("2026-03-08T07:00:00Z")
                .timestamp_nanos_opt()
                .expect("timestamp fixture must be in range"),
            parse_timestamp("2026-03-08T08:00:00Z")
                .timestamp_nanos_opt()
                .expect("timestamp fixture must be in range"),
        ],
        &[1.0, 2.0, 3.0],
        "previous_observation_carry_forward.v1",
        "last_observation.v1",
        "global_mean.v1",
        1,
    )
    .expect("seasonality materialization should succeed");

    assert_eq!(spring_forward_output.s_h, vec![1.0, 2.0, 3.0]);
    assert_eq!(
        spring_forward_output
            .diagnostics
            .skipped_local_hour_interval_count,
        1
    );
    assert_eq!(
        spring_forward_output
            .diagnostics
            .repeated_local_hour_interval_count,
        0
    );

    let fall_back_output = materialize_hourly_seasonality_factors(
        &registry,
        "XNYS",
        parse_timestamp("2026-11-01T05:00:00Z"),
        parse_timestamp("2026-11-01T08:00:00Z"),
        ONE_HOUR_INTERVAL_NANOS,
        &[
            parse_timestamp("2026-11-01T05:00:00Z")
                .timestamp_nanos_opt()
                .expect("timestamp fixture must be in range"),
            parse_timestamp("2026-11-01T06:00:00Z")
                .timestamp_nanos_opt()
                .expect("timestamp fixture must be in range"),
            parse_timestamp("2026-11-01T07:00:00Z")
                .timestamp_nanos_opt()
                .expect("timestamp fixture must be in range"),
        ],
        &[1.5, 2.5, 3.5],
        "previous_observation_carry_forward.v1",
        "last_observation.v1",
        "global_mean.v1",
        1,
    )
    .expect("seasonality materialization should succeed");

    assert_eq!(fall_back_output.s_h, vec![1.5, 2.5, 3.5]);
    assert_eq!(
        fall_back_output
            .diagnostics
            .skipped_local_hour_interval_count,
        0
    );
    assert_eq!(
        fall_back_output
            .diagnostics
            .repeated_local_hour_interval_count,
        1
    );
}

#[test]
fn news_intensity_materializer_supports_known_only_component() {
    let registry = build_calendar_registry();

    let output = materialize_hourly_news_intensity_factors(
        &registry,
        "XNYS",
        parse_timestamp("2026-02-02T14:00:00Z"),
        parse_timestamp("2026-02-02T17:00:00Z"),
        ONE_HOUR_INTERVAL_NANOS,
        &[
            parse_timestamp("2026-02-02T14:05:00Z")
                .timestamp_nanos_opt()
                .expect("timestamp fixture must be in range"),
            parse_timestamp("2026-02-02T15:10:00Z")
                .timestamp_nanos_opt()
                .expect("timestamp fixture must be in range"),
        ],
        0.0,
        &[],
    )
    .expect("news intensity materialization should succeed");

    assert_eq!(
        output.schema_version,
        HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION
    );
    assert_eq!(output.known_component_nu_h, vec![1.0, 1.0, 0.0]);
    assert_eq!(output.surprise_component_nu_h, vec![0.0, 0.0, 0.0]);
    assert_eq!(output.nu_h, vec![1.0, 1.0, 0.0]);
    assert_eq!(
        output.metadata.materialization_method,
        DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD
    );
    assert_eq!(
        output.metadata.component_aggregation_policy,
        DEFAULT_NEWS_INTENSITY_COMPONENT_AGGREGATION_POLICY
    );
    assert_eq!(
        output.metadata.known_news_calendar_anchor_policy,
        DEFAULT_KNOWN_NEWS_CALENDAR_ANCHOR_POLICY
    );
    assert_eq!(
        output.metadata.surprise_news_intensity_control_policy,
        DEFAULT_SURPRISE_NEWS_INTENSITY_CONTROL_POLICY
    );
    assert_eq!(output.metadata.known_news_calendar_anchor_count, 2);
    assert_eq!(output.metadata.surprise_baseline_intensity, 0.0);
    assert_eq!(output.diagnostics.interval_count, 3);
    assert_eq!(
        output.diagnostics.known_component_non_zero_interval_count,
        2
    );
    assert_eq!(
        output
            .diagnostics
            .surprise_component_non_zero_interval_count,
        0
    );
    assert_eq!(
        output
            .diagnostics
            .combined_component_non_zero_interval_count,
        2
    );
    assert_eq!(output.diagnostics.known_scheduled_arrival_count, 2);
    assert_eq!(
        output
            .diagnostics
            .surprise_multiplier_override_interval_count,
        0
    );
    assert_eq!(
        output.metadata.timezone_transition_policy,
        HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY
    );
    assert_eq!(output.diagnostics.skipped_local_hour_interval_count, 0);
    assert_eq!(output.diagnostics.repeated_local_hour_interval_count, 0);
}

#[test]
fn news_intensity_materializer_supports_surprise_only_component() {
    let registry = build_calendar_registry();

    let output = materialize_hourly_news_intensity_factors(
        &registry,
        "XNYS",
        parse_timestamp("2026-02-07T00:00:00Z"),
        parse_timestamp("2026-02-07T03:00:00Z"),
        ONE_HOUR_INTERVAL_NANOS,
        &[],
        0.4,
        &[(0, 1.0), (1, 0.5), (2, 2.0)],
    )
    .expect("surprise-only news intensity materialization should succeed");

    assert_eq!(output.known_component_nu_h, vec![0.0, 0.0, 0.0]);
    assert_eq!(output.surprise_component_nu_h, vec![0.4, 0.2, 0.8]);
    assert_eq!(output.nu_h, vec![0.4, 0.2, 0.8]);
    assert_eq!(
        output.diagnostics.known_component_non_zero_interval_count,
        0
    );
    assert_eq!(
        output
            .diagnostics
            .surprise_component_non_zero_interval_count,
        3
    );
    assert_eq!(
        output
            .diagnostics
            .combined_component_non_zero_interval_count,
        3
    );
    assert_eq!(
        output
            .diagnostics
            .surprise_multiplier_override_interval_count,
        3
    );
}

#[test]
fn news_intensity_materializer_supports_combined_known_and_surprise_components() {
    let registry = build_calendar_registry();

    let output = materialize_hourly_news_intensity_factors(
        &registry,
        "XNYS",
        parse_timestamp("2026-02-02T14:00:00Z"),
        parse_timestamp("2026-02-02T17:00:00Z"),
        ONE_HOUR_INTERVAL_NANOS,
        &[
            parse_timestamp("2026-02-02T14:05:00Z")
                .timestamp_nanos_opt()
                .expect("timestamp fixture must be in range"),
            parse_timestamp("2026-02-02T15:10:00Z")
                .timestamp_nanos_opt()
                .expect("timestamp fixture must be in range"),
        ],
        0.1,
        &[(0, 1.0), (1, 2.0), (2, 1.0)],
    )
    .expect("combined known and surprise materialization should succeed");

    assert_eq!(output.known_component_nu_h, vec![1.0, 1.0, 0.0]);
    assert_eq!(output.surprise_component_nu_h, vec![0.1, 0.2, 0.1]);
    assert_eq!(output.nu_h, vec![1.1, 1.2, 0.1]);
    assert_eq!(
        output.diagnostics.known_component_non_zero_interval_count,
        2
    );
    assert_eq!(
        output
            .diagnostics
            .surprise_component_non_zero_interval_count,
        3
    );
    assert_eq!(
        output
            .diagnostics
            .combined_component_non_zero_interval_count,
        3
    );
}

#[test]
fn news_intensity_materializer_normalizes_known_news_to_per_hour_intensity() {
    let registry = build_calendar_registry();

    let output = materialize_hourly_news_intensity_factors(
        &registry,
        "XNYS",
        parse_timestamp("2026-02-02T14:00:00Z"),
        parse_timestamp("2026-02-02T15:00:00Z"),
        ONE_HOUR_INTERVAL_NANOS / 2,
        &[parse_timestamp("2026-02-02T14:05:00Z")
            .timestamp_nanos_opt()
            .expect("timestamp fixture must be in range")],
        0.0,
        &[],
    )
    .expect("sub-hour known-news materialization should succeed");

    assert_eq!(output.known_component_nu_h, vec![2.0, 0.0]);
    assert_eq!(output.surprise_component_nu_h, vec![0.0, 0.0]);
    assert_eq!(output.nu_h, vec![2.0, 0.0]);
}

#[test]
fn news_intensity_materializer_rejects_non_increasing_known_calendar_anchor_input() {
    let registry = build_calendar_registry();

    let non_increasing_error = materialize_hourly_news_intensity_factors(
        &registry,
        "XNYS",
        parse_timestamp("2026-02-02T14:00:00Z"),
        parse_timestamp("2026-02-02T16:00:00Z"),
        ONE_HOUR_INTERVAL_NANOS,
        &[
            parse_timestamp("2026-02-02T15:10:00Z")
                .timestamp_nanos_opt()
                .expect("timestamp fixture must be in range"),
            parse_timestamp("2026-02-02T14:05:00Z")
                .timestamp_nanos_opt()
                .expect("timestamp fixture must be in range"),
        ],
        0.4,
        &[],
    )
    .expect_err("non-increasing known timestamps must fail");

    assert!(matches!(
        non_increasing_error,
        HourlyFactorContractError::KnownScheduledTimestampsNotStrictlyIncreasing { .. }
    ));
}

#[test]
fn news_intensity_materializer_rejects_invalid_surprise_intensity_controls() {
    let registry = build_calendar_registry();

    let baseline_error = materialize_hourly_news_intensity_factors(
        &registry,
        "XNYS",
        parse_timestamp("2026-02-02T14:00:00Z"),
        parse_timestamp("2026-02-02T16:00:00Z"),
        ONE_HOUR_INTERVAL_NANOS,
        &[],
        -0.1,
        &[],
    )
    .expect_err("negative surprise baseline must fail");

    assert_eq!(
        baseline_error,
        HourlyFactorContractError::InvalidSurpriseBaselineIntensity { value: -0.1 }
    );

    let multiplier_error = materialize_hourly_news_intensity_factors(
        &registry,
        "XNYS",
        parse_timestamp("2026-02-02T14:00:00Z"),
        parse_timestamp("2026-02-02T16:00:00Z"),
        ONE_HOUR_INTERVAL_NANOS,
        &[],
        0.5,
        &[(0, -0.2)],
    )
    .expect_err("negative surprise multiplier must fail");

    assert_eq!(
        multiplier_error,
        HourlyFactorContractError::InvalidSurpriseDeterministicTimeMultiplier {
            interval_index: 0,
            value: -0.2,
        }
    );
    assert!(matches!(
        materialize_hourly_news_intensity_factors(
            &registry,
            "XNYS",
            parse_timestamp("2026-02-02T14:00:00Z"),
            parse_timestamp("2026-02-02T16:00:00Z"),
            ONE_HOUR_INTERVAL_NANOS,
            &[],
            0.5,
            &[(2, 1.0)],
        )
        .expect_err("out-of-range surprise multiplier interval index must fail"),
        HourlyFactorContractError::SurpriseDeterministicTimeMultiplierIntervalOutOfRange {
            interval_index: 2,
            interval_count: 2
        }
    ));
}

#[test]
fn hourly_factor_contract_loader_rejects_mismatched_interval_lengths() {
    let error = load_hourly_factor_contract(valid_hourly_factor_contract_payload_with_overrides(
        &[("nu_h", serde_json::json!([0.4]))],
    ))
    .expect_err("malformed interval payload must fail");

    assert!(matches!(
        error,
        HourlyFactorContractError::IntervalFieldLengthMismatch {
            field: "nu_h",
            expected_length: 2,
            actual_length: 1,
        }
    ));
}

#[test]
fn hourly_factor_contract_loader_rejects_inconsistent_interval_grid_metadata() {
    let error =
        load_hourly_factor_contract(valid_hourly_factor_contract_payload_with_overrides(&[(
            "last_interval_end_utc_nanos",
            serde_json::json!(7_300_000_000_000_i64),
        )]))
        .expect_err("inconsistent interval metadata must fail");

    assert!(matches!(
        error,
        HourlyFactorContractError::IntervalStartMismatch {
            field: "last_interval_end_utc_nanos",
            expected: 7_300_000_000_000_i64,
            actual: 7_200_000_000_000_i64,
        }
    ));
}

#[test]
fn hourly_factor_contract_loader_accepts_zero_seasonality_factors() {
    let output = load_hourly_factor_contract(valid_hourly_factor_contract_payload_with_overrides(
        &[("s_h", serde_json::json!([0.0, 0.9]))],
    ))
    .expect("zero seasonality factors should remain valid contract values");

    assert_eq!(output.s_h, vec![0.0, 0.9]);
}

#[test]
fn hourly_factor_contract_loader_accepts_empty_interval_grid_payload() {
    let output =
        load_hourly_factor_contract(valid_hourly_factor_contract_payload_with_overrides(&[
            ("request_end_utc_nanos", serde_json::json!(0_i64)),
            ("interval_indices", serde_json::json!([])),
            ("interval_start_utc_nanos", serde_json::json!([])),
            ("interval_end_utc_nanos", serde_json::json!([])),
            ("interval_start_exchange_local", serde_json::json!([])),
            ("interval_end_exchange_local", serde_json::json!([])),
            ("s_h", serde_json::json!([])),
            ("known_component_nu_h", serde_json::json!([])),
            ("surprise_component_nu_h", serde_json::json!([])),
            ("nu_h", serde_json::json!([])),
            ("interval_count", serde_json::json!(0_usize)),
            ("last_interval_end_utc_nanos", serde_json::json!(0_i64)),
        ]))
        .expect("empty interval grid payload should remain structurally valid");

    assert_eq!(output.diagnostics.interval_count, 0);
    assert!(output.interval_indices.is_empty());
    assert!(output.s_h.is_empty());
}

#[test]
fn hourly_factor_contract_loader_accepts_canonical_payload() {
    let output =
        load_hourly_factor_contract(valid_hourly_factor_contract_payload_with_overrides(&[]))
            .expect("canonical hourly factor payload should load");

    assert_eq!(output.interval_indices, vec![0, 1]);
    assert_eq!(output.s_h, vec![1.0, 0.9]);
    assert_eq!(output.known_component_nu_h, vec![0.1, 0.0]);
    assert_eq!(output.surprise_component_nu_h, vec![0.3, 0.2]);
}

fn build_calendar_registry() -> crate::calendar::ExchangeCalendarRegistry {
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
                  "holiday_dates": [],
                  "early_close_sessions": []
                }
              ]
            }
            "#,
    )
    .expect("fixture calendar registry should load")
}

fn parse_timestamp(raw_timestamp: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw_timestamp)
        .expect("timestamp fixture must parse")
        .with_timezone(&Utc)
}

fn valid_hourly_factor_contract_payload_with_overrides(
    overrides: &[(&str, serde_json::Value)],
) -> HourlyFactorContractPayload {
    let mut payload = serde_json::json!({
        "schema_version": HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
        "exchange_code": "XNYS",
        "exchange_timezone_name": "America/New_York",
        "request_start_utc_nanos": 0_i64,
        "request_end_utc_nanos": 7_200_000_000_000_i64,
        "interval_duration_nanos": 3_600_000_000_000_i64,
        "interval_indices": [0_u64, 1_u64],
        "interval_start_utc_nanos": [0_i64, 3_600_000_000_000_i64],
        "interval_end_utc_nanos": [3_600_000_000_000_i64, 7_200_000_000_000_i64],
        "interval_start_exchange_local": [
            "2026-02-02T09:00:00-05:00",
            "2026-02-02T10:00:00-05:00"
        ],
        "interval_end_exchange_local": [
            "2026-02-02T10:00:00-05:00",
            "2026-02-02T11:00:00-05:00"
        ],
        "s_h": [1.0, 0.9],
        "known_component_nu_h": [0.1, 0.0],
        "surprise_component_nu_h": [0.3, 0.2],
        "nu_h": [0.4, 0.2],
        "interval_count": 2,
        "first_interval_start_utc_nanos": 0_i64,
        "last_interval_end_utc_nanos": 7_200_000_000_000_i64,
        "skipped_local_hour_interval_count": 0,
        "repeated_local_hour_interval_count": 0,
        "interval_boundary_policy": HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
        "interval_alignment_policy": HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY,
        "timezone_transition_policy": HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
        "spring_forward_policy": HOURLY_FACTOR_SPRING_FORWARD_POLICY,
        "fall_back_policy": HOURLY_FACTOR_FALL_BACK_POLICY,
        "seasonality_materialization_method": DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
        "news_intensity_materialization_method": DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD
    });

    let payload_object = payload
        .as_object_mut()
        .expect("payload template must be a JSON object");
    for (field, value) in overrides {
        payload_object.insert((*field).to_string(), value.clone());
    }

    serde_json::from_value(payload).expect("payload template must deserialize")
}
