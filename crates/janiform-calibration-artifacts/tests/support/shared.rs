#![allow(dead_code, unused_imports)]

pub(crate) use std::fs;
pub(crate) use std::path::PathBuf;
pub(crate) use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) use chrono::{TimeZone, Utc};
pub(crate) use janiform_calibration_artifacts::{
    CALIBRATION_ARTIFACT_ATOMIC_COMMIT_POLICY, CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_FAMILY,
    CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_GENERATION, CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION,
    CALIBRATION_ARTIFACT_PARTIAL_WRITE_RECOVERY_POLICY,
    CALIBRATION_ARTIFACT_ROOT_CONFIGURATION_POLICY, CALIBRATION_ARTIFACT_STORAGE_LAYOUT_POLICY,
    CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_RUN_SCHEMA_VERSION,
    CALIBRATION_DATASET_REFERENCE_CHECKSUM_SCOPE,
    CALIBRATION_DATASET_SNAPSHOT_ARTIFACT_SCHEMA_FAMILY,
    CALIBRATION_DATASET_SNAPSHOT_ARTIFACT_SCHEMA_GENERATION,
    CALIBRATION_DATASET_SNAPSHOT_ARTIFACT_SCHEMA_VERSION,
    CALIBRATION_DATASET_SNAPSHOT_CHECKSUM_ALGORITHM, CALIBRATION_DATASET_SNAPSHOT_CHECKSUM_SCOPE,
    CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY,
    CALIBRATION_DATASET_SNAPSHOT_OUTPUT_SCHEMA_METADATA_VERSION,
    CALIBRATION_DATASET_SNAPSHOT_PERSISTENCE_SCOPE_POLICY,
    CALIBRATION_DATASET_SNAPSHOT_STAGE_INPUT_NAMING_POLICY,
    CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY, CALIBRATION_EXECUTION_MODE_DEFAULT_POLICY,
    CALIBRATION_EXPERIMENT_INDEX_ORDERING_POLICY, CALIBRATION_EXPERIMENT_INDEX_REBUILD_POLICY,
    CALIBRATION_EXPERIMENT_INDEX_SCHEMA_FAMILY, CALIBRATION_EXPERIMENT_INDEX_SCHEMA_GENERATION,
    CALIBRATION_EXPERIMENT_INDEX_SCHEMA_VERSION,
    CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_HAWKES_STAGE_HANDOFF_POLICY, CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_JOINT_REFINEMENT_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_JOINT_REFINEMENT_STAGE_HANDOFF_POLICY,
    CALIBRATION_JOINT_REFINEMENT_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_MEAN_REVERSION_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_MEAN_REVERSION_STAGE_HANDOFF_POLICY,
    CALIBRATION_MEAN_REVERSION_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_RETURN_COMPONENTS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_RETURN_COMPONENTS_STAGE_HANDOFF_POLICY,
    CALIBRATION_RETURN_COMPONENTS_STAGE_OBJECTIVE_WEIGHTING_POLICY,
    CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION, CALIBRATION_RUN_DATA_INPUTS_SCHEMA_VERSION,
    CALIBRATION_RUN_SPEC_SCHEMA_VERSION, CALIBRATION_SEASONALITY_STAGE_DEFAULT_ESTIMATOR_VERSION,
    CALIBRATION_SEASONALITY_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_SEASONALITY_STAGE_HANDOFF_POLICY,
    CALIBRATION_SEASONALITY_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_SOLVER_SETTINGS_CAPTURE_POLICY, CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY,
    CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION, CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION,
    CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_FAMILY,
    CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_GENERATION,
    CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_VERSION,
    CALIBRATION_STAGE_PARAMETER_OPTIMIZER_STATE_SCHEMA_VERSION,
    CALIBRATION_STAGE_PARAMETER_OUTPUT_SCHEMA_METADATA_VERSION,
    CALIBRATION_STAGE_SEED_DERIVATION_POLICY, CALIBRATION_STAGED_RUN_FAILURE_TRIAGE_POLICY,
    CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION,
    CALIBRATION_VALIDATION_METRIC_ARTIFACT_OUTPUT_SCHEMA_METADATA_VERSION,
    CALIBRATION_VALIDATION_METRIC_ARTIFACT_SCHEMA_FAMILY,
    CALIBRATION_VALIDATION_METRIC_ARTIFACT_SCHEMA_GENERATION,
    CALIBRATION_VALIDATION_METRIC_ARTIFACT_SCHEMA_VERSION, CalibrationArtifactIoError,
    CalibrationArtifactKind, CalibrationArtifactManifest, CalibrationArtifactManifestBuildError,
    CalibrationArtifactManifestError, CalibrationArtifactManifestIndexMetadata,
    CalibrationArtifactManifestMetricStatus, CalibrationArtifactManifestMetricStatusCount,
    CalibrationArtifactManifestModelVersion, CalibrationArtifactManifestValidationMetricRole,
    CalibrationArtifactManifestValidationMetricSummary, CalibrationArtifactPersistenceKind,
    CalibrationArtifactReference, CalibrationArtifactReloadComponent,
    CalibrationArtifactReloadError, CalibrationArtifactReloadRequest,
    CalibrationArtifactReloadRequestError, CalibrationArtifactReloadSelection,
    CalibrationArtifactRunPreparationMode, CalibrationArtifactRunWriter,
    CalibrationArtifactRunWriterError, CalibrationCallerSurface,
    CalibrationCheckpointReferenceParseError, CalibrationDatasetInputName,
    CalibrationDatasetInputSnapshot, CalibrationDatasetSnapshotArtifact,
    CalibrationDatasetSnapshotArtifactValidationError,
    CalibrationDatasetSnapshotOutputSchemaMetadata, CalibrationDatasetSnapshotProvenance,
    CalibrationDatasetSnapshotSchemaMetadata, CalibrationExecutionMode,
    CalibrationExperimentIndexDocument, CalibrationExperimentIndexError,
    CalibrationExperimentIndexLookupRequest, CalibrationExperimentIndexLookupResponse,
    CalibrationExperimentIndexRecord, CalibrationOptimizerSettings,
    CalibrationOptimizerSettingsError, CalibrationReloadedArtifactRun,
    CalibrationResolvedStageSeed, CalibrationResumeSettings, CalibrationResumeSettingsError,
    CalibrationResumeSource, CalibrationRunContract, CalibrationRunContractError,
    CalibrationRunDataInputs, CalibrationRunDataInputsError, CalibrationRunSeedSettings,
    CalibrationRunSeedSettingsError, CalibrationRunSpec, CalibrationRunSpecBuilder,
    CalibrationRunSpecError, CalibrationRunSpecErrorClassification, CalibrationStage,
    CalibrationStageCheckpointCompatibilityError, CalibrationStageCheckpointMetadata,
    CalibrationStageDependency, CalibrationStageGraph, CalibrationStageGraphError,
    CalibrationStageHandoff, CalibrationStageHandoffError, CalibrationStageHandoffRoute,
    CalibrationStageHandoffTransport, CalibrationStageParameterArtifact,
    CalibrationStageParameterArtifactBuildError, CalibrationStageParameterArtifactInput,
    CalibrationStageParameterArtifactValidationError, CalibrationStageParameterBuildMetadata,
    CalibrationStageParameterOptimizerAdapterMetadata, CalibrationStageParameterOptimizerState,
    CalibrationStageParameterOptimizerStateInput, CalibrationStageParameterOutputSchemaMetadata,
    CalibrationStageParameterPayload, CalibrationStageParameterSchemaMetadata,
    CalibrationStageParameterSeedMetadata, CalibrationStageSeedOverride,
    CalibrationStageSeedSource, CalibrationStageSolverSettings,
    CalibrationValidationMetricArtifact,
    CalibrationValidationMetricArtifactReferenceIntegrityError,
    CalibrationValidationMetricArtifactRole, CalibrationValidationMetricArtifactValidationError,
    CalibrationValidationMetricCollection, CalibrationValidationMetricConfidenceInterval,
    CalibrationValidationMetricContext, CalibrationValidationMetricContextKind,
    CalibrationValidationMetricManifestLink, CalibrationValidationMetricOutputSchemaMetadata,
    CalibrationValidationMetricRecord, CalibrationValidationMetricSchemaMetadata,
    CalibrationValidationMetricSourceKind, CalibrationValidationMetricStageOutputLink,
    CalibrationValidationMetricStatus, CalibrationValidationMetricThresholdMetadata,
    CalibrationValidationObservedValue, HawkesStageFittedParameters,
    HawkesStageParameterArtifactInput, HawkesStageParameterPayload, JointRefinementStageCarryInput,
    JointRefinementStageFittedParameters, JointRefinementStageLabeledCount,
    JointRefinementStageObjectiveBreakdown, JointRefinementStageOptimizerDiagnostics,
    JointRefinementStageOptionAggregateDiagnostic,
    JointRefinementStageOptionCalibrationDiagnostics, JointRefinementStageOptionNodeDiagnostic,
    JointRefinementStageOptionSnapshotMetadata, JointRefinementStageParameterArtifactInput,
    JointRefinementStageParameterPayload, JointRefinementStagePricingBlockFittedParameters,
    JointRefinementStageRestartDiagnostics, JointRefinementStageRuntimeProfile,
    MeanReversionStageCalibrationGrid, MeanReversionStageCandidateDiagnostics,
    MeanReversionStageCandidateSeriesDiagnostics, MeanReversionStageDiagnostics,
    MeanReversionStageGridDiagnostics, MeanReversionStageParameterArtifactInput,
    MeanReversionStageParameterPayload, MeanReversionStageSeriesSummary,
    ReturnComponentsStageFittedParameters, ReturnComponentsStageParameterArtifactInput,
    ReturnComponentsStageParameterPayload, SeasonalityStageNormalizationMetadata,
    SeasonalityStageParameterArtifactInput, SeasonalityStageParameterPayload,
    SurpriseNewsDeterministicIntensityProfile, TickGridObservationModel,
    build_calibration_artifact_manifest, build_calibration_dataset_snapshot_artifact,
    build_calibration_stage_parameter_artifact, calibration_artifact_manifest_path,
    calibration_artifact_manifest_reference, calibration_checkpoint_run_directory,
    calibration_committed_artifact_run_directory,
    calibration_committed_artifact_runs_root_directory, calibration_dataset_snapshot_artifact_path,
    calibration_dataset_snapshot_artifact_reference, calibration_experiment_index_path,
    calibration_experiment_index_path_for_root, calibration_experiment_index_root_directory,
    calibration_stage_checkpoint_reference, calibration_stage_parameter_artifact_path,
    calibration_stage_parameter_artifact_path_in_run_directory, calibration_stage_token,
    calibration_validation_metric_artifact_path, calibration_validation_metric_artifact_reference,
    default_calibration_execution_mode_for_surface, derive_calibration_stage_seed_from_global_seed,
    load_calibration_artifact_manifest_from_disk,
    load_calibration_dataset_snapshot_artifact_from_disk,
    load_calibration_experiment_index_from_disk,
    load_calibration_stage_parameter_artifact_from_disk,
    load_calibration_validation_metric_artifact_from_disk,
    lookup_calibration_experiment_index_records, persist_calibration_artifact_manifest_to_disk,
    persist_calibration_artifact_manifest_to_run_directory,
    persist_calibration_dataset_snapshot_artifact_to_disk,
    persist_calibration_dataset_snapshot_artifact_to_run_directory,
    persist_calibration_dataset_snapshot_from_run_contract_to_disk,
    persist_calibration_experiment_index_to_disk,
    persist_calibration_stage_parameter_artifact_to_disk,
    persist_calibration_stage_parameter_artifact_to_run_directory,
    persist_calibration_validation_metric_artifact_to_disk,
    rebuild_calibration_experiment_index_from_disk, reload_calibration_artifact_run_from_disk,
    resolve_calibration_execution_mode_for_surface, resolve_calibration_stage_execution_order,
    resolve_calibration_stage_seed, resolve_calibration_stage_seed_metadata,
    stage_output_artifact_references_for_stage, stage_solver_settings_for_stage,
    update_calibration_experiment_index_for_run,
};
pub(crate) use janiform_foundations::time::utc_nanos;
pub(crate) use janiform_model_kernels::mean_reversion::{
    MeanReversionAnchorModel, MeanReversionBoundaryState, MeanReversionFittedParameters,
    MeanReversionRuntimeIntervalAnchors,
};
use janiform_model_kernels::news::{
    KnownNewsArrivalConfiguration, NewsArrivalConfiguration, NewsComponentMarkScaleConfiguration,
    NewsProcessConfiguration, SharedNewsMarkLawConfiguration, SurpriseNewsArrivalConfiguration,
};
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
use janiform_model_kernels::return_clock::ReturnClockExposureParameters;
pub(crate) use serde_json::json;

