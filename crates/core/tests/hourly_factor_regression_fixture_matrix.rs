use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use janiform_core::calendar::{ExchangeCalendarRegistry, ExchangeCalendarRegistryArtifact};
use janiform_core::hourly_factors::{
    HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
    HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION, HourlyFactorContractError,
    build_hourly_factor_contract, materialize_hourly_news_intensity_factors,
    materialize_hourly_seasonality_factors,
};
use serde::Deserialize;

const REQUIRED_CASE_TAGS: [&str; 5] = [
    "normal_session",
    "weekend",
    "holiday",
    "daylight_saving_transition",
    "missing_data_guard",
];

const EXPECTED_CANONICAL_FIXTURE_IDS: [&str; 5] = [
    "xnys_normal_session_materialized_baseline_case",
    "xnys_weekend_closed_hours_case",
    "xnys_holiday_closed_hours_case",
    "xnys_spring_forward_transition_case",
    "xnys_fall_back_transition_case",
];

const EXPECTED_MISSING_DATA_GUARD_FIXTURE_IDS: [&str; 4] = [
    "seasonality_sparse_previous_then_global_mean_case",
    "seasonality_sparse_global_mean_leading_interval_case",
    "seasonality_missing_source_failure_case",
    "seasonality_source_length_mismatch_failure_case",
];

#[derive(Debug, Deserialize)]
struct HourlyFactorRegressionFixtureDocument {
    schema_version: String,
    fixture_set_id: String,
    calendar_fixture_set_id: String,
    acceptance_thresholds: HourlyFactorRegressionAcceptanceThresholds,
    canonical_output_cases: Vec<HourlyFactorCanonicalOutputCaseFixture>,
    missing_data_guard_cases: Vec<HourlyFactorMissingDataGuardCaseFixture>,
}

#[derive(Debug, Deserialize)]
struct HourlyFactorRegressionAcceptanceThresholds {
    s_h_absolute_tolerance: f64,
    nu_h_absolute_tolerance: f64,
}

#[derive(Debug, Deserialize)]
struct HourlyFactorCanonicalOutputCaseFixture {
    fixture_id: String,
    exchange_code: String,
    range_start_utc: String,
    range_end_utc: String,
    seasonality_source: HourlySeasonalitySourceFixture,
    news_intensity_source: HourlyNewsIntensitySourceFixture,
    expected_s_h: Vec<f64>,
    expected_nu_h: Vec<f64>,
    expected_skipped_local_hour_interval_count: usize,
    expected_repeated_local_hour_interval_count: usize,
    tags: Vec<String>,
    description: String,
}

#[derive(Debug, Deserialize)]
struct HourlyFactorMissingDataGuardCaseFixture {
    fixture_id: String,
    exchange_code: String,
    range_start_utc: String,
    range_end_utc: String,
    source_timestamps_utc: Vec<String>,
    source_seasonality_factors: Vec<f64>,
    settings: HourlySeasonalitySettingsFixture,
    expected_outcome: HourlyMissingDataGuardExpectedOutcomeFixture,
    tags: Vec<String>,
    description: String,
}

#[derive(Debug, Deserialize)]
struct HourlySeasonalitySettingsFixture {
    interpolation_method: String,
    aggregation_method: String,
    sparse_fallback_policy: String,
    sparse_hour_minimum_observation_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "source_kind", rename_all = "snake_case")]
enum HourlySeasonalitySourceFixture {
    Explicit {
        hourly_factors: Vec<f64>,
        materialization_method: String,
    },
    Materialized {
        source_timestamps_utc: Vec<String>,
        source_seasonality_factors: Vec<f64>,
        interpolation_method: String,
        aggregation_method: String,
        sparse_fallback_policy: String,
        sparse_hour_minimum_observation_count: usize,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "source_kind", rename_all = "snake_case")]
enum HourlyNewsIntensitySourceFixture {
    Explicit {
        hourly_factors: Vec<f64>,
        materialization_method: String,
    },
    Materialized {
        closed_window_baseline_intensity: f64,
        open_window_override_by_interval_index: BTreeMap<String, f64>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "outcome_kind", rename_all = "snake_case")]
