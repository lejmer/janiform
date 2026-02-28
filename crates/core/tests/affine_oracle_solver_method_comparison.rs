mod support;

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use janiform_core::affine_oracle::{
    ExponentialAffineTransformInput, ExponentialAffineTransformIntegrationStatus,
    ExponentialAffineTransformIntegratorMethod,
};
use janiform_core::verification::{
    EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_AGREEMENT_POLICY,
    EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_INTERVAL_SELECTION_POLICY,
    EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_OUTPUT_SCHEMA_METADATA_VERSION,
    EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REFERENCE_FIXTURE_POLICY,
    EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REPORT_SCHEMA_VERSION,
    EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_TOLERANCE_POLICY_SCHEMA_VERSION,
    ExponentialAffineTransformMethodComparisonFailureCode,
    ExponentialAffineTransformMethodComparisonOutputDifference,
    ExponentialAffineTransformMethodComparisonTolerancePolicy,
    compare_exponential_affine_transform_integration_methods,
};
use serde::Deserialize;
use serde_json::Value;

use support::validation_fixture::fixtures_root_path;

const AFFINE_ORACLE_METHOD_COMPARISON_FIXTURE_SCHEMA_VERSION: &str =
    "affine_oracle.method_comparison.fixture.v1";
const AFFINE_ORACLE_METHOD_COMPARISON_FIXTURE_SET_ID: &str =
    "affine_oracle_solver_method_comparison_fixture_v1";

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct AffineOracleMethodComparisonFixtureDocument {
    schema_version: String,
    fixture_set_id: String,
    policy: ExponentialAffineTransformMethodComparisonTolerancePolicy,
    canonical_regression_metadata: AffineOracleMethodComparisonCanonicalRegressionMetadata,
    cases: Vec<AffineOracleMethodComparisonCaseFixture>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct AffineOracleMethodComparisonCanonicalRegressionMetadata {
    required_tags: Vec<String>,
    stable_fixture_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct AffineOracleMethodComparisonCaseFixture {
    fixture_id: String,
    input: ExponentialAffineTransformInput,
    expected: AffineOracleMethodComparisonExpectedFixture,
    tags: Vec<String>,
    description: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct AffineOracleMethodComparisonExpectedFixture {
    expected_all_checks_passed: bool,
    expected_tolerance_band_label: String,
    expected_alternative_runs: Vec<AffineOracleMethodComparisonExpectedAlternativeRunFixture>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct AffineOracleMethodComparisonExpectedAlternativeRunFixture {
    integrator_method: ExponentialAffineTransformIntegratorMethod,
    step_count: usize,
    expected_status: ExponentialAffineTransformIntegrationStatus,
    expected_passed: bool,
    expected_failure_codes: Vec<ExponentialAffineTransformMethodComparisonFailureCode>,
    expected_output_difference: Option<AffineOracleMethodComparisonExpectedOutputDifferenceFixture>,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct AffineOracleMethodComparisonExpectedOutputDifferenceFixture {
    c_at_delta_absolute_difference: f64,
    d_component_absolute_difference: f64,
    transform_value_absolute_difference: f64,
    absolute_tolerance: f64,
}

#[test]
fn affine_oracle_method_comparison_fixture_schema_and_metadata_are_valid() {
    let fixture_document = load_affine_oracle_method_comparison_fixture_document();

    assert_eq!(
        fixture_document.schema_version,
        AFFINE_ORACLE_METHOD_COMPARISON_FIXTURE_SCHEMA_VERSION
    );
    assert_eq!(
        fixture_document.fixture_set_id,
        AFFINE_ORACLE_METHOD_COMPARISON_FIXTURE_SET_ID
    );
    assert_eq!(
        fixture_document.policy.schema_version,
        EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_TOLERANCE_POLICY_SCHEMA_VERSION
    );

    validate_fixture_document(&fixture_document)
        .expect("affine oracle method comparison fixture document should validate");
}

#[test]
fn method_comparison_harness_matches_expected_fixture_behavior() {
    let fixture_document = load_affine_oracle_method_comparison_fixture_document();

    for fixture_case in &fixture_document.cases {
        let report = compare_exponential_affine_transform_integration_methods(
            fixture_case.input,
            fixture_document.policy.clone(),
        )
        .unwrap_or_else(|source| {
            panic!(
                "fixture {} ({}) method comparison solve failed: {}",
                fixture_case.fixture_id, fixture_case.description, source
            )
        });

        assert_eq!(
            report.schema_version,
            EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REPORT_SCHEMA_VERSION
        );
        assert_eq!(
            report.output_schema_metadata.schema_version,
            EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_OUTPUT_SCHEMA_METADATA_VERSION
        );
        assert_eq!(
            report.agreement_policy,
            EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_AGREEMENT_POLICY
        );
        assert_eq!(
            report.interval_selection_policy,
            EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_INTERVAL_SELECTION_POLICY
        );
        assert_eq!(
            report.reference_fixture_policy,
            EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REFERENCE_FIXTURE_POLICY
        );

        assert_eq!(
            report.tolerance_band.interval_grid_label,
            fixture_case.expected.expected_tolerance_band_label,
            "fixture {} ({}) selected unexpected tolerance band",
            fixture_case.fixture_id,
            fixture_case.description
        );
        assert_eq!(
            report.all_checks_passed, fixture_case.expected.expected_all_checks_passed,
            "fixture {} ({}) all_checks_passed mismatch",
            fixture_case.fixture_id, fixture_case.description
        );

        for expected_alternative_run in &fixture_case.expected.expected_alternative_runs {
            let observed_alternative_run = report
                .alternative_runs
                .iter()
                .find(|alternative_run| {
                    alternative_run.run_summary.integrator_method
                        == expected_alternative_run.integrator_method
                        && alternative_run.run_summary.step_count
                            == expected_alternative_run.step_count
                })
                .unwrap_or_else(|| {
                    panic!(
                        "fixture {} ({}) missing alternative method {:?} step_count {}",
                        fixture_case.fixture_id,
                        fixture_case.description,
                        expected_alternative_run.integrator_method,
                        expected_alternative_run.step_count
                    )
                });

            assert_eq!(
                observed_alternative_run.run_summary.status,
                expected_alternative_run.expected_status,
                "fixture {} ({}) status mismatch for alternative {:?} step_count {}",
                fixture_case.fixture_id,
                fixture_case.description,
                expected_alternative_run.integrator_method,
                expected_alternative_run.step_count
            );
            assert_eq!(
                observed_alternative_run.passed,
                expected_alternative_run.expected_passed,
                "fixture {} ({}) pass/fail mismatch for alternative {:?} step_count {}",
                fixture_case.fixture_id,
                fixture_case.description,
                expected_alternative_run.integrator_method,
                expected_alternative_run.step_count
            );
            assert_eq!(
                observed_alternative_run.failure_codes,
                expected_alternative_run.expected_failure_codes,
                "fixture {} ({}) failure code mismatch for alternative {:?} step_count {}",
                fixture_case.fixture_id,
                fixture_case.description,
                expected_alternative_run.integrator_method,
                expected_alternative_run.step_count
            );

            assert_output_difference_matches_expected(
                observed_alternative_run.output_difference,
                expected_alternative_run.expected_output_difference,
                &fixture_case.fixture_id,
                &fixture_case.description,
                expected_alternative_run.integrator_method,
                expected_alternative_run.step_count,
            );
        }
    }
}

#[test]
fn method_comparison_reports_are_deterministic_for_fixture_cases() {
    let fixture_document = load_affine_oracle_method_comparison_fixture_document();

    for fixture_case in &fixture_document.cases {
        let first_report = compare_exponential_affine_transform_integration_methods(
            fixture_case.input,
            fixture_document.policy.clone(),
        )
        .unwrap_or_else(|source| {
            panic!(
                "fixture {} ({}) first method comparison solve failed: {}",
                fixture_case.fixture_id, fixture_case.description, source
            )
        });

        let second_report = compare_exponential_affine_transform_integration_methods(
            fixture_case.input,
            fixture_document.policy.clone(),
        )
        .unwrap_or_else(|source| {
            panic!(
                "fixture {} ({}) second method comparison solve failed: {}",
                fixture_case.fixture_id, fixture_case.description, source
            )
        });

        assert_eq!(
            first_report, second_report,
            "fixture {} ({}) produced non-deterministic method-comparison report",
            fixture_case.fixture_id, fixture_case.description
        );
    }
}

#[test]
fn method_comparison_policy_deserialization_rejects_invalid_schema_version() {
    let fixture_payload = load_affine_oracle_method_comparison_fixture_payload();
    let mut fixture_value: Value = serde_json::from_str(&fixture_payload)
        .expect("method comparison fixture payload should parse as JSON");

    fixture_value["policy"]["schema_version"] =
        Value::from("affine_oracle.method_comparison.tolerance_policy.v0");

    let parse_result =
        serde_json::from_value::<AffineOracleMethodComparisonFixtureDocument>(fixture_value);
    let parse_error_text = parse_result
        .expect_err("invalid policy schema version must fail")
        .to_string();
    assert!(
        parse_error_text.contains("unsupported method-comparison tolerance policy schema_version"),
        "unexpected parse error: {parse_error_text}"
    );
}

#[test]
fn method_comparison_policy_deserialization_rejects_non_increasing_delta_bands() {
    let fixture_payload = load_affine_oracle_method_comparison_fixture_payload();
    let mut fixture_value: Value = serde_json::from_str(&fixture_payload)
        .expect("method comparison fixture payload should parse as JSON");

    fixture_value["policy"]["interval_tolerance_bands"][1]["delta_upper_bound"] = Value::from(0.5);

    let parse_result =
        serde_json::from_value::<AffineOracleMethodComparisonFixtureDocument>(fixture_value);
    let parse_error_text = parse_result
        .expect_err("non-increasing delta band bounds must fail")
        .to_string();
    assert!(
        parse_error_text.contains("delta_upper_bound values must be strictly increasing"),
        "unexpected parse error: {parse_error_text}"
    );
}

#[test]
fn method_comparison_policy_deserialization_rejects_change_log_without_current_policy_version() {
    let fixture_payload = load_affine_oracle_method_comparison_fixture_payload();
    let mut fixture_value: Value = serde_json::from_str(&fixture_payload)
        .expect("method comparison fixture payload should parse as JSON");

    fixture_value["policy"]["policy_version"] = Value::from("2.0.0");

    let parse_result =
        serde_json::from_value::<AffineOracleMethodComparisonFixtureDocument>(fixture_value);
    let parse_error_text = parse_result
        .expect_err("change log without current policy version must fail")
        .to_string();
    assert!(
        parse_error_text.contains("change_log must include an entry for policy_version"),
        "unexpected parse error: {parse_error_text}"
    );
}

#[test]
#[ignore = "manual fixture update workflow helper"]
fn print_method_comparison_outputs_for_fixture_update() {
    let fixture_document = load_affine_oracle_method_comparison_fixture_document();

    for fixture_case in &fixture_document.cases {
        let report = compare_exponential_affine_transform_integration_methods(
            fixture_case.input,
            fixture_document.policy.clone(),
        )
        .unwrap_or_else(|source| {
            panic!(
                "fixture {} ({}) method comparison solve failed: {}",
                fixture_case.fixture_id, fixture_case.description, source
            )
        });

        println!("fixture_id: {}", fixture_case.fixture_id);
        for alternative_run in &report.alternative_runs {
            println!(
                "  alternative: method={:?} step_count={} status={:?} passed={} failure_codes={:?}",
                alternative_run.run_summary.integrator_method,
                alternative_run.run_summary.step_count,
                alternative_run.run_summary.status,
                alternative_run.passed,
                alternative_run.failure_codes
            );
            if let Some(output_difference) = alternative_run.output_difference {
                println!(
                    "    output_difference: c={:.17}, d={:.17}, transform={:.17}",
                    output_difference.c_at_delta_absolute_difference,
                    output_difference.d_component_absolute_difference,
                    output_difference.transform_value_absolute_difference
                );
            }
        }
    }
}

fn validate_fixture_document(
    fixture_document: &AffineOracleMethodComparisonFixtureDocument,
) -> Result<(), String> {
    if fixture_document.cases.is_empty() {
        return Err("cases must not be empty".to_string());
    }

    let mut observed_fixture_ids = BTreeSet::new();
    let mut observed_tags = BTreeSet::new();

    for fixture_case in &fixture_document.cases {
        if !observed_fixture_ids.insert(fixture_case.fixture_id.clone()) {
            return Err(format!(
                "fixture_id '{}' appears multiple times in cases",
                fixture_case.fixture_id
            ));
        }

        if fixture_case.description.trim().is_empty() {
            return Err(format!(
                "fixture {} has empty description",
                fixture_case.fixture_id
            ));
        }
        if fixture_case.tags.is_empty() {
            return Err(format!(
                "fixture {} must include at least one tag",
                fixture_case.fixture_id
            ));
        }

        fixture_case.input.validate().map_err(|source| {
            format!(
                "fixture {} has invalid input: {source}",
                fixture_case.fixture_id
            )
        })?;

        if fixture_case
            .expected
            .expected_tolerance_band_label
            .trim()
            .is_empty()
        {
            return Err(format!(
                "fixture {} has empty expected_tolerance_band_label",
                fixture_case.fixture_id
            ));
        }

        if fixture_case.expected.expected_alternative_runs.is_empty() {
            return Err(format!(
                "fixture {} must include at least one expected alternative run",
                fixture_case.fixture_id
            ));
        }

        let mut observed_alternative_identities = BTreeSet::new();
        for expected_alternative_run in &fixture_case.expected.expected_alternative_runs {
            if expected_alternative_run.step_count == 0 {
                return Err(format!(
                    "fixture {} has expected alternative run with step_count=0",
                    fixture_case.fixture_id
                ));
            }

            let alternative_identity = format!(
                "{:?}:{}",
                expected_alternative_run.integrator_method, expected_alternative_run.step_count
            );
            if !observed_alternative_identities.insert(alternative_identity) {
                return Err(format!(
                    "fixture {} contains duplicate expected alternative run configuration",
                    fixture_case.fixture_id
                ));
            }

            if let Some(expected_output_difference) =
                expected_alternative_run.expected_output_difference
            {
                validate_non_negative_finite_metric(
                    expected_output_difference.c_at_delta_absolute_difference,
                    "c_at_delta_absolute_difference",
                    &fixture_case.fixture_id,
                )?;
                validate_non_negative_finite_metric(
                    expected_output_difference.d_component_absolute_difference,
                    "d_component_absolute_difference",
                    &fixture_case.fixture_id,
                )?;
                validate_non_negative_finite_metric(
                    expected_output_difference.transform_value_absolute_difference,
                    "transform_value_absolute_difference",
                    &fixture_case.fixture_id,
                )?;
                if !expected_output_difference.absolute_tolerance.is_finite()
                    || expected_output_difference.absolute_tolerance <= 0.0
                {
                    return Err(format!(
                        "fixture {} has invalid expected_output_difference.absolute_tolerance value {}",
                        fixture_case.fixture_id, expected_output_difference.absolute_tolerance
                    ));
                }
            }
        }

        for fixture_tag in &fixture_case.tags {
            observed_tags.insert(fixture_tag.clone());
        }
    }

    let declared_stable_ids: BTreeSet<String> = fixture_document
        .canonical_regression_metadata
        .stable_fixture_ids
        .iter()
        .cloned()
        .collect();
    if observed_fixture_ids != declared_stable_ids {
        return Err(format!(
            "canonical_regression_metadata.stable_fixture_ids mismatch: observed={:?}, declared={:?}",
            observed_fixture_ids, declared_stable_ids
        ));
    }

    let required_tags: BTreeSet<String> = fixture_document
        .canonical_regression_metadata
        .required_tags
        .iter()
        .cloned()
        .collect();
    if !required_tags.is_subset(&observed_tags) {
        return Err(format!(
            "canonical_regression_metadata.required_tags are not covered by cases: missing={:?}",
            required_tags
                .difference(&observed_tags)
                .cloned()
                .collect::<Vec<String>>()
        ));
    }

    Ok(())
}

fn validate_non_negative_finite_metric(
    value: f64,
    label: &str,
    fixture_id: &str,
) -> Result<(), String> {
    if !value.is_finite() || value < 0.0 {
        return Err(format!(
            "fixture {} has invalid {} value {}",
            fixture_id, label, value
        ));
    }

    Ok(())
}

fn assert_output_difference_matches_expected(
    observed_output_difference: Option<ExponentialAffineTransformMethodComparisonOutputDifference>,
    expected_output_difference: Option<AffineOracleMethodComparisonExpectedOutputDifferenceFixture>,
    fixture_id: &str,
    description: &str,
    integrator_method: ExponentialAffineTransformIntegratorMethod,
    step_count: usize,
) {
    match (observed_output_difference, expected_output_difference) {
        (Some(observed), Some(expected)) => {
            assert!(
                (observed.c_at_delta_absolute_difference - expected.c_at_delta_absolute_difference)
                    .abs()
                    <= expected.absolute_tolerance,
                "fixture {} ({}) c_at_delta_absolute_difference mismatch for alternative {:?} step_count {}",
                fixture_id,
                description,
                integrator_method,
                step_count
            );
            assert!(
                (observed.d_component_absolute_difference
                    - expected.d_component_absolute_difference)
                    .abs()
                    <= expected.absolute_tolerance,
                "fixture {} ({}) d_component_absolute_difference mismatch for alternative {:?} step_count {}",
                fixture_id,
                description,
                integrator_method,
                step_count
            );
            assert!(
                (observed.transform_value_absolute_difference
                    - expected.transform_value_absolute_difference)
                    .abs()
                    <= expected.absolute_tolerance,
                "fixture {} ({}) transform_value_absolute_difference mismatch for alternative {:?} step_count {}",
                fixture_id,
                description,
                integrator_method,
                step_count
            );
        }
        (None, None) => {}
        (Some(_), None) => panic!(
            "fixture {} ({}) expected no output difference for alternative {:?} step_count {}, but report included output_difference",
            fixture_id, description, integrator_method, step_count
        ),
        (None, Some(_)) => panic!(
            "fixture {} ({}) expected output difference for alternative {:?} step_count {}, but report omitted it",
            fixture_id, description, integrator_method, step_count
        ),
    }
}

fn load_affine_oracle_method_comparison_fixture_document()
-> AffineOracleMethodComparisonFixtureDocument {
    let fixture_path = affine_oracle_method_comparison_fixture_path();
    let fixture_payload = load_affine_oracle_method_comparison_fixture_payload();

    serde_json::from_str(&fixture_payload).unwrap_or_else(|source| {
        panic!(
            "failed to parse affine oracle method comparison fixture `{}`: {}",
            fixture_path.display(),
            source
        )
    })
}

fn load_affine_oracle_method_comparison_fixture_payload() -> String {
    let fixture_path = affine_oracle_method_comparison_fixture_path();
    fs::read_to_string(&fixture_path).unwrap_or_else(|source| {
        panic!(
            "failed to read affine oracle method comparison fixture `{}`: {}",
            fixture_path.display(),
            source
        )
    })
}

fn affine_oracle_method_comparison_fixture_path() -> PathBuf {
    fixtures_root_path()
        .join("affine_oracle")
        .join("affine_oracle_solver_method_comparison_fixture.json")
}