pub(crate) fn build_data_inputs() -> CalibrationRunDataInputs {
    CalibrationRunDataInputs::try_new(
        "XNYS",
        "dataset://synthetic/trades/v1",
        None,
        Some("dataset://synthetic/options/v1".to_string()),
    )
    .expect("data inputs should validate")
}

fn build_single_stage_seed_settings() -> CalibrationRunSeedSettings {
    CalibrationRunSeedSettings::try_new(
        17,
        vec![CalibrationStageSeedOverride {
            stage: CalibrationStage::Seasonality,
            seed: 101,
        }],
    )
    .expect("single-stage seed settings should validate")
}

pub(crate) fn build_seed_settings() -> CalibrationRunSeedSettings {
    CalibrationRunSeedSettings::try_new(
        17,
        vec![
            CalibrationStageSeedOverride {
                stage: CalibrationStage::Seasonality,
                seed: 101,
            },
            CalibrationStageSeedOverride {
                stage: CalibrationStage::Hawkes,
                seed: 202,
            },
        ],
    )
    .expect("seed settings should validate")
}

pub(crate) fn build_solver_settings() -> CalibrationOptimizerSettings {
    CalibrationOptimizerSettings::try_new("lbfgs", 64, 1, 1e-8, 1e-6, 10.0)
        .expect("solver settings should validate")
}

