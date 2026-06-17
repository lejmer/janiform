mod support;

use std::collections::BTreeSet;

use janiform_calibration_artifacts::{
    CALIBRATION_OBSERVATION_MODELS_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_SEASONALITY_STAGE_DEFAULT_ESTIMATOR_VERSION,
    CALIBRATION_SEASONALITY_STAGE_OUTPUT_SCHEMA_VERSION, CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
    CalibrationExecutionMode, CalibrationStage, CalibrationStageParameterBuildMetadata,
    ObservationModelsStageDiagnostics, ObservationModelsStageParameterArtifactInput,
    SeasonalityStageParameterArtifactInput, build_calibration_stage_parameter_artifact,
    load_calibration_stage_parameter_artifact_from_disk,
    persist_calibration_stage_parameter_artifact_to_disk,
};
use janiform_market_time::business_time_clock::FOURIER_BUSINESS_TIME_CLOCK_MODEL_SCHEMA_VERSION;
use janiform_model_kernels::timestamp_observation::TimestampBatchingObservationModel;
use serde_json::Value;
use support::shared::{
    build_run_contract_with_mode_and_resume, build_solver_settings, flat_business_time_clock_model,
    fs, resolve_calibration_stage_seed_metadata, temporary_checkpoint_directory,
};

#[test]
fn seasonality_stage_parameter_artifact_round_trips_compact_clock_model_payload() {
    let run_contract = build_run_contract_with_mode_and_resume(
        "seasonality-compact-stage-parameter",
        CalibrationExecutionMode::Disk,
        None,
    );

    let stage_seed_metadata = resolve_calibration_stage_seed_metadata(
        &run_contract.run_spec,
        CalibrationStage::Seasonality,
    );

    let artifact = build_calibration_stage_parameter_artifact(
        &run_contract,
        SeasonalityStageParameterArtifactInput {
            metadata: CalibrationStageParameterBuildMetadata {
                stage_output_schema_version: CALIBRATION_SEASONALITY_STAGE_OUTPUT_SCHEMA_VERSION
                    .to_string(),
                run_global_seed: stage_seed_metadata.run_global_seed,
                stage_seed: stage_seed_metadata.stage_seed,
                stage_seed_source: stage_seed_metadata.stage_seed_source,
                stage_seed_derivation_policy: CALIBRATION_STAGE_SEED_DERIVATION_POLICY.to_string(),
                solver_settings: build_solver_settings(),
            },
            dataset_id: "dataset://stage_parameter_compaction/seasonality".to_string(),
            exchange_code: "XNYS".to_string(),
            trade_events_data_reference: "dataset://trades/seasonality-compact".to_string(),
            estimator_version: CALIBRATION_SEASONALITY_STAGE_DEFAULT_ESTIMATOR_VERSION.to_string(),
            timestamp_event_input_schema_version: "market_time.timestamp_event_input.v1"
                .to_string(),
            timestamp_event_preprocessed_schema_version:
                "calibration.timestamp_event_preprocessed.v1".to_string(),
            clock_model_schema_version: FOURIER_BUSINESS_TIME_CLOCK_MODEL_SCHEMA_VERSION
                .to_string(),
            clock_model: flat_business_time_clock_model(),
        },
    )
    .expect("seasonality artifact should build");

    let checkpoint_directory =
        temporary_checkpoint_directory("seasonality-compact-stage-parameter");

    let artifact_path = persist_calibration_stage_parameter_artifact_to_disk(
        checkpoint_directory.as_path(),
        &artifact,
    )
    .expect("seasonality artifact should persist");

    let artifact_json = fs::read_to_string(artifact_path.as_path())
        .expect("seasonality artifact json should be readable");

    let serialized_artifact: Value =
        serde_json::from_str(artifact_json.as_str()).expect("artifact json should parse");

    assert_eq!(
        serialized_artifact["stage_parameters"]["payload_kind"],
        Value::from("seasonality")
    );

    let seasonality_payload = serialized_artifact["stage_parameters"]["payload"]
        .as_object()
        .expect("seasonality payload should serialize as an object");

    let payload_keys = seasonality_payload.keys().cloned().collect::<BTreeSet<_>>();

    assert_eq!(
        payload_keys,
        BTreeSet::from([
            "clock_model".to_string(),
            "clock_model_schema_version".to_string(),
            "dataset_id".to_string(),
            "estimator_version".to_string(),
            "exchange_code".to_string(),
            "timestamp_event_input_schema_version".to_string(),
            "timestamp_event_preprocessed_schema_version".to_string(),
            "trade_events_data_reference".to_string(),
        ])
    );

    let loaded_artifact =
        load_calibration_stage_parameter_artifact_from_disk(artifact_path.as_path())
            .expect("seasonality artifact should load");

    assert_eq!(loaded_artifact, artifact);

    let _ = fs::remove_dir_all(checkpoint_directory);
}

