mod support;

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use janiform_core::calendar::ExchangeCalendarRegistry;
use janiform_core::hourly_factors::{
    HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
    HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION, HourlyFactorContractError,
    build_hourly_factor_contract, materialize_hourly_news_intensity_factors,
    materialize_hourly_seasonality_factors,
};

use support::validation_fixture::{
    HourlyFactorCanonicalOutputCaseFixture, HourlyFactorMissingDataGuardCaseFixture,
    HourlyFactorRegressionFactorRangePolicy, HourlyFactorRegressionFixtureDocument,
    HourlyMissingDataGuardExpectedOutcomeFixture, HourlyNewsIntensitySourceFixture,
    HourlySeasonalitySourceFixture, STATE_CLASSIFICATION_STRICTNESS_POLICY,
    load_calendar_regression_fixture_from_manifest,
    load_hourly_factor_regression_fixture_from_manifest,
};

#[test]
fn hourly_factor_regression_fixture_covers_required_case_tags() {
    let fixture_document = load_hourly_factor_regression_fixture_document();
    let observed_tags: BTreeSet<String> = fixture_document
        .canonical_output_cases
        .iter()
        .flat_map(|fixture_case| fixture_case.tags.iter().cloned())
        .chain(
            fixture_document
                .missing_data_guard_cases
                .iter()
                .flat_map(|fixture_case| fixture_case.tags.iter().cloned()),
        )
        .collect();
    let required_tags: BTreeSet<String> = fixture_document
        .canonical_regression_metadata
        .required_tags
        .iter()
        .map(ToString::to_string)
        .collect();

    assert!(required_tags.is_subset(&observed_tags));
}

#[test]
fn hourly_factor_regression_fixture_identifiers_remain_stable() {
    let fixture_document = load_hourly_factor_regression_fixture_document();
    let observed_canonical_fixture_ids: BTreeSet<String> = fixture_document
        .canonical_output_cases
        .iter()
        .map(|fixture_case| fixture_case.fixture_id.clone())
        .collect();
    let observed_missing_data_guard_fixture_ids: BTreeSet<String> = fixture_document
        .missing_data_guard_cases
        .iter()
        .map(|fixture_case| fixture_case.fixture_id.clone())
        .collect();
    let expected_canonical_fixture_ids: BTreeSet<String> = fixture_document
        .canonical_regression_metadata
        .stable_canonical_output_fixture_ids
        .iter()
        .map(ToString::to_string)
        .collect();
    let expected_missing_data_guard_fixture_ids: BTreeSet<String> = fixture_document
        .canonical_regression_metadata
        .stable_missing_data_guard_fixture_ids
        .iter()
        .map(ToString::to_string)
        .collect();

    assert_eq!(
        observed_canonical_fixture_ids,
        expected_canonical_fixture_ids
    );
    assert_eq!(
        observed_missing_data_guard_fixture_ids,
        expected_missing_data_guard_fixture_ids
    );
}

#[test]
fn hourly_factor_regression_canonical_output_cases_match_golden_matrix() {
    let fixture_document = load_hourly_factor_regression_fixture_document();
    let registry = load_calendar_registry_from_shared_fixture();
    let seasonality_abs_tolerance = fixture_document
        .threshold_configuration
        .tolerance_config
        .s_h_absolute_tolerance;
    let news_abs_tolerance = fixture_document
        .threshold_configuration
        .tolerance_config
        .nu_h_absolute_tolerance;

    for fixture_case in &fixture_document.canonical_output_cases {
        let contract_output = build_contract_for_canonical_case(&registry, fixture_case)
            .unwrap_or_else(|error| {
                panic!(
                    "fixture {} ({}) failed unexpectedly: {error}",
                    fixture_case.fixture_id, fixture_case.description
                )
            });

        assert_float_vectors_within_tolerance(
            &contract_output.s_h,
            &fixture_case.expected_s_h,
            seasonality_abs_tolerance,
            &fixture_case.fixture_id,
            "s_h",
        );
        assert_float_vectors_within_tolerance(
            &contract_output.nu_h,
            &fixture_case.expected_nu_h,
            news_abs_tolerance,
            &fixture_case.fixture_id,
            "nu_h",
        );
        assert_eq!(
            contract_output
                .diagnostics
                .skipped_local_hour_interval_count,
            fixture_case.expected_skipped_local_hour_interval_count,
            "fixture {} ({}) skipped_local_hour_interval_count mismatch",
            fixture_case.fixture_id,
            fixture_case.description
        );
        assert_eq!(
            contract_output
                .diagnostics
                .repeated_local_hour_interval_count,
            fixture_case.expected_repeated_local_hour_interval_count,
            "fixture {} ({}) repeated_local_hour_interval_count mismatch",
            fixture_case.fixture_id,
            fixture_case.description
        );
    }
}