pub(crate) fn build_joint_refinement_solver_settings() -> CalibrationOptimizerSettings {
    CalibrationOptimizerSettings::try_new("lbfgs", 32, 1, 1e-8, 1e-6, 10.0)
        .expect("joint-refinement solver settings should validate")
}

pub(crate) fn build_solver_settings_for_seasonality_and_hawkes()
-> Vec<CalibrationStageSolverSettings> {
    vec![
        CalibrationStageSolverSettings {
            stage: CalibrationStage::Seasonality,
            optimizer_settings: build_solver_settings(),
        },
        CalibrationStageSolverSettings {
            stage: CalibrationStage::Hawkes,
            optimizer_settings: build_solver_settings(),
        },
    ]
}

pub(crate) fn build_solver_settings_for_all_stages() -> Vec<CalibrationStageSolverSettings> {
    vec![
        CalibrationStageSolverSettings {
            stage: CalibrationStage::Seasonality,
            optimizer_settings: build_solver_settings(),
        },
        CalibrationStageSolverSettings {
            stage: CalibrationStage::Hawkes,
            optimizer_settings: build_solver_settings(),
        },
        CalibrationStageSolverSettings {
            stage: CalibrationStage::ReturnComponents,
            optimizer_settings: build_solver_settings(),
        },
        CalibrationStageSolverSettings {
            stage: CalibrationStage::MeanReversion,
            optimizer_settings: build_solver_settings(),
        },
        CalibrationStageSolverSettings {
            stage: CalibrationStage::JointRefinement,
            optimizer_settings: build_joint_refinement_solver_settings(),
        },
    ]
}

pub(crate) fn build_solver_settings_for_pre_joint_pipeline() -> Vec<CalibrationStageSolverSettings>
{
    vec![
        CalibrationStageSolverSettings {
            stage: CalibrationStage::Seasonality,
            optimizer_settings: build_solver_settings(),
        },
        CalibrationStageSolverSettings {
            stage: CalibrationStage::Hawkes,
            optimizer_settings: build_solver_settings(),
        },
        CalibrationStageSolverSettings {
            stage: CalibrationStage::ReturnComponents,
            optimizer_settings: build_solver_settings(),
        },
    ]
}

