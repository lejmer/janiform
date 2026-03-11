use std::path::Path;

use serde_json::Value;

use super::support::*;

#[test]
fn reload_resolver_loads_complete_runs_and_stage_specific_subsets() {
    let run_id = "run-reload-resolver-success";
    let fixture = persist_reload_fixture(run_id);

    let full_request =
        CalibrationArtifactReloadRequest::complete_run(run_id).expect("full reload request");
    let full_reload =
        reload_calibration_artifact_run_from_disk(fixture.root_directory.as_path(), &full_request)
            .expect("full run reload should succeed");

    assert_eq!(full_reload.manifest, fixture.manifest);
    assert_eq!(full_reload.dataset_snapshot, fixture.dataset_snapshot);
    assert_stage_artifact_reload_matches(
        full_reload.stage_parameter_artifacts.as_slice(),
        fixture.stage_parameter_artifacts.as_slice(),
    );
    assert_diagnostics_reload_matches(
        full_reload
            .diagnostics_artifact
            .as_ref()
            .expect("diagnostics artifact should be present"),
        &fixture.diagnostics_artifact,
    );
    assert!(full_reload.comparison_artifact.is_none());
    assert_eq!(
        full_reload
            .stage_parameter_artifact(CalibrationStage::Hawkes)
            .expect("hawkes stage should be present")
            .stage,
        CalibrationStage::Hawkes
    );

    let partial_request = CalibrationArtifactReloadRequest::stage_subset(
        run_id,
        vec![CalibrationStage::ReturnComponents, CalibrationStage::Hawkes],
    )
    .expect("partial reload request");
    let partial_reload = reload_calibration_artifact_run_from_disk(
        fixture.root_directory.as_path(),
        &partial_request,
    )
    .expect("stage subset reload should succeed");

    assert_eq!(partial_reload.manifest.run_id, run_id);
    assert_eq!(partial_reload.dataset_snapshot, fixture.dataset_snapshot);
    assert_eq!(partial_reload.stage_parameter_artifacts.len(), 2);
    assert_eq!(
        partial_reload
            .stage_parameter_artifacts
            .iter()
            .map(|artifact| artifact.stage)
            .collect::<Vec<_>>(),
        vec![CalibrationStage::Hawkes, CalibrationStage::ReturnComponents,]
    );
    assert!(partial_reload.diagnostics_artifact.is_none());
    assert!(partial_reload.comparison_artifact.is_none());

    let _ = fs::remove_dir_all(fixture.root_directory);
}

#[test]
fn reload_resolver_rejects_unsupported_manifest_schema_versions() {
    let run_id = "run-reload-resolver-unsupported-schema";
    let fixture = persist_reload_fixture(run_id);

    let manifest_path =
        calibration_artifact_manifest_path(fixture.root_directory.as_path(), run_id);

    rewrite_json_document(manifest_path.as_path(), |document| {
        document["schema_version"] = json!("calibration.artifact_registry.manifest.v2");
    });

    let request =
        CalibrationArtifactReloadRequest::complete_run(run_id).expect("full reload request");
    let error =
        reload_calibration_artifact_run_from_disk(fixture.root_directory.as_path(), &request)
            .expect_err("unsupported manifest schema should fail reload");

    assert_eq!(
        error,
        CalibrationArtifactReloadError::UnsupportedSchema {
            component: CalibrationArtifactReloadComponent::Manifest,
            path: manifest_path.display().to_string(),
            expected_schema_version: CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION.to_string(),
            found_schema_version: "calibration.artifact_registry.manifest.v2".to_string(),
        }
    );

    let _ = fs::remove_dir_all(fixture.root_directory);
}

#[test]
fn reload_resolver_reports_corrupted_stage_artifacts_with_structured_errors() {
    let run_id = "run-reload-resolver-corrupted-stage";
    let fixture = persist_reload_fixture(run_id);

    let hawkes_path = calibration_stage_parameter_artifact_path(
        fixture.root_directory.as_path(),
        run_id,
        CalibrationStage::Hawkes,
    );
    fs::write(hawkes_path.as_path(), "{ invalid json")
        .expect("corrupted stage artifact should be written");

    let request =
        CalibrationArtifactReloadRequest::complete_run(run_id).expect("full reload request");
    let error =
        reload_calibration_artifact_run_from_disk(fixture.root_directory.as_path(), &request)
            .expect_err("corrupted stage artifact should fail reload");

    match error {
        CalibrationArtifactReloadError::CorruptedArtifact {
            component,
            path,
            error_message,
        } => {
            assert_eq!(
                component,
                CalibrationArtifactReloadComponent::StageParameter {
                    stage: CalibrationStage::Hawkes,
                }
            );
            assert_eq!(path, hawkes_path.display().to_string());
            assert!(!error_message.is_empty());
        }
        other => panic!("expected corrupted artifact error, found {other:?}"),
    }

    let _ = fs::remove_dir_all(fixture.root_directory);
}