#[test]
fn hourly_factor_missing_data_guard_cases_match_defined_outcomes() {
    let fixture_document = load_hourly_factor_regression_fixture_document();
    let registry = load_calendar_registry_from_shared_fixture();
    let seasonality_abs_tolerance = fixture_document
        .threshold_configuration
        .tolerance_config
        .s_h_absolute_tolerance;

    for fixture_case in &fixture_document.missing_data_guard_cases {
        match &fixture_case.expected_outcome {
            HourlyMissingDataGuardExpectedOutcomeFixture::Success {
                expected_s_h,
                expected_sparse_interval_count,
                expected_interpolated_interval_count,
                expected_fallback_interval_count,
            } => {
                let first_output =
                    materialize_seasonality_for_missing_data_case(&registry, fixture_case)
                        .unwrap_or_else(|error| {
                            panic!(
                                "fixture {} ({}) failed unexpectedly: {error}",
                                fixture_case.fixture_id, fixture_case.description
                            )
                        });
                let second_output = materialize_seasonality_for_missing_data_case(
                    &registry,
                    fixture_case,
                )
                .unwrap_or_else(|error| {
                    panic!(
                        "fixture {} ({}) failed unexpectedly on deterministic rerun: {error}",
                        fixture_case.fixture_id, fixture_case.description
                    )
                });

                assert_float_vectors_within_tolerance(
                    &first_output.s_h,
                    expected_s_h,
                    seasonality_abs_tolerance,
                    &fixture_case.fixture_id,
                    "s_h",
                );
                assert_eq!(
                    first_output.diagnostics.sparse_interval_count,
                    *expected_sparse_interval_count
                );
                assert_eq!(
                    first_output.diagnostics.interpolated_interval_count,
                    *expected_interpolated_interval_count
                );
                assert_eq!(
                    first_output.diagnostics.fallback_interval_count,
                    *expected_fallback_interval_count
                );
                assert_eq!(first_output, second_output);
            }
            HourlyMissingDataGuardExpectedOutcomeFixture::Failure {
                expected_error_variant,
                expected_error_message_substring,
            } => {
                let error = materialize_seasonality_for_missing_data_case(&registry, fixture_case)
                    .expect_err("failure case must return error");
                assert_eq!(
                    hourly_factor_error_variant_identifier(&error),
                    expected_error_variant
                );
                assert!(error.to_string().contains(expected_error_message_substring));
            }
        }
    }
}

#[test]
fn hourly_factor_deterministic_fallback_cases_are_repeatable() {
    let fixture_document = load_hourly_factor_regression_fixture_document();
    let registry = load_calendar_registry_from_shared_fixture();
    let deterministic_fallback_cases: Vec<&HourlyFactorMissingDataGuardCaseFixture> =
        fixture_document
            .missing_data_guard_cases
            .iter()
            .filter(|fixture_case| {
                fixture_case
                    .tags
                    .iter()
                    .any(|tag| tag == "deterministic_fallback")
            })
            .collect();

    assert!(!deterministic_fallback_cases.is_empty());

    for fixture_case in deterministic_fallback_cases {
        if let HourlyMissingDataGuardExpectedOutcomeFixture::Success { .. } =
            fixture_case.expected_outcome
        {
            let first_output =
                materialize_seasonality_for_missing_data_case(&registry, fixture_case)
                    .expect("deterministic fallback case should succeed");
            let second_output =
                materialize_seasonality_for_missing_data_case(&registry, fixture_case)
                    .expect("deterministic fallback case should succeed on rerun");

            assert_eq!(first_output, second_output);
        }
    }
}