pub(crate) fn build_valid_run_spec() -> CalibrationRunSpec {
    build_valid_run_spec_with_mode(CalibrationExecutionMode::Memory)
}

pub(crate) fn build_valid_run_spec_with_mode(
    execution_mode: CalibrationExecutionMode,
) -> CalibrationRunSpec {
    CalibrationRunSpec::try_new(
        "run-001",
        build_data_inputs(),
        execution_mode,
        vec![
            CalibrationStage::Seasonality,
            CalibrationStage::Hawkes,
            CalibrationStage::ReturnComponents,
            CalibrationStage::MeanReversion,
            CalibrationStage::JointRefinement,
        ],
        build_seed_settings(),
        build_solver_settings_for_all_stages(),
    )
    .expect("run spec should validate")
}

pub(crate) fn build_endogenous_mean_reversion_parameters() -> MeanReversionFittedParameters {
    MeanReversionFittedParameters::try_new(
        0.25,
        MeanReversionAnchorModel::endogenous_core_pressure_level(24.0, 0.5, 0.75),
    )
    .expect("mean reversion fitted parameters should validate")
}

pub(crate) fn build_valid_artifact_manifest() -> CalibrationArtifactManifest {
    let run_contract =
        build_run_contract_with_mode_and_resume("run-001", CalibrationExecutionMode::Disk, None);
    let dataset_snapshot =
        build_calibration_dataset_snapshot_artifact(&run_contract).expect("dataset snapshot");

    CalibrationArtifactManifest::try_new(
        "run-001",
        CalibrationArtifactManifestIndexMetadata {
            committed_at_utc: "2026-03-05T12:30:00.000000000Z".to_string(),
            committed_at_unix_nanos: utc_nanos(2026, 3, 5, 12, 30, 0),
            exchange_code: dataset_snapshot.exchange_code.clone(),
            dataset_snapshot_checksum: dataset_snapshot.snapshot_checksum.clone(),
            trade_events_data_reference: run_contract
                .run_spec
                .data_inputs
                .trade_events_data_reference
                .clone(),
            trade_events_reference_checksum: dataset_snapshot
                .input_datasets
                .iter()
                .find(|input_dataset| {
                    input_dataset.input_name
                        == CalibrationDatasetInputName::TradeEventsDataReference
                })
                .expect("trade events dataset should exist")
                .reference_checksum
                .clone(),
            option_surface_data_reference: run_contract
                .run_spec
                .data_inputs
                .option_surface_data_reference
                .clone(),
            option_surface_reference_checksum: dataset_snapshot
                .input_datasets
                .iter()
                .find(|input_dataset| {
                    input_dataset.input_name
                        == CalibrationDatasetInputName::OptionSurfaceDataReference
                })
                .map(|input_dataset| input_dataset.reference_checksum.clone()),
            model_version: CalibrationArtifactManifestModelVersion {
                run_contract_schema_version: CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION.to_string(),
                run_spec_schema_version: CALIBRATION_RUN_SPEC_SCHEMA_VERSION.to_string(),
                seasonality_stage_output_schema_version: Some(
                    CALIBRATION_SEASONALITY_STAGE_OUTPUT_SCHEMA_VERSION.to_string(),
                ),
                hawkes_stage_output_schema_version: Some(
                    CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION.to_string(),
                ),
                return_components_stage_output_schema_version: Some(
                    CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION.to_string(),
                ),
                mean_reversion_stage_output_schema_version: Some(
                    CALIBRATION_MEAN_REVERSION_STAGE_OUTPUT_SCHEMA_VERSION.to_string(),
                ),
                joint_refinement_stage_output_schema_version: Some(
                    CALIBRATION_JOINT_REFINEMENT_STAGE_OUTPUT_SCHEMA_VERSION.to_string(),
                ),
                option_refinement_stage_output_schema_version: None,
                finalization_stage_output_schema_version: None,
                seasonality_estimator_version: Some(
                    CALIBRATION_SEASONALITY_STAGE_DEFAULT_ESTIMATOR_VERSION.to_string(),
                ),
            },
            validation_metric_summaries: vec![CalibrationArtifactManifestValidationMetricSummary {
                artifact_role: CalibrationArtifactManifestValidationMetricRole::Diagnostics,
                artifact_reference: "artifact://diagnostics/run-001".to_string(),
                overall_status: CalibrationArtifactManifestMetricStatus::Fail,
                collection_count: 1,
                metric_count: 2,
                collection_status_counts: vec![CalibrationArtifactManifestMetricStatusCount {
                    status: CalibrationArtifactManifestMetricStatus::Fail,
                    count: 1,
                }],
                metric_status_counts: vec![
                    CalibrationArtifactManifestMetricStatusCount {
                        status: CalibrationArtifactManifestMetricStatus::Pass,
                        count: 1,
                    },
                    CalibrationArtifactManifestMetricStatusCount {
                        status: CalibrationArtifactManifestMetricStatus::Fail,
                        count: 1,
                    },
                ],
                failing_metric_ids: vec!["tail_skew.hourly".to_string()],
            }],
        },
        vec![
            CalibrationArtifactReference {
                artifact_id: "run_contract".to_string(),
                artifact_kind: CalibrationArtifactKind::RunContract,
                artifact_reference: "artifact://run_contract/run-001".to_string(),
                stage: None,
            },
            CalibrationArtifactReference {
                artifact_id: "seasonality_checkpoint".to_string(),
                artifact_kind: CalibrationArtifactKind::StageCheckpoint,
                artifact_reference: "artifact://stage_checkpoint/seasonality/run-001".to_string(),
                stage: Some(CalibrationStage::Seasonality),
            },
            CalibrationArtifactReference {
                artifact_id: "stylized_fact_report".to_string(),
                artifact_kind: CalibrationArtifactKind::Diagnostics,
                artifact_reference: "artifact://diagnostics/stylized_facts/run-001".to_string(),
                stage: None,
            },
        ],
    )
    .expect("artifact manifest should validate")
}

