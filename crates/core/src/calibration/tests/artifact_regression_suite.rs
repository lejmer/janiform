use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use super::support::*;

const CALIBRATION_ARTIFACT_REGRESSION_FIXTURE_SCHEMA_VERSION: &str =
    "calibration.artifact_regression.fixture.v1";
const CALIBRATION_ARTIFACT_REGRESSION_FIXTURE_SET_ID: &str =
    "calibration_artifact_round_trip_regression_fixture_v1";
const CALIBRATION_ARTIFACT_REGRESSION_SCHEMA_FAMILY: &str =
    "validation.calibration.artifact_regression.fixture";
const CALIBRATION_ARTIFACT_REGRESSION_SCHEMA_GENERATION: &str = "v1";
const CALIBRATION_ARTIFACT_REGRESSION_FORWARD_EXTENSION_POLICY: &str = "additive_fields_only.v1";
const CALIBRATION_ARTIFACT_REGRESSION_LOADING_ORDER_POLICY: &str =
    "preserve_declared_case_order.v1";
const CALIBRATION_ARTIFACT_REGRESSION_CONTENT_HASH_SCOPE_POLICY: &str = "document_payload_bytes.v1";
const CALIBRATION_ARTIFACT_REGRESSION_CONTENT_HASH_ALGORITHM: &str = "fnv1a_64_hex_utf8.v1";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct CalibrationArtifactRegressionFixtureDocument {
    schema_version: String,
    fixture_set_id: String,
    schema_metadata: CalibrationArtifactRegressionSchemaMetadata,
    determinism_metadata: CalibrationArtifactRegressionDeterminismMetadata,
    canonical_regression_metadata: CalibrationArtifactRegressionCanonicalMetadata,
    round_trip_cases: Vec<CalibrationArtifactRoundTripCase>,
    corruption_cases: Vec<CalibrationArtifactCorruptionCase>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct CalibrationArtifactRegressionSchemaMetadata {
    schema_family: String,
    schema_generation: String,
    forward_extension_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct CalibrationArtifactRegressionDeterminismMetadata {
    fixture_loading_order: String,
    content_hash_scope: String,
    content_hash_algorithm: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct CalibrationArtifactRegressionCanonicalMetadata {
    required_artifact_categories: Vec<String>,
    required_corruption_kinds: Vec<String>,
    required_default_policy_surfaces: Vec<String>,
    supported_validation_metric_roles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct CalibrationArtifactRoundTripCase {
    fixture_id: String,
    run_id: String,
    execution_mode: CalibrationExecutionMode,
    validation_metric_roles: Vec<CalibrationValidationMetricArtifactRole>,
    expected_stage_order: Vec<CalibrationStage>,
    expect_experiment_index_record: bool,
    description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct CalibrationArtifactCorruptionCase {
    fixture_id: String,
    run_id: String,
    execution_mode: CalibrationExecutionMode,
    selection: CalibrationArtifactRegressionSelection,
    #[serde(default)]
    requested_stages: Vec<CalibrationStage>,
    validation_metric_roles: Vec<CalibrationValidationMetricArtifactRole>,
    artifact_target: CalibrationArtifactCorruptionTarget,
    corruption_kind: CalibrationArtifactCorruptionKind,
    expected_error: CalibrationArtifactCorruptionExpectation,
    description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CalibrationArtifactRegressionSelection {
    CompleteRun,
    StageSubset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CalibrationArtifactCorruptionKind {
    MissingFile,
    MalformedPayload,
    UnsupportedSchemaVersion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CalibrationArtifactRegressionComponentKind {
    Manifest,
    DatasetSnapshot,
    StageParameter,
    ValidationMetric,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct CalibrationArtifactCorruptionTarget {
    component_kind: CalibrationArtifactRegressionComponentKind,
    #[serde(default)]
    stage: Option<CalibrationStage>,
    #[serde(default)]
    artifact_role: Option<CalibrationValidationMetricArtifactRole>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CalibrationArtifactRegressionExpectedErrorKind {
    MissingArtifactFile,
    CorruptedArtifact,
    UnsupportedSchema,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct CalibrationArtifactCorruptionExpectation {
    kind: CalibrationArtifactRegressionExpectedErrorKind,
    component_kind: CalibrationArtifactRegressionComponentKind,
    #[serde(default)]
    stage: Option<CalibrationStage>,
    #[serde(default)]
    artifact_role: Option<CalibrationValidationMetricArtifactRole>,
    #[serde(default)]
    expected_schema_version: Option<String>,
    #[serde(default)]
    found_schema_version: Option<String>,
    diagnostic_substrings: Vec<String>,
}

struct PersistedArtifactRegressionFixture {
    root_directory: PathBuf,
    manifest: CalibrationArtifactManifest,
    dataset_snapshot: CalibrationDatasetSnapshotArtifact,
    stage_parameter_artifacts: Vec<CalibrationStageParameterArtifact>,
    validation_metric_artifacts: Vec<CalibrationValidationMetricArtifact>,
    experiment_index: CalibrationExperimentIndexDocument,
}

#[test]
fn calibration_artifact_regression_fixture_metadata_is_complete() {
    let fixture = load_calibration_artifact_regression_fixture();

    assert_eq!(
        fixture.schema_version,
        CALIBRATION_ARTIFACT_REGRESSION_FIXTURE_SCHEMA_VERSION
    );
    assert_eq!(
        fixture.fixture_set_id,
        CALIBRATION_ARTIFACT_REGRESSION_FIXTURE_SET_ID
    );
    assert_eq!(
        fixture.schema_metadata.schema_family,
        CALIBRATION_ARTIFACT_REGRESSION_SCHEMA_FAMILY
    );
    assert_eq!(
        fixture.schema_metadata.schema_generation,
        CALIBRATION_ARTIFACT_REGRESSION_SCHEMA_GENERATION
    );
    assert_eq!(
        fixture.schema_metadata.forward_extension_policy,
        CALIBRATION_ARTIFACT_REGRESSION_FORWARD_EXTENSION_POLICY
    );
    assert_eq!(
        fixture.determinism_metadata.fixture_loading_order,
        CALIBRATION_ARTIFACT_REGRESSION_LOADING_ORDER_POLICY
    );
    assert_eq!(
        fixture.determinism_metadata.content_hash_scope,
        CALIBRATION_ARTIFACT_REGRESSION_CONTENT_HASH_SCOPE_POLICY
    );
    assert_eq!(
        fixture.determinism_metadata.content_hash_algorithm,
        CALIBRATION_ARTIFACT_REGRESSION_CONTENT_HASH_ALGORITHM
    );
    assert_eq!(
        fixture
            .canonical_regression_metadata
            .required_artifact_categories,
        vec![
            "dataset_snapshot".to_string(),
            "stage_parameter".to_string(),
            "validation_metric".to_string(),
            "manifest".to_string(),
            "experiment_index".to_string(),
        ]
    );
    assert_eq!(
        fixture
            .canonical_regression_metadata
            .required_corruption_kinds,
        vec![
            "missing_file".to_string(),
            "malformed_payload".to_string(),
            "unsupported_schema_version".to_string(),
        ]
    );
    assert_eq!(
        fixture
            .canonical_regression_metadata
            .required_default_policy_surfaces,
        vec!["cli".to_string(), "python_importable_api".to_string()]
    );
    assert_eq!(
        fixture
            .canonical_regression_metadata
            .supported_validation_metric_roles,
        vec!["comparison".to_string(), "diagnostics".to_string()]
    );
    assert_eq!(fixture.round_trip_cases.len(), 1);
    assert_eq!(fixture.corruption_cases.len(), 4);
}

#[test]
fn calibration_artifact_regression_round_trip_preserves_all_persisted_categories() {
    let fixture = load_calibration_artifact_regression_fixture();

    for round_trip_case in &fixture.round_trip_cases {
        assert_eq!(
            round_trip_case.execution_mode,
            CalibrationExecutionMode::Disk,
            "fixture_id={} must target persisted disk-mode execution",
            round_trip_case.fixture_id
        );

        let persisted_fixture = commit_persisted_artifact_regression_fixture(
            round_trip_case.run_id.as_str(),
            round_trip_case.validation_metric_roles.as_slice(),
        );

        assert_eq!(
            persisted_fixture
                .stage_parameter_artifacts
                .iter()
                .map(|artifact| artifact.stage)
                .collect::<Vec<_>>(),
            round_trip_case.expected_stage_order,
            "fixture_id={} unexpected stage artifact order",
            round_trip_case.fixture_id
        );
        assert_eq!(
            persisted_fixture.manifest.run_id, round_trip_case.run_id,
            "fixture_id={} manifest run_id mismatch",
            round_trip_case.fixture_id
        );
        assert_eq!(
            persisted_fixture.dataset_snapshot.run_id, round_trip_case.run_id,
            "fixture_id={} dataset snapshot run_id mismatch",
            round_trip_case.fixture_id
        );

        let manifest_path = calibration_artifact_manifest_path(
            persisted_fixture.root_directory.as_path(),
            round_trip_case.run_id.as_str(),
        );
        let loaded_manifest = load_calibration_artifact_manifest_from_disk(manifest_path.as_path())
            .expect("committed manifest should load");
        assert_eq!(
            loaded_manifest, persisted_fixture.manifest,
            "fixture_id={} manifest round-trip changed",
            round_trip_case.fixture_id
        );

        let dataset_snapshot_path = calibration_dataset_snapshot_artifact_path(
            persisted_fixture.root_directory.as_path(),
            round_trip_case.run_id.as_str(),
        );
        let loaded_dataset_snapshot =
            load_calibration_dataset_snapshot_artifact_from_disk(dataset_snapshot_path.as_path())
                .expect("dataset snapshot should load");
        assert_eq!(
            loaded_dataset_snapshot, persisted_fixture.dataset_snapshot,
            "fixture_id={} dataset snapshot round-trip changed",
            round_trip_case.fixture_id
        );

        for expected_stage_artifact in &persisted_fixture.stage_parameter_artifacts {
            let stage_artifact_path = calibration_stage_parameter_artifact_path(
                persisted_fixture.root_directory.as_path(),
                round_trip_case.run_id.as_str(),
                expected_stage_artifact.stage,
            );
            let loaded_stage_artifact =
                load_calibration_stage_parameter_artifact_from_disk(stage_artifact_path.as_path())
                    .expect("stage parameter artifact should load");

            assert_stage_artifact_matches(
                round_trip_case.fixture_id.as_str(),
                &loaded_stage_artifact,
                expected_stage_artifact,
            );
        }

        for expected_validation_metric_artifact in &persisted_fixture.validation_metric_artifacts {
            let validation_metric_path = calibration_validation_metric_artifact_path(
                persisted_fixture.root_directory.as_path(),
                round_trip_case.run_id.as_str(),
                expected_validation_metric_artifact.artifact_role,
            );
            let loaded_validation_metric = load_calibration_validation_metric_artifact_from_disk(
                validation_metric_path.as_path(),
            )
            .expect("validation metric artifact should load");

            assert_eq!(
                loaded_validation_metric, *expected_validation_metric_artifact,
                "fixture_id={} role={:?} validation metric round-trip changed",
                round_trip_case.fixture_id, expected_validation_metric_artifact.artifact_role
            );
        }

        if round_trip_case.expect_experiment_index_record {
            assert_eq!(
                persisted_fixture.experiment_index.records.len(),
                1,
                "fixture_id={} experiment index should contain one record",
                round_trip_case.fixture_id
            );

            let index_record = &persisted_fixture.experiment_index.records[0];
            assert_eq!(
                index_record.run_id, round_trip_case.run_id,
                "fixture_id={} experiment index run_id mismatch",
                round_trip_case.fixture_id
            );
            assert_eq!(
                index_record.execution_mode,
                CalibrationExecutionMode::Disk,
                "fixture_id={} experiment index execution_mode mismatch",
                round_trip_case.fixture_id
            );
            assert_eq!(
                index_record.index_metadata, persisted_fixture.manifest.index_metadata,
                "fixture_id={} experiment index metadata drifted from manifest",
                round_trip_case.fixture_id
            );
        }

        let reload_request =
            CalibrationArtifactReloadRequest::complete_run(round_trip_case.run_id.as_str())
                .expect("complete-run reload request should validate");
        let reloaded_run = reload_calibration_artifact_run_from_disk(
            persisted_fixture.root_directory.as_path(),
            &reload_request,
        )
        .expect("complete-run reload should succeed");

        assert_eq!(
            reloaded_run.manifest, persisted_fixture.manifest,
            "fixture_id={} reload manifest mismatch",
            round_trip_case.fixture_id
        );
        assert_eq!(
            reloaded_run.dataset_snapshot, persisted_fixture.dataset_snapshot,
            "fixture_id={} reload dataset snapshot mismatch",
            round_trip_case.fixture_id
        );
        assert_eq!(
            reloaded_run.stage_parameter_artifacts.len(),
            persisted_fixture.stage_parameter_artifacts.len(),
            "fixture_id={} reload stage artifact count mismatch",
            round_trip_case.fixture_id
        );

        for (reloaded_stage_artifact, expected_stage_artifact) in reloaded_run
            .stage_parameter_artifacts
            .iter()
            .zip(&persisted_fixture.stage_parameter_artifacts)
        {
            assert_stage_artifact_matches(
                round_trip_case.fixture_id.as_str(),
                reloaded_stage_artifact,
                expected_stage_artifact,
            );
        }

        assert_eq!(
            reloaded_run.diagnostics_artifact,
            persisted_fixture
                .validation_metric_artifacts
                .iter()
                .find(|artifact| {
                    artifact.artifact_role == CalibrationValidationMetricArtifactRole::Diagnostics
                })
                .cloned(),
            "fixture_id={} diagnostics reload mismatch",
            round_trip_case.fixture_id
        );
        assert_eq!(
            reloaded_run.comparison_artifact,
            persisted_fixture
                .validation_metric_artifacts
                .iter()
                .find(|artifact| {
                    artifact.artifact_role == CalibrationValidationMetricArtifactRole::Comparison
                })
                .cloned(),
            "fixture_id={} comparison reload mismatch",
            round_trip_case.fixture_id
        );

        let _ = fs::remove_dir_all(persisted_fixture.root_directory);
    }
}

#[test]
fn calibration_artifact_regression_reports_stable_corruption_diagnostics_per_category() {
    let fixture = load_calibration_artifact_regression_fixture();

    for corruption_case in &fixture.corruption_cases {
        assert_eq!(
            corruption_case.execution_mode,
            CalibrationExecutionMode::Disk,
            "fixture_id={} corruption cases must target persisted disk-mode execution",
            corruption_case.fixture_id
        );

        let persisted_fixture = commit_persisted_artifact_regression_fixture(
            corruption_case.run_id.as_str(),
            corruption_case.validation_metric_roles.as_slice(),
        );

        let target_path = regression_artifact_target_path(
            persisted_fixture.root_directory.as_path(),
            corruption_case.run_id.as_str(),
            &corruption_case.artifact_target,
        );

        apply_corruption(
            target_path.as_path(),
            corruption_case.corruption_kind,
            corruption_case
                .expected_error
                .found_schema_version
                .as_deref()
                .unwrap_or("calibration.unsupported_schema_fixture.v2"),
        );

        let reload_request = build_regression_reload_request(corruption_case);
        let error = reload_calibration_artifact_run_from_disk(
            persisted_fixture.root_directory.as_path(),
            &reload_request,
        )
        .expect_err("corrupted artifact set should fail reload");

        assert_regression_reload_error_matches(corruption_case, error, target_path.as_path());

        let _ = fs::remove_dir_all(persisted_fixture.root_directory);
    }
}

#[test]
fn calibration_artifact_regression_enforces_cli_and_import_default_persistence_policy() {
    let fixture = load_calibration_artifact_regression_fixture();
    assert_eq!(
        fixture
            .canonical_regression_metadata
            .required_default_policy_surfaces,
        vec!["cli".to_string(), "python_importable_api".to_string()]
    );

    assert_eq!(
        default_calibration_execution_mode_for_surface(CalibrationCallerSurface::Cli),
        CalibrationExecutionMode::Disk
    );
    assert_eq!(
        default_calibration_execution_mode_for_surface(
            CalibrationCallerSurface::PythonImportableApi
        ),
        CalibrationExecutionMode::Memory
    );
    assert_eq!(
        resolve_calibration_execution_mode_for_surface(
            CalibrationCallerSurface::PythonImportableApi,
            Some(CalibrationExecutionMode::Disk)
        ),
        CalibrationExecutionMode::Disk
    );

    let calendar_registry = calibration_stage_test_registry();

    let cli_directory = temporary_checkpoint_directory("artifact-regression-cli-default");
    let cli_contract = build_run_contract_for_caller_surface(
        "run-artifact-regression-cli-default",
        CalibrationCallerSurface::Cli,
        None,
    );

    let cli_report = run_staged_calibration_with_checkpointing(
        &cli_contract,
        &calendar_registry,
        staged_run_inputs(true, true, true),
        None,
        Some(cli_directory.as_path()),
    );
    assert_eq!(cli_report.status, CalibrationStageExecutionStatus::Success);
    assert_eq!(cli_report.execution_mode, CalibrationExecutionMode::Disk);
    assert!(
        calibration_artifact_manifest_path(
            cli_directory.as_path(),
            cli_contract.run_spec.run_id.as_str(),
        )
        .exists()
    );
    assert!(
        calibration_dataset_snapshot_artifact_path(
            cli_directory.as_path(),
            cli_contract.run_spec.run_id.as_str(),
        )
        .exists()
    );

    let python_directory = temporary_checkpoint_directory("artifact-regression-python-default");
    let python_contract = build_run_contract_for_caller_surface(
        "run-artifact-regression-python-default",
        CalibrationCallerSurface::PythonImportableApi,
        None,
    );
    let mut checkpoint_store = CalibrationMemoryCheckpointStore::new();

    let python_report = run_staged_calibration_with_checkpointing(
        &python_contract,
        &calendar_registry,
        staged_run_inputs(true, true, true),
        Some(&mut checkpoint_store),
        Some(python_directory.as_path()),
    );
    assert_eq!(
        python_report.status,
        CalibrationStageExecutionStatus::Success
    );
    assert_eq!(
        python_report.execution_mode,
        CalibrationExecutionMode::Memory
    );
    assert!(
        !calibration_artifact_manifest_path(
            python_directory.as_path(),
            python_contract.run_spec.run_id.as_str(),
        )
        .exists()
    );
    assert!(
        !calibration_dataset_snapshot_artifact_path(
            python_directory.as_path(),
            python_contract.run_spec.run_id.as_str(),
        )
        .exists()
    );

    for stage in [
        CalibrationStage::Seasonality,
        CalibrationStage::Hawkes,
        CalibrationStage::ReturnComponents,
    ] {
        assert!(
            !calibration_stage_parameter_artifact_path(
                python_directory.as_path(),
                python_contract.run_spec.run_id.as_str(),
                stage,
            )
            .exists(),
            "memory-mode default should not persist stage artifact for {stage:?}"
        );
    }

    let _ = fs::remove_dir_all(cli_directory);
    let _ = fs::remove_dir_all(python_directory);
}

fn load_calibration_artifact_regression_fixture() -> CalibrationArtifactRegressionFixtureDocument {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/calibration/calibration_artifact_round_trip_regression_fixture.json"
    )))
    .expect("calibration artifact regression fixture should deserialize")
}

fn commit_persisted_artifact_regression_fixture(
    run_id: &str,
    validation_metric_roles: &[CalibrationValidationMetricArtifactRole],
) -> PersistedArtifactRegressionFixture {
    let root_directory = temporary_checkpoint_directory("artifact-regression-suite");
    let calendar_registry = calibration_stage_test_registry();
    let run_contract =
        build_run_contract_with_mode_and_resume(run_id, CalibrationExecutionMode::Disk, None);

    let seasonality_report = run_seasonality_calibration_stage(
        &run_contract.run_spec,
        &calendar_registry,
        SeasonalityStageExecutionInput::from_timestamp_event_records(
            calibration_stage_timestamp_fixture(),
        ),
    );
    let hawkes_report = run_hawkes_calibration_stage(
        &run_contract.run_spec,
        HawkesStageExecutionInput::from_persisted_seasonality_artifact_reference(
            format!("artifact://seasonality/{run_id}"),
            hawkes_stage_candidate_fixture(),
        ),
    );
    let return_components_report = run_return_components_calibration_stage(
        &run_contract.run_spec,
        &calendar_registry,
        ReturnComponentsStageExecutionInput::from_persisted_activity_artifact_reference(
            format!("artifact://hawkes/{run_id}"),
            return_components_stage_candidate_generation_fixture(),
        ),
    );

    let stage_parameter_artifacts = vec![
        build_calibration_stage_parameter_artifact(
            &run_contract,
            &CalibrationStagedRunStageReport::Seasonality(seasonality_report),
        )
        .expect("seasonality stage artifact should build"),
        build_calibration_stage_parameter_artifact(
            &run_contract,
            &CalibrationStagedRunStageReport::Hawkes(hawkes_report),
        )
        .expect("hawkes stage artifact should build"),
        build_calibration_stage_parameter_artifact(
            &run_contract,
            &CalibrationStagedRunStageReport::ReturnComponents(return_components_report),
        )
        .expect("return-components stage artifact should build"),
    ];
    let dataset_snapshot = build_calibration_dataset_snapshot_artifact(&run_contract)
        .expect("dataset snapshot should build");

    let mut validation_metric_artifacts = validation_metric_roles
        .iter()
        .copied()
        .map(|artifact_role| {
            build_validation_metric_artifact_for_role(
                run_id,
                stage_parameter_artifacts.as_slice(),
                artifact_role,
            )
        })
        .collect::<Vec<_>>();
    validation_metric_artifacts.sort_by_key(|artifact| artifact.artifact_role);

    let manifest = build_calibration_artifact_manifest(
        &run_contract,
        &dataset_snapshot,
        stage_parameter_artifacts.as_slice(),
        &[],
        validation_metric_artifacts.as_slice(),
        &[],
        Utc.with_ymd_and_hms(2026, 3, 7, 9, 30, 0)
            .single()
            .expect("fixture timestamp should be valid"),
    )
    .expect("artifact manifest should build");

    let writer = CalibrationArtifactRunWriter::prepare(
        root_directory.as_path(),
        run_id,
        CalibrationArtifactRunPreparationMode::FreshRun,
    )
    .expect("artifact writer should prepare");

    persist_calibration_dataset_snapshot_artifact_to_run_directory(
        writer.pending_run_directory(),
        &dataset_snapshot,
    )
    .expect("dataset snapshot should persist");

    for stage_parameter_artifact in &stage_parameter_artifacts {
        persist_calibration_stage_parameter_artifact_to_run_directory(
            writer.pending_run_directory(),
            stage_parameter_artifact,
        )
        .expect("stage artifact should persist");
    }

    for validation_metric_artifact in &validation_metric_artifacts {
        persist_calibration_validation_metric_artifact_to_run_directory(
            writer.pending_run_directory(),
            validation_metric_artifact,
        )
        .expect("validation metric artifact should persist");
    }

    persist_calibration_artifact_manifest_to_run_directory(
        writer.pending_run_directory(),
        &manifest,
    )
    .expect("manifest should persist");

    writer.commit().expect("artifact set should commit");

    let experiment_index = load_calibration_experiment_index_from_disk(root_directory.as_path())
        .expect("experiment index should load");

    PersistedArtifactRegressionFixture {
        root_directory,
        manifest,
        dataset_snapshot,
        stage_parameter_artifacts,
        validation_metric_artifacts,
        experiment_index,
    }
}

fn build_validation_metric_artifact_for_role(
    run_id: &str,
    stage_parameter_artifacts: &[CalibrationStageParameterArtifact],
    artifact_role: CalibrationValidationMetricArtifactRole,
) -> CalibrationValidationMetricArtifact {
    let fixture_policy_id = format!(
        "calibration_artifact_regression.{}.thresholds.v1",
        artifact_role.artifact_token()
    );
    let mut metric_collections = build_validation_metric_artifact_fixture()
        .metric_collections
        .clone();

    for metric_collection in &mut metric_collections {
        metric_collection.collection_id = format!(
            "{}.{}",
            artifact_role.artifact_token(),
            metric_collection.collection_id
        );
        metric_collection.source_kind = match artifact_role {
            CalibrationValidationMetricArtifactRole::Diagnostics => {
                CalibrationValidationMetricSourceKind::ActivityClustering
            }
            CalibrationValidationMetricArtifactRole::Comparison => {
                CalibrationValidationMetricSourceKind::StylizedFactHorizonComparison
            }
        };
        metric_collection.context.context_id = run_id.to_string();
        metric_collection.context.context_label = format!(
            "{} regression fixture for {run_id}",
            artifact_role.artifact_token()
        );
        metric_collection.primary_role_label = "observed".to_string();
        metric_collection.secondary_role_label = Some("expected".to_string());
        metric_collection.threshold_policy_id = fixture_policy_id.clone();

        for metric in &mut metric_collection.metrics {
            metric.metric_id = format!(
                "{}.{}.{}",
                artifact_role.artifact_token(),
                run_id,
                metric.metric_id
            );
            metric.threshold_metadata.threshold_policy_id = fixture_policy_id.clone();
            metric.threshold_metadata.threshold_policy_version = Some("fixture.v1".to_string());
            metric.threshold_metadata.threshold_id =
                Some(format!("{}_threshold", artifact_role.artifact_token()));
            metric.threshold_metadata.threshold_rationale = Some(format!(
                "{} round-trip fixture should remain unchanged",
                artifact_role.artifact_token()
            ));
        }
    }

    CalibrationValidationMetricArtifact::try_new(
        run_id,
        artifact_role,
        build_stage_output_links(stage_parameter_artifacts),
        metric_collections,
    )
    .expect("validation metric artifact should build")
}

fn build_regression_reload_request(
    corruption_case: &CalibrationArtifactCorruptionCase,
) -> CalibrationArtifactReloadRequest {
    match corruption_case.selection {
        CalibrationArtifactRegressionSelection::CompleteRun => {
            CalibrationArtifactReloadRequest::complete_run(corruption_case.run_id.as_str())
                .expect("complete-run reload request should validate")
        }
        CalibrationArtifactRegressionSelection::StageSubset => {
            CalibrationArtifactReloadRequest::stage_subset(
                corruption_case.run_id.as_str(),
                corruption_case.requested_stages.clone(),
            )
            .expect("stage-subset reload request should validate")
        }
    }
}

fn regression_artifact_target_path(
    root_directory: &Path,
    run_id: &str,
    artifact_target: &CalibrationArtifactCorruptionTarget,
) -> PathBuf {
    match artifact_target.component_kind {
        CalibrationArtifactRegressionComponentKind::Manifest => {
            calibration_artifact_manifest_path(root_directory, run_id)
        }
        CalibrationArtifactRegressionComponentKind::DatasetSnapshot => {
            calibration_dataset_snapshot_artifact_path(root_directory, run_id)
        }
        CalibrationArtifactRegressionComponentKind::StageParameter => {
            calibration_stage_parameter_artifact_path(
                root_directory,
                run_id,
                artifact_target
                    .stage
                    .expect("stage-parameter corruption targets require a stage"),
            )
        }
        CalibrationArtifactRegressionComponentKind::ValidationMetric => {
            calibration_validation_metric_artifact_path(
                root_directory,
                run_id,
                artifact_target
                    .artifact_role
                    .expect("validation-metric corruption targets require an artifact role"),
            )
        }
    }
}

fn apply_corruption(
    artifact_path: &Path,
    corruption_kind: CalibrationArtifactCorruptionKind,
    unsupported_schema_version: &str,
) {
    match corruption_kind {
        CalibrationArtifactCorruptionKind::MissingFile => {
            fs::remove_file(artifact_path).expect("corrupted fixture file should be removable");
        }
        CalibrationArtifactCorruptionKind::MalformedPayload => {
            fs::write(artifact_path, "{ invalid json")
                .expect("corrupted fixture payload should be written");
        }
        CalibrationArtifactCorruptionKind::UnsupportedSchemaVersion => {
            rewrite_json_document(artifact_path, |document| {
                document["schema_version"] = Value::String(unsupported_schema_version.to_string());
            });
        }
    }
}

fn rewrite_json_document(artifact_path: &Path, update: impl FnOnce(&mut Value)) {
    let mut document: Value = serde_json::from_str(
        fs::read_to_string(artifact_path)
            .expect("artifact document should be readable")
            .as_str(),
    )
    .expect("artifact document should decode to JSON");

    update(&mut document);

    fs::write(
        artifact_path,
        serde_json::to_vec_pretty(&document).expect("artifact document should serialize"),
    )
    .expect("artifact document should be rewritten");
}

fn assert_regression_reload_error_matches(
    corruption_case: &CalibrationArtifactCorruptionCase,
    error: CalibrationArtifactReloadError,
    expected_path: &Path,
) {
    let diagnostic = error.to_string();
    let expected_component = regression_reload_component(
        corruption_case.expected_error.component_kind,
        corruption_case.expected_error.stage,
        corruption_case.expected_error.artifact_role,
    );

    match (corruption_case.expected_error.kind, error) {
        (
            CalibrationArtifactRegressionExpectedErrorKind::MissingArtifactFile,
            CalibrationArtifactReloadError::MissingArtifactFile { component, path },
        ) => {
            assert_eq!(
                component, expected_component,
                "fixture_id={} unexpected reload component",
                corruption_case.fixture_id
            );
            assert_eq!(
                path,
                expected_path.display().to_string(),
                "fixture_id={} unexpected missing-artifact path",
                corruption_case.fixture_id
            );
        }
        (
            CalibrationArtifactRegressionExpectedErrorKind::CorruptedArtifact,
            CalibrationArtifactReloadError::CorruptedArtifact {
                component,
                path,
                error_message: _,
            },
        ) => {
            assert_eq!(
                component, expected_component,
                "fixture_id={} unexpected reload component",
                corruption_case.fixture_id
            );
            assert_eq!(
                path,
                expected_path.display().to_string(),
                "fixture_id={} unexpected corrupted-artifact path",
                corruption_case.fixture_id
            );
        }
        (
            CalibrationArtifactRegressionExpectedErrorKind::UnsupportedSchema,
            CalibrationArtifactReloadError::UnsupportedSchema {
                component,
                path,
                expected_schema_version,
                found_schema_version,
            },
        ) => {
            assert_eq!(
                component, expected_component,
                "fixture_id={} unexpected reload component",
                corruption_case.fixture_id
            );
            assert_eq!(
                path,
                expected_path.display().to_string(),
                "fixture_id={} unexpected unsupported-schema path",
                corruption_case.fixture_id
            );
            assert_eq!(
                Some(expected_schema_version),
                corruption_case
                    .expected_error
                    .expected_schema_version
                    .clone(),
                "fixture_id={} unexpected expected-schema diagnostic",
                corruption_case.fixture_id
            );
            assert_eq!(
                Some(found_schema_version),
                corruption_case.expected_error.found_schema_version.clone(),
                "fixture_id={} unexpected found-schema diagnostic",
                corruption_case.fixture_id
            );
        }
        (_, other) => panic!(
            "fixture_id={} expected {:?} for run_id={}, found {other:?}",
            corruption_case.fixture_id, corruption_case.expected_error.kind, corruption_case.run_id
        ),
    }

    for expected_substring in &corruption_case.expected_error.diagnostic_substrings {
        assert!(
            diagnostic.contains(expected_substring),
            "fixture_id={} expected diagnostic `{}` to contain `{}`",
            corruption_case.fixture_id,
            diagnostic,
            expected_substring
        );
    }
}

fn regression_reload_component(
    component_kind: CalibrationArtifactRegressionComponentKind,
    stage: Option<CalibrationStage>,
    artifact_role: Option<CalibrationValidationMetricArtifactRole>,
) -> CalibrationArtifactReloadComponent {
    match component_kind {
        CalibrationArtifactRegressionComponentKind::Manifest => {
            CalibrationArtifactReloadComponent::Manifest
        }
        CalibrationArtifactRegressionComponentKind::DatasetSnapshot => {
            CalibrationArtifactReloadComponent::DatasetSnapshot
        }
        CalibrationArtifactRegressionComponentKind::StageParameter => {
            CalibrationArtifactReloadComponent::StageParameter {
                stage: stage.expect("stage-parameter expectations require a stage"),
            }
        }
        CalibrationArtifactRegressionComponentKind::ValidationMetric => {
            CalibrationArtifactReloadComponent::ValidationMetric {
                artifact_role: artifact_role
                    .expect("validation-metric expectations require an artifact role"),
            }
        }
    }
}

fn assert_stage_artifact_matches(
    fixture_id: &str,
    loaded_artifact: &CalibrationStageParameterArtifact,
    expected_artifact: &CalibrationStageParameterArtifact,
) {
    assert_eq!(
        loaded_artifact.run_id, expected_artifact.run_id,
        "fixture_id={} stage={:?} run_id drifted",
        fixture_id, expected_artifact.stage
    );
    assert_eq!(
        loaded_artifact.execution_mode, expected_artifact.execution_mode,
        "fixture_id={} stage={:?} execution_mode drifted",
        fixture_id, expected_artifact.stage
    );
    assert_eq!(
        loaded_artifact.stage, expected_artifact.stage,
        "fixture_id={} stage artifact kind drifted",
        fixture_id
    );
    assert_eq!(
        loaded_artifact.artifact_reference, expected_artifact.artifact_reference,
        "fixture_id={} stage={:?} artifact reference drifted",
        fixture_id, expected_artifact.stage
    );
    assert_eq!(
        loaded_artifact.stage_output_schema_version, expected_artifact.stage_output_schema_version,
        "fixture_id={} stage={:?} stage_output_schema_version drifted",
        fixture_id, expected_artifact.stage
    );
    assert_eq!(
        loaded_artifact.seed_metadata, expected_artifact.seed_metadata,
        "fixture_id={} stage={:?} seed metadata drifted",
        fixture_id, expected_artifact.stage
    );
    assert_eq!(
        loaded_artifact.solver_settings, expected_artifact.solver_settings,
        "fixture_id={} stage={:?} solver settings drifted",
        fixture_id, expected_artifact.stage
    );
    assert_eq!(
        loaded_artifact.optimizer_state, expected_artifact.optimizer_state,
        "fixture_id={} stage={:?} optimizer state drifted",
        fixture_id, expected_artifact.stage
    );

    match (
        &loaded_artifact.stage_parameters,
        &expected_artifact.stage_parameters,
    ) {
        (
            CalibrationStageParameterPayload::Seasonality(loaded_payload),
            CalibrationStageParameterPayload::Seasonality(expected_payload),
        ) => {
            assert_eq!(
                loaded_payload.dataset_id, expected_payload.dataset_id,
                "fixture_id={} seasonality dataset_id drifted",
                fixture_id
            );
            assert_eq!(
                loaded_payload.exchange_code, expected_payload.exchange_code,
                "fixture_id={} seasonality exchange_code drifted",
                fixture_id
            );
            assert_eq!(
                loaded_payload.trade_events_data_reference,
                expected_payload.trade_events_data_reference,
                "fixture_id={} seasonality trade_events_data_reference drifted",
                fixture_id
            );
            assert_eq!(
                loaded_payload.intraday_factor_by_bin.len(),
                expected_payload.intraday_factor_by_bin.len(),
                "fixture_id={} seasonality intraday factor length drifted",
                fixture_id
            );
            assert_eq!(
                loaded_payload.day_of_week_factor.len(),
                expected_payload.day_of_week_factor.len(),
                "fixture_id={} seasonality day-of-week factor length drifted",
                fixture_id
            );
            assert_eq!(
                loaded_payload.day_of_year_factor.len(),
                expected_payload.day_of_year_factor.len(),
                "fixture_id={} seasonality day-of-year factor length drifted",
                fixture_id
            );
            assert_eq!(
                loaded_payload.normalization_metadata, expected_payload.normalization_metadata,
                "fixture_id={} seasonality normalization metadata drifted",
                fixture_id
            );
        }
        (
            CalibrationStageParameterPayload::Hawkes(loaded_payload),
            CalibrationStageParameterPayload::Hawkes(expected_payload),
        ) => {
            assert_eq!(
                loaded_payload.selected_candidate_id, expected_payload.selected_candidate_id,
                "fixture_id={} hawkes selected_candidate_id drifted",
                fixture_id
            );
            assert_eq!(
                loaded_payload.fitted_parameters, expected_payload.fitted_parameters,
                "fixture_id={} hawkes fitted_parameters drifted",
                fixture_id
            );
        }
        (
            CalibrationStageParameterPayload::ReturnComponents(loaded_payload),
            CalibrationStageParameterPayload::ReturnComponents(expected_payload),
        ) => {
            assert_eq!(
                loaded_payload.selected_candidate_id, expected_payload.selected_candidate_id,
                "fixture_id={} return-components selected_candidate_id drifted",
                fixture_id
            );
            assert_eq!(
                loaded_payload.fitted_parameters, expected_payload.fitted_parameters,
                "fixture_id={} return-components fitted_parameters drifted",
                fixture_id
            );
        }
        _ => panic!(
            "fixture_id={} stage={:?} payload variant changed across round-trip",
            fixture_id, expected_artifact.stage
        ),
    }
}