#[test]
fn hourly_factor_threshold_policy_applies_factor_ranges_to_canonical_cases() {
    let fixture_document = load_hourly_factor_regression_fixture_document();
    let registry = load_calendar_registry_from_shared_fixture();

    assert_eq!(
        fixture_document
            .threshold_configuration
            .state_classification_strictness,
        STATE_CLASSIFICATION_STRICTNESS_POLICY
    );

    for fixture_case in &fixture_document.canonical_output_cases {
        let contract_output = build_contract_for_canonical_case(&registry, fixture_case)
            .unwrap_or_else(|error| {
                panic!(
                    "fixture {} ({}) failed unexpectedly: {error}",
                    fixture_case.fixture_id, fixture_case.description
                )
            });
        let range_policy = factor_range_policy_for_case(&fixture_document, fixture_case);

        assert_float_vector_within_range(
            &contract_output.s_h,
            range_policy.s_h_min,
            range_policy.s_h_max,
            &fixture_case.fixture_id,
            "s_h",
        );
        assert_float_vector_within_range(
            &contract_output.nu_h,
            range_policy.nu_h_min,
            range_policy.nu_h_max,
            &fixture_case.fixture_id,
            "nu_h",
        );
    }
}

#[test]
fn hourly_factor_threshold_policy_range_boundaries_are_inclusive() {
    assert_float_vector_within_range(&[0.9, 1.2], 0.9, 1.2, "boundary_fixture", "s_h");
}

#[test]
#[should_panic(expected = "outside allowed range")]
fn hourly_factor_threshold_policy_range_check_fails_when_exceeded() {
    assert_float_vector_within_range(&[1.2000000001], 0.9, 1.2, "boundary_fixture", "s_h");
}

fn build_contract_for_canonical_case(
    registry: &ExchangeCalendarRegistry,
    fixture_case: &HourlyFactorCanonicalOutputCaseFixture,
) -> Result<janiform_core::hourly_factors::HourlyFactorContractOutput, HourlyFactorContractError> {
    let range_start_utc = parse_utc_timestamp(&fixture_case.range_start_utc);
    let range_end_utc = parse_utc_timestamp(&fixture_case.range_end_utc);

    let (seasonality_hourly_factors, seasonality_materialization_method) =
        match &fixture_case.seasonality_source {
            HourlySeasonalitySourceFixture::Explicit {
                hourly_factors,
                materialization_method,
            } => (hourly_factors.clone(), materialization_method.clone()),
            HourlySeasonalitySourceFixture::Materialized {
                source_timestamps_utc,
                source_seasonality_factors,
                interpolation_method,
                aggregation_method,
                sparse_fallback_policy,
                sparse_hour_minimum_observation_count,
            } => {
                let source_timestamp_utc_nanos: Vec<i64> = source_timestamps_utc
                    .iter()
                    .map(|timestamp| parse_utc_timestamp_to_nanos(timestamp))
                    .collect();
                let seasonality_output = materialize_hourly_seasonality_factors(
                    registry,
                    &fixture_case.exchange_code,
                    range_start_utc,
                    range_end_utc,
                    &source_timestamp_utc_nanos,
                    source_seasonality_factors,
                    interpolation_method,
                    aggregation_method,
                    sparse_fallback_policy,
                    *sparse_hour_minimum_observation_count,
                )?;

                (
                    seasonality_output.s_h,
                    HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION.to_string(),
                )
            }
        };

    let (news_hourly_factors, news_materialization_method) =
        match &fixture_case.news_intensity_source {
            HourlyNewsIntensitySourceFixture::Explicit {
                hourly_factors,
                materialization_method,
            } => (hourly_factors.clone(), materialization_method.clone()),
            HourlyNewsIntensitySourceFixture::Materialized {
                known_scheduled_event_timestamps_utc,
                surprise_baseline_intensity,
                surprise_deterministic_time_multiplier_by_interval_index,
            } => {
                let known_scheduled_event_timestamps_utc_nanos =
                    parse_known_scheduled_event_timestamps_to_nanos(
                        known_scheduled_event_timestamps_utc,
                    );
                let parsed_surprise_time_multipliers = parse_surprise_time_multipliers(
                    surprise_deterministic_time_multiplier_by_interval_index,
                );
                let news_output = materialize_hourly_news_intensity_factors(
                    registry,
                    &fixture_case.exchange_code,
                    range_start_utc,
                    range_end_utc,
                    &known_scheduled_event_timestamps_utc_nanos,
                    *surprise_baseline_intensity,
                    &parsed_surprise_time_multipliers,
                )?;

                (
                    news_output.nu_h,
                    HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION.to_string(),
                )
            }
        };

    build_hourly_factor_contract(
        registry,
        &fixture_case.exchange_code,
        range_start_utc,
        range_end_utc,
        &seasonality_hourly_factors,
        &news_hourly_factors,
        &seasonality_materialization_method,
        &news_materialization_method,
    )
}