pub(crate) fn build_validation_metric_artifact_fixture() -> CalibrationValidationMetricArtifact {
    CalibrationValidationMetricArtifact::try_new(
        "run-001",
        CalibrationValidationMetricArtifactRole::Diagnostics,
        vec![
            CalibrationValidationMetricStageOutputLink {
                stage: CalibrationStage::Seasonality,
                artifact_reference: "artifact://seasonality/run-001".to_string(),
                stage_output_schema_version:
                    CALIBRATION_SEASONALITY_STAGE_EXECUTION_REPORT_SCHEMA_VERSION.to_string(),
            },
            CalibrationValidationMetricStageOutputLink {
                stage: CalibrationStage::Hawkes,
                artifact_reference: "artifact://hawkes/run-001".to_string(),
                stage_output_schema_version:
                    CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION.to_string(),
            },
            CalibrationValidationMetricStageOutputLink {
                stage: CalibrationStage::ReturnComponents,
                artifact_reference: "artifact://return_components/run-001".to_string(),
                stage_output_schema_version:
                    CALIBRATION_RETURN_COMPONENTS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION.to_string(),
            },
            CalibrationValidationMetricStageOutputLink {
                stage: CalibrationStage::MeanReversion,
                artifact_reference: "artifact://mean_reversion/run-001".to_string(),
                stage_output_schema_version:
                    CALIBRATION_MEAN_REVERSION_STAGE_EXECUTION_REPORT_SCHEMA_VERSION.to_string(),
            },
            CalibrationValidationMetricStageOutputLink {
                stage: CalibrationStage::JointRefinement,
                artifact_reference: "artifact://joint_refinement/run-001".to_string(),
                stage_output_schema_version:
                    CALIBRATION_JOINT_REFINEMENT_STAGE_EXECUTION_REPORT_SCHEMA_VERSION.to_string(),
            },
        ],
        vec![CalibrationValidationMetricCollection {
            collection_id: "activity-clustering-1h".to_string(),
            source_kind: CalibrationValidationMetricSourceKind::ActivityClustering,
            source_schema_version: "validation.activity_clustering.report.v1".to_string(),
            status: CalibrationValidationMetricStatus::Pass,
            context: CalibrationValidationMetricContext {
                context_kind: CalibrationValidationMetricContextKind::Horizon,
                context_id: "1h".to_string(),
                context_label: "1 Hour".to_string(),
            },
            primary_role_label: "empirical".to_string(),
            secondary_role_label: Some("simulated".to_string()),
            threshold_policy_id: "activity_clustering.default.v1".to_string(),
            metrics: vec![CalibrationValidationMetricRecord {
                metric_id: "returns_abs_quantile_shape_ratio_q99_q95".to_string(),
                metric_name: "Returns Abs Quantile Shape Ratio Q99 Q95".to_string(),
                metric_variant: None,
                status: CalibrationValidationMetricStatus::Pass,
                primary_value: CalibrationValidationObservedValue {
                    value: Some(1.18),
                    confidence_interval: Some(CalibrationValidationMetricConfidenceInterval {
                        lower_bound: 1.11,
                        upper_bound: 1.24,
                        confidence_level: 0.95,
                    }),
                },
                secondary_value: Some(CalibrationValidationObservedValue {
                    value: Some(1.16),
                    confidence_interval: Some(CalibrationValidationMetricConfidenceInterval {
                        lower_bound: 1.10,
                        upper_bound: 1.22,
                        confidence_level: 0.95,
                    }),
                }),
                delta_value: Some(-0.02),
                absolute_delta: Some(0.02),
                standardized_delta: Some(0.31),
                threshold_metadata: CalibrationValidationMetricThresholdMetadata {
                    threshold_policy_id: "activity_clustering.default.v1".to_string(),
                    threshold_policy_version: Some("1.0.0".to_string()),
                    threshold_id: Some("abs_delta_q99_q95".to_string()),
                    threshold_rationale: Some(
                        "Keep tail-shape gap below approved threshold".to_string(),
                    ),
                    max_absolute_delta: Some(0.05),
                    max_standardized_delta: Some(1.0),
                    absolute_delta_exceeds_threshold: Some(false),
                    standardized_delta_exceeds_threshold: Some(false),
                },
            }],
        }],
    )
    .expect("validation metric artifact fixture should validate")
}

