mod support;
use support::shared::*;

#[test]
fn calibration_stage_parameter_artifacts_round_trip_per_stage_preserve_stage_payloads() {
    let calendar_registry = calibration_stage_test_registry();
    let run_contract =
        build_run_contract_with_mode_and_resume("run-001", CalibrationExecutionMode::Disk, None);

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
            "artifact://seasonality/run-001",
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
            "artifact://hawkes/run-001",
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

    assert_eq!(
        seasonality_artifact.schema_version,
        CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_VERSION
    );
    assert_eq!(
        seasonality_artifact.schema_metadata.schema_family,
        CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_FAMILY
    );
    assert_eq!(
        seasonality_artifact.schema_metadata.schema_generation,
        CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_GENERATION
    );
    assert_eq!(
        seasonality_artifact.output_schema_metadata.schema_version,
        CALIBRATION_STAGE_PARAMETER_OUTPUT_SCHEMA_METADATA_VERSION
    );
    assert!(seasonality_artifact.optimizer_state.is_none());

    let hawkes_optimizer_state = hawkes_artifact
        .optimizer_state
        .as_ref()
        .expect("hawkes artifact should include optimizer state");
    assert_eq!(
        hawkes_optimizer_state.schema_version,
        CALIBRATION_STAGE_PARAMETER_OPTIMIZER_STATE_SCHEMA_VERSION
    );
    assert_eq!(
        hawkes_optimizer_state
            .optimizer_adapter_metadata
            .schema_version,
        CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_RUN_SCHEMA_VERSION
    );
    assert_eq!(
        hawkes_artifact.artifact_reference,
        "artifact://hawkes/run-001"
    );

    let return_components_optimizer_state = return_components_artifact
        .optimizer_state
        .as_ref()
        .expect("return-components artifact should include optimizer state");
    assert_eq!(
        return_components_optimizer_state
            .optimizer_adapter_metadata
            .schema_version,
        CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_RUN_SCHEMA_VERSION
    );
    assert_eq!(
        return_components_artifact.artifact_reference,
        "artifact://return_components/run-001"
    );

    let checkpoint_directory = temporary_checkpoint_directory("stage-parameter-artifacts");
    let seasonality_path = persist_calibration_stage_parameter_artifact_to_disk(
        checkpoint_directory.as_path(),
        &seasonality_artifact,
    )
    .expect("seasonality artifact should persist");
    let hawkes_path = persist_calibration_stage_parameter_artifact_to_disk(
        checkpoint_directory.as_path(),
        &hawkes_artifact,
    )
    .expect("hawkes artifact should persist");
    let return_components_path = persist_calibration_stage_parameter_artifact_to_disk(
        checkpoint_directory.as_path(),
        &return_components_artifact,
    )
    .expect("return-components artifact should persist");

    assert_eq!(
        seasonality_path,
        calibration_stage_parameter_artifact_path(
            checkpoint_directory.as_path(),
            "run-001",
            CalibrationStage::Seasonality,
        )
    );
    assert_eq!(
        hawkes_path,
        calibration_stage_parameter_artifact_path(
            checkpoint_directory.as_path(),
            "run-001",
            CalibrationStage::Hawkes,
        )
    );
    assert_eq!(
        return_components_path,
        calibration_stage_parameter_artifact_path(
            checkpoint_directory.as_path(),
            "run-001",
            CalibrationStage::ReturnComponents,
        )
    );

    let loaded_seasonality =
        load_calibration_stage_parameter_artifact_from_disk(seasonality_path.as_path())
            .expect("seasonality artifact should load");
    let loaded_hawkes = load_calibration_stage_parameter_artifact_from_disk(hawkes_path.as_path())
        .expect("hawkes artifact should load");
    let loaded_return_components =
        load_calibration_stage_parameter_artifact_from_disk(return_components_path.as_path())
            .expect("return-components artifact should load");

    assert_eq!(loaded_seasonality.run_id, seasonality_artifact.run_id);
    assert_eq!(loaded_seasonality.stage, CalibrationStage::Seasonality);
    assert_eq!(
        loaded_seasonality.artifact_reference,
        seasonality_artifact.artifact_reference
    );
    match (
        &loaded_seasonality.stage_parameters,
        &seasonality_artifact.stage_parameters,
    ) {
        (
            CalibrationStageParameterPayload::Seasonality(loaded_payload),
            CalibrationStageParameterPayload::Seasonality(expected_payload),
        ) => {
            assert_eq!(loaded_payload.dataset_id, expected_payload.dataset_id);
            assert_eq!(
                loaded_payload.intraday_factor_by_bin.len(),
                expected_payload.intraday_factor_by_bin.len()
            );
            assert_eq!(
                loaded_payload.day_of_week_factor.len(),
                expected_payload.day_of_week_factor.len()
            );
            assert_eq!(
                loaded_payload.day_of_year_factor.len(),
                expected_payload.day_of_year_factor.len()
            );
            assert_eq!(
                loaded_payload.timestamp_utc_nanos,
                expected_payload.timestamp_utc_nanos
            );
            assert_eq!(
                loaded_payload.normalized_combined_factor_by_event,
                expected_payload.normalized_combined_factor_by_event
            );
        }
        _ => panic!("expected seasonality payload"),
    }

    assert_eq!(loaded_hawkes.run_id, hawkes_artifact.run_id);
    assert_eq!(loaded_hawkes.stage, CalibrationStage::Hawkes);
    assert_eq!(
        loaded_hawkes.artifact_reference,
        hawkes_artifact.artifact_reference
    );
    match (
        &loaded_hawkes.stage_parameters,
        &hawkes_artifact.stage_parameters,
    ) {
        (
            CalibrationStageParameterPayload::Hawkes(loaded_payload),
            CalibrationStageParameterPayload::Hawkes(expected_payload),
        ) => {
            assert_eq!(
                loaded_payload.selected_candidate_id,
                expected_payload.selected_candidate_id
            );
            assert_eq!(
                loaded_payload.fitted_parameters,
                expected_payload.fitted_parameters
            );
        }
        _ => panic!("expected hawkes payload"),
    }

    assert_eq!(
        loaded_return_components.run_id,
        return_components_artifact.run_id
    );
    assert_eq!(
        loaded_return_components.stage,
        CalibrationStage::ReturnComponents
    );
    assert_eq!(
        loaded_return_components.artifact_reference,
        return_components_artifact.artifact_reference
    );
    match (
        &loaded_return_components.stage_parameters,
        &return_components_artifact.stage_parameters,
    ) {
        (
            CalibrationStageParameterPayload::ReturnComponents(loaded_payload),
            CalibrationStageParameterPayload::ReturnComponents(expected_payload),
        ) => {
            assert_eq!(
                loaded_payload.selected_candidate_id,
                expected_payload.selected_candidate_id
            );
            assert_eq!(
                loaded_payload.fitted_parameters,
                expected_payload.fitted_parameters
            );
        }
        _ => panic!("expected return-components payload"),
    }

    let _ = fs::remove_dir_all(checkpoint_directory);
}