fn materialize_seasonality_for_missing_data_case(
    registry: &ExchangeCalendarRegistry,
    fixture_case: &HourlyFactorMissingDataGuardCaseFixture,
) -> Result<
    janiform_core::hourly_factors::HourlySeasonalityMaterializationOutput,
    HourlyFactorContractError,
> {
    let source_timestamp_utc_nanos: Vec<i64> = fixture_case
        .source_timestamps_utc
        .iter()
        .map(|timestamp| parse_utc_timestamp_to_nanos(timestamp))
        .collect();
    let range_start_utc = parse_utc_timestamp(&fixture_case.range_start_utc);
    let range_end_utc = parse_utc_timestamp(&fixture_case.range_end_utc);

    materialize_hourly_seasonality_factors(
        registry,
        &fixture_case.exchange_code,
        range_start_utc,
        range_end_utc,
        &source_timestamp_utc_nanos,
        &fixture_case.source_seasonality_factors,
        &fixture_case.settings.interpolation_method,
        &fixture_case.settings.aggregation_method,
        &fixture_case.settings.sparse_fallback_policy,
        fixture_case.settings.sparse_hour_minimum_observation_count,
    )
}

fn assert_float_vectors_within_tolerance(
    observed: &[f64],
    expected: &[f64],
    absolute_tolerance: f64,
    fixture_id: &str,
    field_name: &str,
) {
    assert_eq!(
        observed.len(),
        expected.len(),
        "fixture {} field {} length mismatch",
        fixture_id,
        field_name
    );

    for value_index in 0..observed.len() {
        let observed_value = observed[value_index];
        let expected_value = expected[value_index];
        let absolute_difference = (observed_value - expected_value).abs();

        assert!(
            absolute_difference <= absolute_tolerance,
            "fixture {} field {} index {} expected {}, observed {}, abs_diff {}, abs_tol {}",
            fixture_id,
            field_name,
            value_index,
            expected_value,
            observed_value,
            absolute_difference,
            absolute_tolerance
        );
    }
}

fn assert_float_vector_within_range(
    observed: &[f64],
    minimum: f64,
    maximum: f64,
    fixture_id: &str,
    field_name: &str,
) {
    for (value_index, observed_value) in observed.iter().enumerate() {
        assert!(
            *observed_value >= minimum && *observed_value <= maximum,
            "fixture {} field {} index {} value {} is outside allowed range [{}, {}]",
            fixture_id,
            field_name,
            value_index,
            observed_value,
            minimum,
            maximum
        );
    }
}