pub(crate) fn build_validation_metric_manifest_fixture() -> CalibrationArtifactManifest {
    let index_metadata = build_valid_artifact_manifest().index_metadata;

    CalibrationArtifactManifest::try_new(
        "run-001",
        index_metadata,
        vec![
            CalibrationArtifactReference {
                artifact_id: "dataset_snapshot".to_string(),
                artifact_kind: CalibrationArtifactKind::DatasetSnapshot,
                artifact_reference: "artifact://dataset_snapshot/run-001".to_string(),
                stage: None,
            },
            CalibrationArtifactReference {
                artifact_id: "seasonality_stage_output".to_string(),
                artifact_kind: CalibrationArtifactKind::StageOutput,
                artifact_reference: "artifact://seasonality/run-001".to_string(),
                stage: Some(CalibrationStage::Seasonality),
            },
            CalibrationArtifactReference {
                artifact_id: "hawkes_stage_output".to_string(),
                artifact_kind: CalibrationArtifactKind::StageOutput,
                artifact_reference: "artifact://hawkes/run-001".to_string(),
                stage: Some(CalibrationStage::Hawkes),
            },
            CalibrationArtifactReference {
                artifact_id: "return_components_stage_output".to_string(),
                artifact_kind: CalibrationArtifactKind::StageOutput,
                artifact_reference: "artifact://return_components/run-001".to_string(),
                stage: Some(CalibrationStage::ReturnComponents),
            },
            CalibrationArtifactReference {
                artifact_id: "mean_reversion_stage_output".to_string(),
                artifact_kind: CalibrationArtifactKind::StageOutput,
                artifact_reference: "artifact://mean_reversion/run-001".to_string(),
                stage: Some(CalibrationStage::MeanReversion),
            },
            CalibrationArtifactReference {
                artifact_id: "joint_refinement_stage_output".to_string(),
                artifact_kind: CalibrationArtifactKind::StageOutput,
                artifact_reference: "artifact://joint_refinement/run-001".to_string(),
                stage: Some(CalibrationStage::JointRefinement),
            },
            CalibrationArtifactReference {
                artifact_id: "validation_metric_diagnostics".to_string(),
                artifact_kind: CalibrationArtifactKind::Diagnostics,
                artifact_reference: calibration_validation_metric_artifact_reference(
                    "run-001",
                    CalibrationValidationMetricArtifactRole::Diagnostics,
                ),
                stage: None,
            },
        ],
    )
    .expect("validation metric manifest fixture should validate")
}

pub(crate) fn build_run_contract_with_mode_and_resume(
    run_id: &str,
    execution_mode: CalibrationExecutionMode,
    resume_settings: Option<CalibrationResumeSettings>,
) -> CalibrationRunContract {
    let run_spec = {
        let builder = CalibrationRunSpec::builder(
            run_id,
            build_data_inputs(),
            execution_mode,
            vec![
                CalibrationStage::Seasonality,
                CalibrationStage::Hawkes,
                CalibrationStage::ReturnComponents,
                CalibrationStage::MeanReversion,
                CalibrationStage::JointRefinement,
            ],
            build_seed_settings(),
            build_solver_settings_for_all_stages(),
        );

        let builder = if let Some(resume_settings) = resume_settings {
            builder.with_resume_settings(resume_settings)
        } else {
            builder
        };

        builder.build().expect("run spec should validate")
    };

    CalibrationRunContract::try_new(run_spec, default_stage_graph())
        .expect("run contract should validate")
}

pub(crate) fn default_stage_graph() -> CalibrationStageGraph {
    CalibrationStageGraph::new(vec![
        CalibrationStageDependency {
            stage: CalibrationStage::Hawkes,
            depends_on: CalibrationStage::Seasonality,
        },
        CalibrationStageDependency {
            stage: CalibrationStage::ReturnComponents,
            depends_on: CalibrationStage::Hawkes,
        },
        CalibrationStageDependency {
            stage: CalibrationStage::MeanReversion,
            depends_on: CalibrationStage::ReturnComponents,
        },
        CalibrationStageDependency {
            stage: CalibrationStage::JointRefinement,
            depends_on: CalibrationStage::Hawkes,
        },
        CalibrationStageDependency {
            stage: CalibrationStage::JointRefinement,
            depends_on: CalibrationStage::ReturnComponents,
        },
        CalibrationStageDependency {
            stage: CalibrationStage::JointRefinement,
            depends_on: CalibrationStage::MeanReversion,
        },
    ])
}

pub(crate) fn build_mode_aware_stage_handoff(
    from_stage: CalibrationStage,
    to_stage: CalibrationStage,
    execution_mode: CalibrationExecutionMode,
    persisted_artifact_reference: &str,
) -> CalibrationStageHandoff {
    match execution_mode {
        CalibrationExecutionMode::Memory => {
            CalibrationStageHandoff::in_memory_output(from_stage, to_stage)
        }
        CalibrationExecutionMode::Disk => CalibrationStageHandoff::persisted_artifact_reference(
            from_stage,
            to_stage,
            persisted_artifact_reference,
        )
        .expect("disk-mode persisted handoff fixture should be valid"),
    }
}

pub(crate) fn build_single_stage_disk_run_contract(run_id: &str) -> CalibrationRunContract {
    let run_spec = CalibrationRunSpec::try_new(
        run_id,
        build_data_inputs(),
        CalibrationExecutionMode::Disk,
        vec![CalibrationStage::Seasonality],
        build_single_stage_seed_settings(),
        vec![CalibrationStageSolverSettings {
            stage: CalibrationStage::Seasonality,
            optimizer_settings: build_solver_settings(),
        }],
    )
    .expect("single-stage run spec should validate");

    CalibrationRunContract::try_new(run_spec, CalibrationStageGraph::new(Vec::new()))
        .expect("single-stage run contract should validate")
}