enum HourlyMissingDataGuardExpectedOutcomeFixture {
    Success {
        expected_s_h: Vec<f64>,
        expected_sparse_interval_count: usize,
        expected_interpolated_interval_count: usize,
        expected_fallback_interval_count: usize,
    },
    Failure {
        expected_error_variant: String,
        expected_error_message_substring: String,
    },
}

#[derive(Debug, Deserialize)]
struct CalendarRegressionFixtureDocument {
    fixture_set_id: String,
    calendar_registry: ExchangeCalendarRegistryArtifact,
}

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
    let required_tags: BTreeSet<String> = REQUIRED_CASE_TAGS
        .into_iter()
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
    let expected_canonical_fixture_ids: BTreeSet<String> = EXPECTED_CANONICAL_FIXTURE_IDS
        .into_iter()
        .map(ToString::to_string)
        .collect();
    let expected_missing_data_guard_fixture_ids: BTreeSet<String> =
        EXPECTED_MISSING_DATA_GUARD_FIXTURE_IDS
            .into_iter()
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
        .acceptance_thresholds
        .s_h_absolute_tolerance;
    let news_abs_tolerance = fixture_document
        .acceptance_thresholds
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
        .acceptance_thresholds
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
                closed_window_baseline_intensity,
                open_window_override_by_interval_index,
            } => {
                let parsed_open_window_overrides =
                    parse_open_window_overrides(open_window_override_by_interval_index);
                let news_output = materialize_hourly_news_intensity_factors(
                    registry,
                    &fixture_case.exchange_code,
                    range_start_utc,
                    range_end_utc,
                    *closed_window_baseline_intensity,
                    &parsed_open_window_overrides,
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

fn hourly_factor_error_variant_identifier(error: &HourlyFactorContractError) -> &'static str {
    match error {
        HourlyFactorContractError::EmptySeasonalitySourceSignal => "EmptySeasonalitySourceSignal",
        HourlyFactorContractError::SourceLengthMismatch { .. } => "SourceLengthMismatch",
        _ => "Other",
    }
}

fn parse_open_window_overrides(
    raw_open_window_override_by_interval_index: &BTreeMap<String, f64>,
) -> Vec<(u64, f64)> {
    let mut parsed_open_window_overrides: Vec<(u64, f64)> = raw_open_window_override_by_interval_index
        .iter()
        .map(|(raw_interval_index, override_value)| {
            let interval_index = raw_interval_index.parse::<u64>().unwrap_or_else(|error| {
                panic!(
                    "open_window_override_by_interval_index key `{raw_interval_index}` must parse as u64: {error}"
                )
            });
            (interval_index, *override_value)
        })
        .collect();
    parsed_open_window_overrides.sort_by_key(|(interval_index, _)| *interval_index);
    parsed_open_window_overrides
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
    let raw_fixture =
        include_str!("../../../tests/fixtures/hourly_factors/hourly_factor_regression_matrix.json");
    let fixture_document =
        serde_json::from_str::<HourlyFactorRegressionFixtureDocument>(raw_fixture)
            .expect("hourly factor regression fixture file should be valid JSON");

    assert_eq!(
        fixture_document.schema_version,
        "hourly.factor.regression.v1"
    );
    assert_eq!(
        fixture_document.fixture_set_id,
        "hourly_factor_regression_matrix_v1"
    );
    assert_eq!(
        fixture_document.calendar_fixture_set_id,
        "calendar_service_regression_matrix_v1"
    );

    fixture_document
}

fn load_calendar_registry_from_shared_fixture() -> ExchangeCalendarRegistry {
    let raw_calendar_fixture =
        include_str!("../../../tests/fixtures/calendar/calendar_service_regression_matrix.json");
    let calendar_fixture =
        serde_json::from_str::<CalendarRegressionFixtureDocument>(raw_calendar_fixture)
            .expect("shared calendar regression fixture should be valid JSON");
    assert_eq!(
        calendar_fixture.fixture_set_id,
        "calendar_service_regression_matrix_v1"
    );

    ExchangeCalendarRegistry::from_artifact(calendar_fixture.calendar_registry)
        .expect("shared calendar registry fixture should load")
}