#[test]
fn observation_models_stage_parameter_artifact_round_trips_compact_model_payload() {
    let run_contract = build_run_contract_with_mode_and_resume(
        "observation-models-compact-stage-parameter",
        CalibrationExecutionMode::Disk,
        None,
    );

    let stage_seed_metadata = resolve_calibration_stage_seed_metadata(
        &run_contract.run_spec,
        CalibrationStage::ObservationModels,
    );

    let artifact = build_calibration_stage_parameter_artifact(
        &run_contract,
        ObservationModelsStageParameterArtifactInput {
            metadata: CalibrationStageParameterBuildMetadata {
                stage_output_schema_version:
                    CALIBRATION_OBSERVATION_MODELS_STAGE_OUTPUT_SCHEMA_VERSION.to_string(),
                run_global_seed: stage_seed_metadata.run_global_seed,
                stage_seed: stage_seed_metadata.stage_seed,
                stage_seed_source: stage_seed_metadata.stage_seed_source,
                stage_seed_derivation_policy: CALIBRATION_STAGE_SEED_DERIVATION_POLICY.to_string(),
                solver_settings: build_solver_settings(),
            },
            timestamp_batching_observation_model: TimestampBatchingObservationModel::disabled(),
            diagnostics: ObservationModelsStageDiagnostics {
                estimation_policy: "stage_parameter_compaction_test.v1".to_string(),
                observed_open_session_return_count: 2,
                same_session_timestamp_transition_count: 2,
                same_timestamp_continuation_count: 0,
            },
        },
    )
    .expect("observation-models artifact should build");

    let checkpoint_directory =
        temporary_checkpoint_directory("observation-models-compact-stage-parameter");

    let artifact_path = persist_calibration_stage_parameter_artifact_to_disk(
        checkpoint_directory.as_path(),
        &artifact,
    )
    .expect("observation-models artifact should persist");

    let artifact_json = fs::read_to_string(artifact_path.as_path())
        .expect("observation-models artifact json should be readable");

    let serialized_artifact: Value =
        serde_json::from_str(artifact_json.as_str()).expect("artifact json should parse");

    assert_eq!(
        serialized_artifact["stage_parameters"]["payload_kind"],
        Value::from("observation_models")
    );

    let observation_models_payload = serialized_artifact["stage_parameters"]["payload"]
        .as_object()
        .expect("observation-models payload should serialize as an object");

    let payload_keys = observation_models_payload
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();

    assert_eq!(
        payload_keys,
        BTreeSet::from([
            "diagnostics".to_string(),
            "timestamp_batching_observation_model".to_string(),
        ])
    );

    let loaded_artifact =
        load_calibration_stage_parameter_artifact_from_disk(artifact_path.as_path())
            .expect("observation-models artifact should load");

    assert_eq!(loaded_artifact, artifact);

    let _ = fs::remove_dir_all(checkpoint_directory);
}