pub(crate) fn build_seasonality_stage_parameter_artifact_fixture(
    run_contract: &CalibrationRunContract,
) -> CalibrationStageParameterArtifact {
    let stage_seed_metadata = resolve_calibration_stage_seed_metadata(
        &run_contract.run_spec,
        CalibrationStage::Seasonality,
    );
    let solver_settings =
        stage_solver_settings_for_stage(&run_contract.run_spec, CalibrationStage::Seasonality)
            .expect("seasonality solver settings should exist");

    build_calibration_stage_parameter_artifact(
        run_contract,
        CalibrationStageParameterArtifactInput::Seasonality(Box::new(
            SeasonalityStageParameterArtifactInput {
                metadata: CalibrationStageParameterBuildMetadata {
                    stage_output_schema_version:
                        CALIBRATION_SEASONALITY_STAGE_OUTPUT_SCHEMA_VERSION.to_string(),
                    run_global_seed: stage_seed_metadata.run_global_seed,
                    stage_seed: stage_seed_metadata.stage_seed,
                    stage_seed_source: stage_seed_metadata.stage_seed_source,
                    stage_seed_derivation_policy: CALIBRATION_STAGE_SEED_DERIVATION_POLICY
                        .to_string(),
                    solver_settings,
                },
                dataset_id: format!("dataset://{}", run_contract.run_spec.run_id),
                exchange_code: run_contract.run_spec.data_inputs.exchange_code.clone(),
                trade_events_data_reference: run_contract
                    .run_spec
                    .data_inputs
                    .trade_events_data_reference
                    .clone(),
                estimator_version: CALIBRATION_SEASONALITY_STAGE_DEFAULT_ESTIMATOR_VERSION
                    .to_string(),
                timestamp_event_input_schema_version: "market_time.timestamp_event_input.v1"
                    .to_string(),
                timestamp_event_preprocessed_schema_version:
                    "calibration.timestamp_event_preprocessed.v1".to_string(),
                intraday_estimate_schema_version: "calibration.seasonality.intraday_estimate.v1"
                    .to_string(),
                day_of_week_estimate_schema_version:
                    "calibration.seasonality.day_of_week_estimate.v1".to_string(),
                day_of_year_estimate_schema_version:
                    "calibration.seasonality.day_of_year_estimate.v1".to_string(),
                composition_schema_version: "calibration.seasonality.composition.v1".to_string(),
                intraday_minutes_per_bin: 60,
                intraday_factor_by_bin: vec![0.9, 1.1],
                day_of_week_factor: vec![1.0, 0.98, 1.01, 1.02, 0.99],
                hour_of_week_factor: vec![1.0; 168],
                day_of_year_factor: vec![0.97, 1.03],
                normalization_metadata: SeasonalityStageNormalizationMetadata {
                    normalization_policy: "unit_mean_rescaling_with_log_offset_guard.v1"
                        .to_string(),
                    fit_window_event_count: 2,
                    target_mean_factor: 1.0,
                    pre_normalization_mean_factor: 1.05,
                    normalization_constant: 0.9523809524,
                    normalization_log_offset: 1e-9,
                    post_normalization_mean_factor: 1.0,
                    mean_anchor_tolerance: 1e-9,
                },
            },
        )),
    )
    .expect("seasonality stage parameter artifact should build")
}

