mod support;

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use janiform_affine_oracle::affine_oracle::{
    EXPONENTIAL_AFFINE_TRANSFORM_DEFAULT_SOLVER_POLICY,
    EXPONENTIAL_AFFINE_TRANSFORM_EQUATION_MAPPING_POLICY, ExponentialAffineTransformInput,
    ExponentialAffineTransformIntegrationStatus, ExponentialAffineTransformIntegratorMethod,
    ExponentialAffineTransformOutput, ExponentialAffineTransformSolverSettings,
    solve_exponential_affine_transform_with_settings,
};
use janiform_foundations::complex_math::ComplexScalar;
use serde::Deserialize;
use serde_json::Value;

use support::validation_fixture::fixtures_root_path;

const AFFINE_ORACLE_SOLVER_REFERENCE_FIXTURE_SCHEMA_VERSION: &str =
    "affine_oracle.solver.reference_fixture.v1";
const AFFINE_ORACLE_SOLVER_REFERENCE_FIXTURE_SET_ID: &str =
    "affine_oracle_solver_reference_fixture_v1";
const AFFINE_ORACLE_REFERENCE_FIXTURE_LOADING_ORDER_POLICY: &str =
    "preserve_declared_case_order.v1";
const AFFINE_ORACLE_REFERENCE_CONTENT_HASH_SCOPE_POLICY: &str = "document_payload_bytes.v1";
const AFFINE_ORACLE_REFERENCE_CONTENT_HASH_ALGORITHM_POLICY: &str = "fnv1a_64_hex_utf8.v1";
const AFFINE_ORACLE_REFERENCE_OUTPUT_DETERMINISM_POLICY: &str =
    "fixed_solver_settings_and_inputs.v1";

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct AffineOracleSolverReferenceFixtureDocument {
    schema_version: String,
    fixture_set_id: String,
    equation_mapping_policy: String,
    default_solver_policy: String,
    determinism_metadata: AffineOracleReferenceDeterminismMetadata,
    canonical_regression_metadata: AffineOracleReferenceCanonicalRegressionMetadata,
    cases: Vec<AffineOracleSolverReferenceCaseFixture>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct AffineOracleReferenceDeterminismMetadata {
    fixture_loading_order: String,
    content_hash_scope: String,
    content_hash_algorithm: String,
    solver_output_determinism: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct AffineOracleReferenceCanonicalRegressionMetadata {
    required_tags: Vec<String>,
    stable_fixture_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct AffineOracleSolverReferenceCaseFixture {
    fixture_id: String,
    solver_settings: AffineOracleSolverSettingsFixture,
    input: ExponentialAffineTransformInput,
    expected_status: AffineOracleExpectedStatusFixture,
    expected_output: ExponentialAffineTransformOutput,
    tolerances: AffineOracleOutputToleranceFixture,
    tags: Vec<String>,
    description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AffineOracleIntegratorMethodFixture {
    FourthOrderRungeKutta,
    ForwardEuler,
}

impl AffineOracleIntegratorMethodFixture {
    fn into_core(self) -> ExponentialAffineTransformIntegratorMethod {
        match self {
            Self::FourthOrderRungeKutta => {
                ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta
            }
            Self::ForwardEuler => ExponentialAffineTransformIntegratorMethod::ForwardEuler,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AffineOracleExpectedStatusFixture {
    Completed,
    DomainGuardViolation,
    BlowupTerminated,
}

impl AffineOracleExpectedStatusFixture {
    fn into_core(self) -> ExponentialAffineTransformIntegrationStatus {
        match self {
            Self::Completed => ExponentialAffineTransformIntegrationStatus::Completed,
            Self::DomainGuardViolation => {
                ExponentialAffineTransformIntegrationStatus::DomainGuardViolation
            }
            Self::BlowupTerminated => ExponentialAffineTransformIntegrationStatus::BlowupTerminated,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct AffineOracleSolverSettingsFixture {
    integrator_method: AffineOracleIntegratorMethodFixture,
    step_count: usize,
}

impl AffineOracleSolverSettingsFixture {
    fn to_core(self, fixture_id: &str) -> ExponentialAffineTransformSolverSettings {
        ExponentialAffineTransformSolverSettings::try_new(
            self.integrator_method.into_core(),
            self.step_count,
        )
        .unwrap_or_else(|source| {
            panic!(
                "fixture {} has invalid solver settings (step_count={}): {}",
                fixture_id, self.step_count, source
            )
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct AffineOracleOutputToleranceFixture {
    c_at_delta_absolute_tolerance: f64,
    d_component_absolute_tolerance: f64,
    transform_value_absolute_tolerance: f64,
}

impl AffineOracleOutputToleranceFixture {
    fn validate(self, fixture_id: &str) -> Result<(), String> {
        validate_positive_finite_tolerance(
            fixture_id,
            "c_at_delta_absolute_tolerance",
            self.c_at_delta_absolute_tolerance,
        )?;
        validate_positive_finite_tolerance(
            fixture_id,
            "d_component_absolute_tolerance",
            self.d_component_absolute_tolerance,
        )?;
        validate_positive_finite_tolerance(
            fixture_id,
            "transform_value_absolute_tolerance",
            self.transform_value_absolute_tolerance,
        )?;

        Ok(())
    }
}

#[test]
fn affine_oracle_reference_fixture_schema_and_metadata_are_valid() {
    let fixture_document = load_affine_oracle_solver_reference_fixture_document();

    assert_eq!(
        fixture_document.schema_version,
        AFFINE_ORACLE_SOLVER_REFERENCE_FIXTURE_SCHEMA_VERSION
    );
    assert_eq!(
        fixture_document.fixture_set_id,
        AFFINE_ORACLE_SOLVER_REFERENCE_FIXTURE_SET_ID
    );
    assert_eq!(
        fixture_document.equation_mapping_policy,
        EXPONENTIAL_AFFINE_TRANSFORM_EQUATION_MAPPING_POLICY
    );
    assert_eq!(
        fixture_document.default_solver_policy,
        EXPONENTIAL_AFFINE_TRANSFORM_DEFAULT_SOLVER_POLICY
    );
    assert_eq!(
        fixture_document.determinism_metadata.fixture_loading_order,
        AFFINE_ORACLE_REFERENCE_FIXTURE_LOADING_ORDER_POLICY
    );
    assert_eq!(
        fixture_document.determinism_metadata.content_hash_scope,
        AFFINE_ORACLE_REFERENCE_CONTENT_HASH_SCOPE_POLICY
    );
    assert_eq!(
        fixture_document.determinism_metadata.content_hash_algorithm,
        AFFINE_ORACLE_REFERENCE_CONTENT_HASH_ALGORITHM_POLICY
    );
    assert_eq!(
        fixture_document
            .determinism_metadata
            .solver_output_determinism,
        AFFINE_ORACLE_REFERENCE_OUTPUT_DETERMINISM_POLICY
    );

    validate_fixture_document(&fixture_document)
        .expect("affine oracle reference fixture document should validate");
}

#[test]
fn affine_oracle_solver_runs_are_deterministic_for_fixed_reference_cases() {
    let fixture_document = load_affine_oracle_solver_reference_fixture_document();

    for fixture_case in &fixture_document.cases {
        let solver_settings = fixture_case
            .solver_settings
            .to_core(&fixture_case.fixture_id);

        let first_result =
            solve_exponential_affine_transform_with_settings(fixture_case.input, solver_settings)
                .unwrap_or_else(|source| {
                    panic!(
                        "fixture {} ({}) first solve failed: {}",
                        fixture_case.fixture_id, fixture_case.description, source
                    )
                });
        let second_result =
            solve_exponential_affine_transform_with_settings(fixture_case.input, solver_settings)
                .unwrap_or_else(|source| {
                    panic!(
                        "fixture {} ({}) second solve failed: {}",
                        fixture_case.fixture_id, fixture_case.description, source
                    )
                });

        assert_eq!(
            first_result, second_result,
            "fixture {} ({}) produced non-deterministic solver output",
            fixture_case.fixture_id, fixture_case.description
        );
        assert_eq!(
            first_result.status,
            fixture_case.expected_status.into_core(),
            "fixture {} ({}) status drifted from expected fixture status",
            fixture_case.fixture_id,
            fixture_case.description
        );
    }
}

#[test]
fn affine_oracle_reference_cases_match_expected_outputs_within_tolerance() {
    let fixture_document = load_affine_oracle_solver_reference_fixture_document();

    for fixture_case in &fixture_document.cases {
        let solver_settings = fixture_case
            .solver_settings
            .to_core(&fixture_case.fixture_id);
        let observed_result =
            solve_exponential_affine_transform_with_settings(fixture_case.input, solver_settings)
                .unwrap_or_else(|source| {
                    panic!(
                        "fixture {} ({}) solve failed: {}",
                        fixture_case.fixture_id, fixture_case.description, source
                    )
                });

        assert_eq!(
            observed_result.status,
            fixture_case.expected_status.into_core(),
            "fixture {} ({}) status mismatch",
            fixture_case.fixture_id,
            fixture_case.description
        );

        let observed_output = observed_result.output.unwrap_or_else(|| {
            panic!(
                "fixture {} ({}) expected output but solver returned none",
                fixture_case.fixture_id, fixture_case.description
            )
        });

        assert_complex_within_tolerance(
            observed_output.c_at_delta,
            fixture_case.expected_output.c_at_delta,
            fixture_case.tolerances.c_at_delta_absolute_tolerance,
            &fixture_case.fixture_id,
            "c_at_delta",
            &fixture_case.description,
        );
        assert_complex_within_tolerance(
            observed_output.d_at_delta.plus_plus,
            fixture_case.expected_output.d_at_delta.plus_plus,
            fixture_case.tolerances.d_component_absolute_tolerance,
            &fixture_case.fixture_id,
            "d_at_delta.plus_plus",
            &fixture_case.description,
        );
        assert_complex_within_tolerance(
            observed_output.d_at_delta.plus_minus,
            fixture_case.expected_output.d_at_delta.plus_minus,
            fixture_case.tolerances.d_component_absolute_tolerance,
            &fixture_case.fixture_id,
            "d_at_delta.plus_minus",
            &fixture_case.description,
        );
        assert_complex_within_tolerance(
            observed_output.d_at_delta.minus_plus,
            fixture_case.expected_output.d_at_delta.minus_plus,
            fixture_case.tolerances.d_component_absolute_tolerance,
            &fixture_case.fixture_id,
            "d_at_delta.minus_plus",
            &fixture_case.description,
        );
        assert_complex_within_tolerance(
            observed_output.d_at_delta.minus_minus,
            fixture_case.expected_output.d_at_delta.minus_minus,
            fixture_case.tolerances.d_component_absolute_tolerance,
            &fixture_case.fixture_id,
            "d_at_delta.minus_minus",
            &fixture_case.description,
        );
        assert_complex_within_tolerance(
            observed_output.transform_value,
            fixture_case.expected_output.transform_value,
            fixture_case.tolerances.transform_value_absolute_tolerance,
            &fixture_case.fixture_id,
            "transform_value",
            &fixture_case.description,
        );
    }
}

#[test]
fn affine_oracle_reference_fixture_rejects_missing_required_metadata() {
    let fixture_payload = load_affine_oracle_solver_reference_fixture_payload();
    let mut fixture_value: Value = serde_json::from_str(&fixture_payload)
        .expect("reference fixture payload should parse as JSON");

    fixture_value
        .as_object_mut()
        .expect("fixture document root should be an object")
        .remove("determinism_metadata");

    let parse_result =
        serde_json::from_value::<AffineOracleSolverReferenceFixtureDocument>(fixture_value);

    assert!(parse_result.is_err());
}

#[test]
fn affine_oracle_reference_fixture_validation_rejects_non_positive_tolerance_values() {
    let fixture_payload = load_affine_oracle_solver_reference_fixture_payload();
    let mut fixture_value: Value = serde_json::from_str(&fixture_payload)
        .expect("reference fixture payload should parse as JSON");

    fixture_value["cases"][0]["tolerances"]["c_at_delta_absolute_tolerance"] = Value::from(0.0);

    let parsed_fixture =
        serde_json::from_value::<AffineOracleSolverReferenceFixtureDocument>(fixture_value)
            .expect("fixture with non-positive tolerance should still deserialize");

    let validation_result = validate_fixture_document(&parsed_fixture);
    assert!(validation_result.is_err());
}

#[test]
#[ignore = "manual fixture update workflow helper"]
fn print_reference_outputs_for_fixture_update() {
    let fixture_document = load_affine_oracle_solver_reference_fixture_document();

    for fixture_case in &fixture_document.cases {
        let solver_settings = fixture_case
            .solver_settings
            .to_core(&fixture_case.fixture_id);
        let observed_result =
            solve_exponential_affine_transform_with_settings(fixture_case.input, solver_settings)
                .unwrap_or_else(|source| {
                    panic!(
                        "fixture {} ({}) solve failed: {}",
                        fixture_case.fixture_id, fixture_case.description, source
                    )
                });

        let observed_output = observed_result.output.unwrap_or_else(|| {
            panic!(
                "fixture {} ({}) expected output but solver returned none",
                fixture_case.fixture_id, fixture_case.description
            )
        });

        println!("fixture_id: {}", fixture_case.fixture_id);
        println!(
            "c_at_delta: {{\"real\": {:.17}, \"imaginary\": {:.17}}}",
            observed_output.c_at_delta.real, observed_output.c_at_delta.imaginary
        );
        println!(
            "d_at_delta.plus_plus: {{\"real\": {:.17}, \"imaginary\": {:.17}}}",
            observed_output.d_at_delta.plus_plus.real,
            observed_output.d_at_delta.plus_plus.imaginary
        );
        println!(
            "d_at_delta.plus_minus: {{\"real\": {:.17}, \"imaginary\": {:.17}}}",
            observed_output.d_at_delta.plus_minus.real,
            observed_output.d_at_delta.plus_minus.imaginary
        );
        println!(
            "d_at_delta.minus_plus: {{\"real\": {:.17}, \"imaginary\": {:.17}}}",
            observed_output.d_at_delta.minus_plus.real,
            observed_output.d_at_delta.minus_plus.imaginary
        );
        println!(
            "d_at_delta.minus_minus: {{\"real\": {:.17}, \"imaginary\": {:.17}}}",
            observed_output.d_at_delta.minus_minus.real,
            observed_output.d_at_delta.minus_minus.imaginary
        );
        println!(
            "transform_value: {{\"real\": {:.17}, \"imaginary\": {:.17}}}",
            observed_output.transform_value.real, observed_output.transform_value.imaginary
        );
    }
}

fn validate_fixture_document(
    fixture_document: &AffineOracleSolverReferenceFixtureDocument,
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
        fixture_case.expected_output.validate().map_err(|source| {
            format!(
                "fixture {} has invalid expected_output: {source}",
                fixture_case.fixture_id
            )
        })?;
        fixture_case.tolerances.validate(&fixture_case.fixture_id)?;

        fixture_case
            .solver_settings
            .to_core(&fixture_case.fixture_id);

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

fn validate_positive_finite_tolerance(
    fixture_id: &str,
    field_name: &str,
    value: f64,
) -> Result<(), String> {
    if !value.is_finite() || value <= 0.0 {
        return Err(format!(
            "fixture {} has invalid {} value {}",
            fixture_id, field_name, value
        ));
    }

    Ok(())
}

fn assert_complex_within_tolerance(
    observed: ComplexScalar,
    expected: ComplexScalar,
    tolerance: f64,
    fixture_id: &str,
    component_label: &str,
    description: &str,
) {
    assert!(
        (observed.real - expected.real).abs() <= tolerance,
        "fixture {} ({}) {} real drift: observed={}, expected={}, tolerance={}",
        fixture_id,
        description,
        component_label,
        observed.real,
        expected.real,
        tolerance
    );
    assert!(
        (observed.imaginary - expected.imaginary).abs() <= tolerance,
        "fixture {} ({}) {} imaginary drift: observed={}, expected={}, tolerance={}",
        fixture_id,
        description,
        component_label,
        observed.imaginary,
        expected.imaginary,
        tolerance
    );
}

fn load_affine_oracle_solver_reference_fixture_document()
-> AffineOracleSolverReferenceFixtureDocument {
    let fixture_path = affine_oracle_fixture_path();
    let fixture_payload = load_affine_oracle_solver_reference_fixture_payload();

    serde_json::from_str(&fixture_payload).unwrap_or_else(|source| {
        panic!(
            "failed to parse affine oracle solver reference fixture `{}`: {}",
            fixture_path.display(),
            source
        )
    })
}

fn load_affine_oracle_solver_reference_fixture_payload() -> String {
    let fixture_path = affine_oracle_fixture_path();
    fs::read_to_string(&fixture_path).unwrap_or_else(|source| {
        panic!(
            "failed to read affine oracle solver reference fixture `{}`: {}",
            fixture_path.display(),
            source
        )
    })
}

fn affine_oracle_fixture_path() -> PathBuf {
    fixtures_root_path()
        .join("affine_oracle")
        .join("affine_oracle_solver_reference_fixture.json")
}