struct PersistedReloadFixture {
    root_directory: PathBuf,
    manifest: CalibrationArtifactManifest,
    dataset_snapshot: CalibrationDatasetSnapshotArtifact,
    stage_parameter_artifacts: Vec<CalibrationStageParameterArtifact>,
    diagnostics_artifact: CalibrationValidationMetricArtifact,
}

fn persist_reload_fixture(run_id: &str) -> PersistedReloadFixture {
    let root_directory = temporary_checkpoint_directory("reload-resolver");
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
        &calendar_registry,
        HawkesStageExecutionInput::from_persisted_seasonality_artifact_reference(
            format!("artifact://seasonality/{run_id}"),
            seasonality_report
                .normalized_stage_payload
                .clone()
                .expect("seasonality report should expose payload"),
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

    let seasonality_artifact = build_calibration_stage_parameter_artifact(
        &run_contract,
        &CalibrationStagedRunStageReport::Seasonality(seasonality_report),
    )
    .expect("seasonality artifact should build");
    let hawkes_artifact = build_calibration_stage_parameter_artifact(
        &run_contract,
        &CalibrationStagedRunStageReport::Hawkes(hawkes_report),
    )
    .expect("hawkes artifact should build");
    let return_components_artifact = build_calibration_stage_parameter_artifact(
        &run_contract,
        &CalibrationStagedRunStageReport::ReturnComponents(return_components_report),
    )
    .expect("return-components artifact should build");
    let stage_parameter_artifacts = vec![
        seasonality_artifact,
        hawkes_artifact,
        return_components_artifact,
    ];

    let dataset_snapshot = build_calibration_dataset_snapshot_artifact(&run_contract)
        .expect("dataset snapshot should build");
    persist_calibration_dataset_snapshot_artifact_to_disk(
        root_directory.as_path(),
        &dataset_snapshot,
    )
    .expect("dataset snapshot should persist");

    for stage_parameter_artifact in &stage_parameter_artifacts {
        persist_calibration_stage_parameter_artifact_to_disk(
            root_directory.as_path(),
            stage_parameter_artifact,
        )
        .expect("stage parameter artifact should persist");
    }

    let diagnostics_artifact = CalibrationValidationMetricArtifact::try_new(
        run_id,
        CalibrationValidationMetricArtifactRole::Diagnostics,
        build_stage_output_links(stage_parameter_artifacts.as_slice()),
        build_validation_metric_artifact_fixture()
            .metric_collections
            .clone(),
    )
    .expect("diagnostics artifact should build");
    persist_calibration_validation_metric_artifact_to_disk(
        root_directory.as_path(),
        &diagnostics_artifact,
    )
    .expect("diagnostics artifact should persist");

    let manifest = build_calibration_artifact_manifest(
        &run_contract,
        &dataset_snapshot,
        stage_parameter_artifacts.as_slice(),
        &[],
        &[diagnostics_artifact.clone()],
        &[],
        Utc.with_ymd_and_hms(2026, 3, 4, 11, 30, 0)
            .single()
            .expect("fixture timestamp should be valid"),
    )
    .expect("artifact manifest should build");
    persist_calibration_artifact_manifest_to_disk(root_directory.as_path(), &manifest)
        .expect("artifact manifest should persist");

    PersistedReloadFixture {
        root_directory,
        manifest,
        dataset_snapshot,
        stage_parameter_artifacts,
        diagnostics_artifact,
    }
}

fn rewrite_json_document(artifact_path: &Path, update: impl FnOnce(&mut Value)) {
    let mut document: Value = serde_json::from_str(
        fs::read_to_string(artifact_path)
            .expect("artifact document should be readable")
            .as_str(),
    )
    .expect("artifact document should be valid JSON");

    update(&mut document);

    fs::write(
        artifact_path,
        serde_json::to_vec_pretty(&document).expect("artifact document should serialize"),
    )
    .expect("artifact document should be written");
}

fn assert_stage_artifact_reload_matches(
    reloaded_artifacts: &[CalibrationStageParameterArtifact],
    expected_artifacts: &[CalibrationStageParameterArtifact],
) {
    assert_eq!(reloaded_artifacts.len(), expected_artifacts.len());

    for (reloaded_artifact, expected_artifact) in reloaded_artifacts.iter().zip(expected_artifacts)
    {
        assert_eq!(reloaded_artifact.run_id, expected_artifact.run_id);
        assert_eq!(reloaded_artifact.stage, expected_artifact.stage);
        assert_eq!(
            reloaded_artifact.artifact_reference,
            expected_artifact.artifact_reference
        );
        assert_eq!(
            reloaded_artifact.stage_output_schema_version,
            expected_artifact.stage_output_schema_version
        );
        assert_eq!(
            reloaded_artifact.seed_metadata,
            expected_artifact.seed_metadata
        );
        assert_eq!(
            reloaded_artifact.solver_settings,
            expected_artifact.solver_settings
        );
        assert_eq!(
            reloaded_artifact.optimizer_state,
            expected_artifact.optimizer_state
        );

        match (
            &reloaded_artifact.stage_parameters,
            &expected_artifact.stage_parameters,
        ) {
            (
                CalibrationStageParameterPayload::Seasonality(reloaded_payload),
                CalibrationStageParameterPayload::Seasonality(expected_payload),
            ) => {
                assert_eq!(reloaded_payload.dataset_id, expected_payload.dataset_id);
                assert_eq!(
                    reloaded_payload.exchange_code,
                    expected_payload.exchange_code
                );
                assert_eq!(
                    reloaded_payload.trade_events_data_reference,
                    expected_payload.trade_events_data_reference
                );
                assert_eq!(
                    reloaded_payload.intraday_factor_by_bin.len(),
                    expected_payload.intraday_factor_by_bin.len()
                );
                assert_eq!(
                    reloaded_payload.day_of_week_factor.len(),
                    expected_payload.day_of_week_factor.len()
                );
                assert_eq!(
                    reloaded_payload.day_of_year_factor.len(),
                    expected_payload.day_of_year_factor.len()
                );
                assert_eq!(
                    reloaded_payload.normalization_metadata,
                    expected_payload.normalization_metadata
                );
            }
            (
                CalibrationStageParameterPayload::Hawkes(reloaded_payload),
                CalibrationStageParameterPayload::Hawkes(expected_payload),
            ) => {
                assert_eq!(
                    reloaded_payload.selected_candidate_id,
                    expected_payload.selected_candidate_id
                );
                assert_eq!(
                    reloaded_payload.fitted_parameters,
                    expected_payload.fitted_parameters
                );
                assert_eq!(
                    reloaded_payload.seasonality_input_transport,
                    expected_payload.seasonality_input_transport
                );
                assert_eq!(
                    reloaded_payload.seasonality_dataset_id,
                    expected_payload.seasonality_dataset_id
                );
                assert_eq!(
                    reloaded_payload.seasonality_artifact_reference,
                    expected_payload.seasonality_artifact_reference
                );
            }
            (
                CalibrationStageParameterPayload::ReturnComponents(reloaded_payload),
                CalibrationStageParameterPayload::ReturnComponents(expected_payload),
            ) => {
                assert_eq!(
                    reloaded_payload.selected_candidate_id,
                    expected_payload.selected_candidate_id
                );
                assert_eq!(
                    reloaded_payload.fitted_parameters,
                    expected_payload.fitted_parameters
                );
                assert_eq!(
                    reloaded_payload.candidate_generation_policy,
                    expected_payload.candidate_generation_policy
                );
                assert_eq!(
                    reloaded_payload.objective_weighting_policy,
                    expected_payload.objective_weighting_policy
                );
                assert_eq!(
                    reloaded_payload.objective_metric_bucket_policy,
                    expected_payload.objective_metric_bucket_policy
                );
                assert_eq!(
                    reloaded_payload.activity_input_transport,
                    expected_payload.activity_input_transport
                );
                assert_eq!(
                    reloaded_payload.activity_selected_hawkes_candidate_id,
                    expected_payload.activity_selected_hawkes_candidate_id
                );
                assert_eq!(
                    reloaded_payload.activity_artifact_reference,
                    expected_payload.activity_artifact_reference
                );
            }
            _ => panic!("reloaded stage payload kind did not match expected payload kind"),
        }
    }
}

fn assert_diagnostics_reload_matches(
    reloaded_artifact: &CalibrationValidationMetricArtifact,
    expected_artifact: &CalibrationValidationMetricArtifact,
) {
    assert_eq!(reloaded_artifact.run_id, expected_artifact.run_id);
    assert_eq!(
        reloaded_artifact.artifact_role,
        expected_artifact.artifact_role
    );
    assert_eq!(
        reloaded_artifact.artifact_reference,
        expected_artifact.artifact_reference
    );
    assert_eq!(
        reloaded_artifact.manifest_link,
        expected_artifact.manifest_link
    );
    assert_eq!(
        reloaded_artifact.stage_output_links,
        expected_artifact.stage_output_links
    );
    assert_eq!(
        reloaded_artifact.metric_collections,
        expected_artifact.metric_collections
    );
}