pub(crate) fn build_return_components_stage_parameter_artifact_fixture(
    run_contract: &CalibrationRunContract,
) -> CalibrationStageParameterArtifact {
    let stage_seed_metadata = resolve_calibration_stage_seed_metadata(
        &run_contract.run_spec,
        CalibrationStage::ReturnComponents,
    );
    let solver_settings =
        stage_solver_settings_for_stage(&run_contract.run_spec, CalibrationStage::ReturnComponents)
            .expect("return-components solver settings should exist");

    let fitted_parameters = ReturnComponentsStageFittedParameters {
        nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration {
            alpha: 2.3,
            beta_0: -0.8,
            delta: 0.7,
            mu: 0.1,
            delta_beta: -0.2,
        },
        news_process_configuration: NewsProcessConfiguration::try_new(
            NewsArrivalConfiguration {
                known: KnownNewsArrivalConfiguration {
                    scheduled_event_timestamps_utc_nanos: Vec::new(),
                },
                surprise: SurpriseNewsArrivalConfiguration {
                    open_baseline_intensity: 0.02,
                    closed_baseline_intensity: 0.02,
                },
            },
            SharedNewsMarkLawConfiguration {
                gamma_severity_marks: janiform_model_kernels::news::GammaSeverityMarkConfiguration {
                    shape_k: 1.4,
                },
            },
            NewsComponentMarkScaleConfiguration {
                activity_coupling_scale: 1.0,
                return_jump_drift: 0.0,
                return_jump_volatility: 1.0,
            },
            NewsComponentMarkScaleConfiguration {
                activity_coupling_scale: 1.0,
                return_jump_drift: 0.0,
                return_jump_volatility: 1.0,
            },
        )
        .expect("news process configuration should validate"),
        surprise_deterministic_intensity_profile: SurpriseNewsDeterministicIntensityProfile {
            weekly_hour_utc_multipliers: (0..168)
                .map(|hour_index| if hour_index == 14 { 2.0 } else { 1.0 })
                .collect(),
            profile_resolution_hours: 1,
            fit_policy:
                "selected_exogenous_surprise_episode_rate_per_utc_hour_of_week_with_baseline_shrinkage.v1"
                    .to_string(),
            normalization_policy: "exposure_weighted_mean_one_over_source_window.v1"
                .to_string(),
        },
        return_clock_exposure_parameters: ReturnClockExposureParameters::neutral(),
        long_variance_parameters: janiform_model_kernels::long_variance::LongVarianceParameters::neutral(),
        tick_grid_observation_model: TickGridObservationModel {
            tick_size: None,
            tick_size_source: "test".to_string(),
            inference_policy: "test".to_string(),
            observation_policy: "test".to_string(),
        },
        mean_reversion_fitted_parameters:
            build_endogenous_mean_reversion_parameters(),
    };

    build_calibration_stage_parameter_artifact(
        run_contract,
        CalibrationStageParameterArtifactInput::ReturnComponents(Box::new(
            ReturnComponentsStageParameterArtifactInput {
                metadata: CalibrationStageParameterBuildMetadata {
                    stage_output_schema_version:
                        CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION.to_string(),
                    run_global_seed: stage_seed_metadata.run_global_seed,
                    stage_seed: stage_seed_metadata.stage_seed,
                    stage_seed_source: stage_seed_metadata.stage_seed_source,
                    stage_seed_derivation_policy: CALIBRATION_STAGE_SEED_DERIVATION_POLICY
                        .to_string(),
                    solver_settings: solver_settings.clone(),
                },
                optimizer_state: CalibrationStageParameterOptimizerStateInput {
                    backend_id: "return_components.fixture_search.v1".to_string(),
                    random_seed: stage_seed_metadata.stage_seed,
                    selection_policy:
                        "validity_then_weighted_objective_then_lexicographic_tie_break.v1"
                            .to_string(),
                    penalty_policy: "hard_constraint_violation_additive_penalty.v1".to_string(),
                    bounds_policy: "candidate_family_cartesian_product.v1".to_string(),
                    solver_settings: solver_settings.clone(),
                    input_candidate_count: 12,
                    valid_candidate_count: 9,
                    selected_candidate_raw_objective: 1.25,
                    selected_candidate_penalized_objective: 1.25,
                },
                selected_candidate_id: "return_candidate_balanced".to_string(),
                fitted_parameters,
                candidate_generation_policy:
                    "return_components.candidate_generation.layered_family_search.v1".to_string(),
                objective_weighting_policy:
                    "return_components.objective_weighting.weighted_metric_sum.v1".to_string(),
                objective_metric_bucket_policy:
                    "return_components.objective_metrics.bucketed_fit_quality.v1".to_string(),
                activity_input_transport: CalibrationStageHandoffTransport::InMemoryOutput,
                activity_selected_hawkes_candidate_id: Some(
                    "candidate_stable_low_loss".to_string(),
                ),
                activity_artifact_reference: None,
            },
        )),
    )
    .expect("return-components stage parameter artifact should build")
}

pub(crate) fn build_stage_checkpoint_metadata_fixture(
    run_contract: &CalibrationRunContract,
    completed_stage: CalibrationStage,
    completed_stage_output_schema_version: &str,
) -> CalibrationStageCheckpointMetadata {
    let completed_stage_position = run_contract
        .execution_stage_order
        .iter()
        .position(|stage| *stage == completed_stage)
        .expect("completed stage should be present in the execution order");
    let stage_seed_metadata =
        resolve_calibration_stage_seed_metadata(&run_contract.run_spec, completed_stage);
    let solver_settings = stage_solver_settings_for_stage(&run_contract.run_spec, completed_stage)
        .expect("solver settings should exist for the completed stage");

    CalibrationStageCheckpointMetadata {
        schema_version: CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION.to_string(),
        checkpoint_reference_policy: CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY.to_string(),
        disk_write_policy: CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY.to_string(),
        artifact_manifest_schema_version: CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION.to_string(),
        checkpoint_reference: calibration_stage_checkpoint_reference(
            run_contract.run_spec.run_id.as_str(),
            completed_stage,
        ),
        run_id: run_contract.run_spec.run_id.clone(),
        execution_mode: run_contract.run_spec.execution_mode,
        run_contract_schema_version: run_contract.schema_version.clone(),
        run_spec_schema_version: run_contract.run_spec_schema_version.clone(),
        stage_graph_schema_version: run_contract.stage_graph_schema_version.clone(),
        execution_stage_order: run_contract.execution_stage_order.clone(),
        completed_stage,
        completed_stage_position,
        run_global_seed: run_contract.run_spec.seed_settings.global_seed,
        completed_stage_output_schema_version: completed_stage_output_schema_version.to_string(),
        completed_stage_seed: stage_seed_metadata.stage_seed,
        completed_stage_seed_source: stage_seed_metadata.stage_seed_source,
        stage_seed_derivation_policy: CALIBRATION_STAGE_SEED_DERIVATION_POLICY.to_string(),
        completed_stage_solver_settings: solver_settings,
    }
}

pub(crate) fn temporary_checkpoint_directory(test_name: &str) -> PathBuf {
    let unique_suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after UNIX epoch")
        .as_nanos();
    let checkpoint_directory =
        std::env::temp_dir().join(format!("janiform-checkpoints-{test_name}-{unique_suffix}"));

    if checkpoint_directory.exists() {
        let _ = fs::remove_dir_all(checkpoint_directory.as_path());
    }

    fs::create_dir_all(checkpoint_directory.as_path())
        .expect("temporary checkpoint directory should be created");

    checkpoint_directory
}