#[test]
fn calibration_stage_parameter_artifact_validation_rejects_missing_outputs_and_malformed_payloads()
{
    let calendar_registry = calibration_stage_test_registry();
    let run_contract =
        build_run_contract_with_mode_and_resume("run-001", CalibrationExecutionMode::Disk, None);

    let mut hawkes_report = run_hawkes_calibration_stage(
        &run_contract.run_spec,
        &calendar_registry,
        HawkesStageExecutionInput::from_persisted_seasonality_artifact_reference(
            "artifact://seasonality/run-001",
            build_seasonality_stage_success_payload(),
            hawkes_stage_candidate_fixture(),
        ),
    );
    hawkes_report.fitted_stage_payload = None;

    assert_eq!(
        build_calibration_stage_parameter_artifact(
            &run_contract,
            &CalibrationStagedRunStageReport::Hawkes(hawkes_report),
        )
        .expect_err("missing hawkes payload should fail"),
        CalibrationStageParameterArtifactBuildError::ValidationFailed {
            stage: CalibrationStage::Hawkes,
            error_message: "successful hawkes stage is missing its payload".to_string(),
        }
    );

    let valid_hawkes_artifact = build_calibration_stage_parameter_artifact(
        &run_contract,
        &CalibrationStagedRunStageReport::Hawkes(run_hawkes_calibration_stage(
            &run_contract.run_spec,
            &calendar_registry,
            HawkesStageExecutionInput::from_persisted_seasonality_artifact_reference(
                "artifact://seasonality/run-001",
                build_seasonality_stage_success_payload(),
                hawkes_stage_candidate_fixture(),
            ),
        )),
    )
    .expect("valid hawkes artifact should build");

    let mut missing_optimizer_state = valid_hawkes_artifact.clone();
    missing_optimizer_state.optimizer_state = None;

    assert_eq!(
        missing_optimizer_state
            .validate()
            .expect_err("hawkes artifact requires optimizer state"),
        CalibrationStageParameterArtifactValidationError::MissingOptimizerState {
            stage: CalibrationStage::Hawkes,
        }
    );

    let valid_return_components_artifact = build_calibration_stage_parameter_artifact(
        &run_contract,
        &CalibrationStagedRunStageReport::ReturnComponents(
            run_return_components_calibration_stage(
                &run_contract.run_spec,
                &calendar_registry,
                ReturnComponentsStageExecutionInput::from_persisted_activity_artifact_reference(
                    "artifact://hawkes/run-001",
                    return_components_stage_candidate_generation_fixture(),
                ),
            ),
        ),
    )
    .expect("valid return-components artifact should build");

    let mut malformed_return_components = valid_return_components_artifact.clone();
    match &mut malformed_return_components.stage_parameters {
        CalibrationStageParameterPayload::ReturnComponents(payload) => {
            payload.activity_artifact_reference = None;
        }
        _ => panic!("expected return-components payload"),
    }

    assert_eq!(
        malformed_return_components
            .validate()
            .expect_err("persisted activity boundary must remain explicit"),
        CalibrationStageParameterArtifactValidationError::ReturnComponentsBoundaryMismatch {
            expected_transport: "persisted_artifact_reference",
        }
    );
}