fn hourly_factor_error_variant_identifier(error: &HourlyFactorContractError) -> &'static str {
    match error {
        HourlyFactorContractError::EmptySeasonalitySourceSignal => "EmptySeasonalitySourceSignal",
        HourlyFactorContractError::SourceLengthMismatch { .. } => "SourceLengthMismatch",
        _ => "Other",
    }
}

fn parse_known_scheduled_event_timestamps_to_nanos(
    raw_known_scheduled_event_timestamps_utc: &[String],
) -> Vec<i64> {
    raw_known_scheduled_event_timestamps_utc
        .iter()
        .map(|raw_timestamp_utc| parse_utc_timestamp_to_nanos(raw_timestamp_utc))
        .collect()
}

fn parse_surprise_time_multipliers(
    raw_surprise_deterministic_time_multiplier_by_interval_index: &BTreeMap<String, f64>,
) -> Vec<(u64, f64)> {
    let mut parsed_surprise_time_multipliers: Vec<(u64, f64)> = raw_surprise_deterministic_time_multiplier_by_interval_index
        .iter()
        .map(|(raw_interval_index, multiplier_value)| {
            let parsed_interval_index = raw_interval_index.parse::<u64>().unwrap_or_else(|error| {
                panic!(
                    "surprise_deterministic_time_multiplier_by_interval_index key `{raw_interval_index}` must parse as u64: {error}"
                )
            });
            (parsed_interval_index, *multiplier_value)
        })
        .collect();

    parsed_surprise_time_multipliers.sort_by_key(|(interval_index, _)| *interval_index);
    parsed_surprise_time_multipliers
}

fn factor_range_policy_for_case<'a>(
    fixture_document: &'a HourlyFactorRegressionFixtureDocument,
    fixture_case: &HourlyFactorCanonicalOutputCaseFixture,
) -> &'a HourlyFactorRegressionFactorRangePolicy {
    let matched_fixture_classes: Vec<&str> = fixture_document
        .threshold_configuration
        .factor_range_by_fixture_class
        .keys()
        .filter_map(|fixture_class| {
            if fixture_case.tags.iter().any(|tag| tag == fixture_class) {
                Some(fixture_class.as_str())
            } else {
                None
            }
        })
        .collect();

    assert_eq!(
        matched_fixture_classes.len(),
        1,
        "fixture {} must match exactly one threshold fixture class, matched={:?}",
        fixture_case.fixture_id,
        matched_fixture_classes
    );

    fixture_document
        .threshold_configuration
        .factor_range_by_fixture_class
        .get(matched_fixture_classes[0])
        .expect("matched threshold fixture class should exist")
}

fn parse_utc_timestamp(raw_timestamp: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw_timestamp)
        .unwrap_or_else(|error| {
            panic!("timestamp `{raw_timestamp}` should parse as RFC3339: {error}")
        })
        .with_timezone(&Utc)
}

fn parse_utc_timestamp_to_nanos(raw_timestamp: &str) -> i64 {
    parse_utc_timestamp(raw_timestamp)
        .timestamp_nanos_opt()
        .unwrap_or_else(|| panic!("timestamp `{raw_timestamp}` should convert to nanoseconds"))
}

fn load_hourly_factor_regression_fixture_document() -> HourlyFactorRegressionFixtureDocument {
    load_hourly_factor_regression_fixture_from_manifest()
        .expect("hourly factor regression fixture should load from shared manifest")
        .fixture_document
}

fn load_calendar_registry_from_shared_fixture() -> ExchangeCalendarRegistry {
    let calendar_fixture = load_calendar_regression_fixture_from_manifest()
        .expect("shared calendar regression fixture should load from manifest");

    ExchangeCalendarRegistry::from_artifact(calendar_fixture.fixture_document.calendar_registry)
        .expect("shared calendar registry fixture should load")
}
