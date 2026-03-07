#![allow(unused_imports)]

pub(super) use std::collections::BTreeSet;
pub(super) use std::fs;
pub(super) use std::path::PathBuf;
pub(super) use std::time::{SystemTime, UNIX_EPOCH};

pub(super) use chrono::{TimeZone, Utc};

pub(super) use crate::calendar::{
    ExchangeCalendarRegistry, ExchangeCalendarRegistryLoader, TimestampEventInputRecord,
};
pub(super) use crate::calibration::{
    CALIBRATION_ARTIFACT_ATOMIC_COMMIT_POLICY, CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_FAMILY,
    CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_GENERATION, CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION,
    CALIBRATION_ARTIFACT_PARTIAL_WRITE_RECOVERY_POLICY,
    CALIBRATION_ARTIFACT_ROOT_CONFIGURATION_POLICY, CALIBRATION_ARTIFACT_STORAGE_LAYOUT_POLICY,
    CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_RUN_SCHEMA_VERSION,
    CALIBRATION_DATASET_SNAPSHOT_ARTIFACT_SCHEMA_FAMILY,
    CALIBRATION_DATASET_SNAPSHOT_ARTIFACT_SCHEMA_GENERATION,
    CALIBRATION_DATASET_SNAPSHOT_ARTIFACT_SCHEMA_VERSION,
    CALIBRATION_DATASET_SNAPSHOT_CHECKSUM_ALGORITHM,
    CALIBRATION_DATASET_SNAPSHOT_OUTPUT_SCHEMA_METADATA_VERSION,
    CALIBRATION_DATASET_SNAPSHOT_PERSISTENCE_SCOPE_POLICY,
    CALIBRATION_DATASET_SNAPSHOT_STAGE_INPUT_NAMING_POLICY,
    CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY, CALIBRATION_EXECUTION_MODE_DEFAULT_POLICY,
    CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_OBJECTIVE_COMPOSITION_SCHEMA_VERSION,
    CALIBRATION_RETURN_COMPONENTS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION, CALIBRATION_RUN_DATA_INPUTS_SCHEMA_VERSION,
    CALIBRATION_RUN_SPEC_SCHEMA_VERSION, CALIBRATION_SEASONALITY_STAGE_DEFAULT_ESTIMATOR_VERSION,
    CALIBRATION_SEASONALITY_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
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
    CALIBRATION_VALIDATION_METRIC_ARTIFACT_SCHEMA_VERSION, CalibrationArtifactKind,
    CalibrationArtifactManifest, CalibrationArtifactManifestBuildError,
    CalibrationArtifactManifestError, CalibrationArtifactReference,
    CalibrationArtifactReloadComponent, CalibrationArtifactReloadError,
    CalibrationArtifactReloadRequest, CalibrationArtifactReloadRequestError,
    CalibrationArtifactRunPreparationMode, CalibrationArtifactRunWriter, CalibrationCallerSurface,
    CalibrationDatasetInputName, CalibrationDatasetSnapshotArtifact,
    CalibrationDatasetSnapshotArtifactValidationError, CalibrationExecutionMode,
    CalibrationMemoryCheckpointStore, CalibrationObjectiveTermInput,
    CalibrationOptimizerAdapterCandidateInput, CalibrationOptimizerAdapterConfiguration,
    CalibrationOptimizerAdapterError, CalibrationOptimizerCandidateParameterValue,
    CalibrationOptimizerParameterBound, CalibrationOptimizerSettings, CalibrationResumeSettings,
    CalibrationResumeSettingsError, CalibrationRunContract, CalibrationRunContractError,
    CalibrationRunDataInputs, CalibrationRunSeedSettings, CalibrationRunSpec,
    CalibrationRunSpecError, CalibrationRunSpecErrorClassification, CalibrationStage,
    CalibrationStageDependency, CalibrationStageExecutionStatus, CalibrationStageGraph,
    CalibrationStageGraphError, CalibrationStageHandoff, CalibrationStageHandoffError,
    CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError,
    CalibrationStageParameterArtifactValidationError, CalibrationStageParameterPayload,
    CalibrationStageSeedOverride, CalibrationStageSeedSource, CalibrationStageSolverSettings,
    CalibrationStagedRunFailure, CalibrationStagedRunInputs, CalibrationStagedRunStageReport,
    CalibrationValidationMetricArtifact,
    CalibrationValidationMetricArtifactReferenceIntegrityError,
    CalibrationValidationMetricArtifactRole, CalibrationValidationMetricArtifactValidationError,
    CalibrationValidationMetricCollection, CalibrationValidationMetricConfidenceInterval,
    CalibrationValidationMetricContext, CalibrationValidationMetricContextKind,
    CalibrationValidationMetricManifestLink, CalibrationValidationMetricRecord,
    CalibrationValidationMetricSourceKind, CalibrationValidationMetricStageOutputLink,
    CalibrationValidationMetricStatus, CalibrationValidationMetricThresholdMetadata,
    CalibrationValidationObservedValue, HawkesStageCandidateInput, HawkesStageExecutionFailure,
    HawkesStageExecutionInput, HawkesStageObjectiveTermInput,
    OptionSurfaceCalibrationObservationError, ReturnComponentsDetectedCloseOpenGapCandidate,
    ReturnComponentsDetectedIntradayJumpCandidate, ReturnComponentsKnownNewsCalendarInput,
    ReturnComponentsRealizedKnownNewsReturnSample, ReturnComponentsStageCandidateGenerationInput,
    ReturnComponentsStageExecutionFailure, ReturnComponentsStageExecutionInput,
    ReturnComponentsStageObjectiveMetricWeights, ReturnComponentsStageSignalSourceWindow,
    SeasonalityStageExecutionFailure, SeasonalityStageExecutionInput,
    build_calibration_artifact_manifest, build_calibration_dataset_snapshot_artifact,
    build_calibration_stage_parameter_artifact,
    build_option_surface_calibration_observation_from_pricing_hooks,
    calibration_artifact_manifest_path, calibration_artifact_manifest_reference,
    calibration_checkpoint_reference_for_stage, calibration_checkpoint_run_directory,
    calibration_committed_artifact_run_directory, calibration_dataset_snapshot_artifact_path,
    calibration_last_success_checkpoint_path, calibration_last_success_checkpoint_reference,
    calibration_stage_checkpoint_path, calibration_stage_parameter_artifact_path,
    calibration_validation_metric_artifact_path, calibration_validation_metric_artifact_reference,
    compose_calibration_weighted_objective, default_calibration_execution_mode_for_surface,
    derive_calibration_stage_seed_from_global_seed,
    evaluate_calibration_martingale_condition_under_esscher_tilt,
    load_calibration_artifact_manifest_from_disk,
    load_calibration_dataset_snapshot_artifact_from_disk,
    load_calibration_stage_parameter_artifact_from_disk,
    load_calibration_validation_metric_artifact_from_disk,
    persist_calibration_artifact_manifest_to_disk,
    persist_calibration_dataset_snapshot_artifact_to_disk,
    persist_calibration_stage_parameter_artifact_to_disk,
    persist_calibration_validation_metric_artifact_to_disk,
    precheck_hawkes_kernel_parameter_domain, precheck_structured_hawkes_kernel_parameter_domain,
    reload_calibration_artifact_run_from_disk, resolve_calibration_execution_mode_for_surface,
    resolve_calibration_stage_execution_order, resolve_calibration_stage_seed_metadata,
    run_constrained_optimizer_adapter, run_hawkes_calibration_stage,
    run_return_components_calibration_stage, run_seasonality_calibration_stage,
    run_staged_calibration_with_checkpointing,
    transform_nig_exponent_for_calibration_under_esscher_tilt,
};
pub(super) use crate::complex_math::ComplexScalar;
pub(super) use crate::hawkes::{
    HAWKES_KERNEL_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION, HawkesKernelLoadings,
    HawkesKernelValidationStatus, HawkesStructuredAlphaParameterization,
    HawkesStructuredRhoParameterization, HawkesStructuredRhoSelfDominancePolicy,
};
pub(super) use crate::news::{
    CoupledGapMarkConfiguration, DoubleExponentialBaseShockConfiguration,
    GammaSeverityMarkConfiguration, SharedNewsMarkLawConfiguration,
};
pub(super) use crate::nig::{NigParameters, NigSharedPlusShiftConfiguration};
pub(super) use crate::pricing::VanillaOptionPricingCalibrationHooks;
pub(super) use crate::risk_neutral::{
    EsscherMartingaleConditionInput, NigEsscherTransformationInput,
};
pub(super) use serde_json::json;

pub(super) fn build_data_inputs() -> CalibrationRunDataInputs {
    CalibrationRunDataInputs::try_new(
        "XNYS",
        "dataset://synthetic/trades/v1",
        Some("dataset://synthetic/options/v1".to_string()),
    )
    .expect("data inputs should validate")
}

pub(super) fn build_seed_settings() -> CalibrationRunSeedSettings {
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

pub(super) fn build_solver_settings() -> CalibrationOptimizerSettings {
    CalibrationOptimizerSettings::try_new("lbfgs", 250, 1e-8, 1e-6, 10.0)
        .expect("solver settings should validate")
}

pub(super) fn build_solver_settings_for_seasonality_and_hawkes()
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

pub(super) fn build_solver_settings_for_all_stages() -> Vec<CalibrationStageSolverSettings> {
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

pub(super) fn build_valid_run_spec() -> CalibrationRunSpec {
    build_valid_run_spec_with_mode(CalibrationExecutionMode::Memory)
}

pub(super) fn build_valid_run_spec_with_mode(
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
        ],
        build_seed_settings(),
        build_solver_settings_for_all_stages(),
    )
    .expect("run spec should validate")
}

pub(super) fn build_valid_artifact_manifest() -> CalibrationArtifactManifest {
    CalibrationArtifactManifest::try_new(
        "run-001",
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

pub(super) fn build_validation_metric_artifact_fixture() -> CalibrationValidationMetricArtifact {
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

pub(super) fn build_validation_metric_manifest_fixture() -> CalibrationArtifactManifest {
    CalibrationArtifactManifest::try_new(
        "run-001",
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

pub(super) fn build_run_contract_with_mode_and_resume(
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

pub(super) fn build_run_contract_for_caller_surface(
    run_id: &str,
    caller_surface: CalibrationCallerSurface,
    execution_mode_override: Option<CalibrationExecutionMode>,
) -> CalibrationRunContract {
    let execution_mode =
        resolve_calibration_execution_mode_for_surface(caller_surface, execution_mode_override);
    let run_spec = CalibrationRunSpec::try_new(
        run_id,
        build_data_inputs(),
        execution_mode,
        vec![
            CalibrationStage::Seasonality,
            CalibrationStage::Hawkes,
            CalibrationStage::ReturnComponents,
        ],
        build_seed_settings(),
        build_solver_settings_for_all_stages(),
    )
    .expect("caller-surface run spec should validate");

    CalibrationRunContract::try_new(run_spec, default_stage_graph())
        .expect("caller-surface run contract should validate")
}

pub(super) fn default_stage_graph() -> CalibrationStageGraph {
    CalibrationStageGraph::new(vec![
        CalibrationStageDependency {
            stage: CalibrationStage::Hawkes,
            depends_on: CalibrationStage::Seasonality,
        },
        CalibrationStageDependency {
            stage: CalibrationStage::ReturnComponents,
            depends_on: CalibrationStage::Hawkes,
        },
    ])
}

pub(super) fn build_mode_aware_stage_handoff(
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

pub(super) fn staged_run_inputs(
    include_seasonality_input: bool,
    include_hawkes_candidates: bool,
    include_return_candidate_generation_input: bool,
) -> CalibrationStagedRunInputs {
    CalibrationStagedRunInputs {
        seasonality_stage_input: if include_seasonality_input {
            Some(
                SeasonalityStageExecutionInput::from_timestamp_event_records(
                    calibration_stage_timestamp_fixture(),
                ),
            )
        } else {
            None
        },
        hawkes_candidate_inputs: if include_hawkes_candidates {
            Some(hawkes_stage_candidate_fixture())
        } else {
            None
        },
        return_components_candidate_generation_input: if include_return_candidate_generation_input {
            Some(return_components_stage_candidate_generation_fixture())
        } else {
            None
        },
    }
}

pub(super) fn temporary_checkpoint_directory(test_name: &str) -> PathBuf {
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

pub(super) fn build_seasonality_stage_run_spec(
    execution_mode: CalibrationExecutionMode,
) -> CalibrationRunSpec {
    let stage_handoff = build_mode_aware_stage_handoff(
        CalibrationStage::Seasonality,
        CalibrationStage::Hawkes,
        execution_mode,
        "artifact://seasonality/run-001",
    );

    CalibrationRunSpec::builder(
        "run-001",
        build_data_inputs(),
        execution_mode,
        vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
        build_seed_settings(),
        build_solver_settings_for_seasonality_and_hawkes(),
    )
    .with_stage_handoffs(vec![stage_handoff])
    .build()
    .expect("seasonality stage run spec should validate")
}

pub(super) fn build_hawkes_stage_run_spec(
    execution_mode: CalibrationExecutionMode,
) -> CalibrationRunSpec {
    let seasonality_handoff = build_mode_aware_stage_handoff(
        CalibrationStage::Seasonality,
        CalibrationStage::Hawkes,
        execution_mode,
        "artifact://seasonality/run-001",
    );

    let hawkes_handoff = build_mode_aware_stage_handoff(
        CalibrationStage::Hawkes,
        CalibrationStage::ReturnComponents,
        execution_mode,
        "artifact://hawkes/run-001",
    );

    CalibrationRunSpec::builder(
        "run-001",
        build_data_inputs(),
        execution_mode,
        vec![
            CalibrationStage::Seasonality,
            CalibrationStage::Hawkes,
            CalibrationStage::ReturnComponents,
        ],
        build_seed_settings(),
        build_solver_settings_for_all_stages(),
    )
    .with_stage_handoffs(vec![seasonality_handoff, hawkes_handoff])
    .build()
    .expect("hawkes stage run spec should validate")
}

pub(super) fn build_return_components_stage_pipeline_run_spec(
    execution_mode: CalibrationExecutionMode,
) -> CalibrationRunSpec {
    build_hawkes_stage_run_spec(execution_mode)
}

pub(super) fn build_return_components_stage_standalone_run_spec(
    execution_mode: CalibrationExecutionMode,
) -> CalibrationRunSpec {
    let seed_settings = CalibrationRunSeedSettings::try_new(
        23,
        vec![CalibrationStageSeedOverride {
            stage: CalibrationStage::ReturnComponents,
            seed: 303,
        }],
    )
    .expect("standalone return-components seed settings should validate");

    CalibrationRunSpec::try_new(
        "run-return-components-standalone",
        build_data_inputs(),
        execution_mode,
        vec![CalibrationStage::ReturnComponents],
        seed_settings,
        vec![CalibrationStageSolverSettings {
            stage: CalibrationStage::ReturnComponents,
            optimizer_settings: build_solver_settings(),
        }],
    )
    .expect("standalone return-components run spec should validate")
}

pub(super) fn build_seasonality_stage_success_payload()
-> crate::calibration::SeasonalityStageNormalizedPayload {
    let calendar_registry = calibration_stage_test_registry();
    let report = run_seasonality_calibration_stage(
        &build_seasonality_stage_run_spec(CalibrationExecutionMode::Memory),
        &calendar_registry,
        SeasonalityStageExecutionInput::from_timestamp_event_records(
            calibration_stage_timestamp_fixture(),
        ),
    );

    report
        .normalized_stage_payload
        .expect("seasonality stage fixture should produce normalized payload")
}

pub(super) fn build_hawkes_stage_success_payload() -> crate::calibration::HawkesStageFittedPayload {
    let seasonality_payload = build_seasonality_stage_success_payload();
    let report = run_hawkes_calibration_stage(
        &build_hawkes_stage_run_spec(CalibrationExecutionMode::Memory),
        HawkesStageExecutionInput::from_in_memory_seasonality_output(
            seasonality_payload,
            hawkes_stage_candidate_fixture(),
        ),
    );

    report
        .fitted_stage_payload
        .expect("hawkes stage fixture should produce fitted payload")
}

pub(super) fn hawkes_stage_candidate_fixture() -> Vec<HawkesStageCandidateInput> {
    vec![
        HawkesStageCandidateInput {
            candidate_id: "candidate_high_loss".to_string(),
            lambda_bar: 0.85,
            beta: 1.50,
            structured_alpha: HawkesStructuredAlphaParameterization::new(0.40, 0.10, 0.08, 0.10),
            structured_rho: HawkesStructuredRhoParameterization::new(0.35, 0.10, 0.05, 0.10),
            self_dominance_policy: Some(HawkesStructuredRhoSelfDominancePolicy::new(1.0)),
            stability_numeric_tolerance: 0.0,
            objective_terms: vec![
                HawkesStageObjectiveTermInput {
                    term_id: "event_rate_residual".to_string(),
                    weight: 1.0,
                    observed_score: 2.8,
                },
                HawkesStageObjectiveTermInput {
                    term_id: "clustering_autocorrelation".to_string(),
                    weight: 1.0,
                    observed_score: 1.4,
                },
            ],
        },
        HawkesStageCandidateInput {
            candidate_id: "candidate_stable_low_loss".to_string(),
            lambda_bar: 0.80,
            beta: 1.60,
            structured_alpha: HawkesStructuredAlphaParameterization::new(0.35, 0.06, 0.07, 0.04),
            structured_rho: HawkesStructuredRhoParameterization::new(0.30, 0.08, 0.04, 0.05),
            self_dominance_policy: Some(HawkesStructuredRhoSelfDominancePolicy::new(1.0)),
            stability_numeric_tolerance: 0.0,
            objective_terms: vec![
                HawkesStageObjectiveTermInput {
                    term_id: "event_rate_residual".to_string(),
                    weight: 1.0,
                    observed_score: 1.6,
                },
                HawkesStageObjectiveTermInput {
                    term_id: "clustering_autocorrelation".to_string(),
                    weight: 1.0,
                    observed_score: 0.7,
                },
            ],
        },
    ]
}

pub(super) fn hawkes_stage_constraint_violating_candidate_fixture() -> HawkesStageCandidateInput {
    HawkesStageCandidateInput {
        candidate_id: "candidate_invalid_constraints".to_string(),
        lambda_bar: -0.2,
        beta: 0.0,
        structured_alpha: HawkesStructuredAlphaParameterization::new(-0.1, -0.1, -0.1, -0.1),
        structured_rho: HawkesStructuredRhoParameterization::new(-0.2, -0.1, -0.1, -0.1),
        self_dominance_policy: Some(HawkesStructuredRhoSelfDominancePolicy::new(0.9)),
        stability_numeric_tolerance: -0.5,
        objective_terms: vec![HawkesStageObjectiveTermInput {
            term_id: "event_rate_residual".to_string(),
            weight: 1.0,
            observed_score: 1.0,
        }],
    }
}

pub(super) fn return_components_stage_candidate_generation_fixture()
-> ReturnComponentsStageCandidateGenerationInput {
    let source_window = ReturnComponentsStageSignalSourceWindow {
        window_start_utc_nanos: utc_nanos(2026, 2, 2, 13, 0, 0),
        window_end_utc_nanos: utc_nanos(2026, 2, 5, 0, 0, 0),
    };

    ReturnComponentsStageCandidateGenerationInput {
        known_news_calendar: ReturnComponentsKnownNewsCalendarInput {
            source_window,
            scheduled_event_timestamps_utc_nanos: vec![
                utc_nanos(2026, 2, 2, 14, 30, 0),
                utc_nanos(2026, 2, 3, 15, 0, 0),
                utc_nanos(2026, 2, 4, 16, 0, 0),
            ],
        },
        realized_known_news_return_samples: vec![
            ReturnComponentsRealizedKnownNewsReturnSample {
                event_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 30, 0),
                open_window_jump_return: 0.016,
                closed_window_gap_return: -0.009,
            },
            ReturnComponentsRealizedKnownNewsReturnSample {
                event_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 15, 0, 0),
                open_window_jump_return: -0.012,
                closed_window_gap_return: 0.004,
            },
            ReturnComponentsRealizedKnownNewsReturnSample {
                event_timestamp_utc_nanos: utc_nanos(2026, 2, 4, 16, 0, 0),
                open_window_jump_return: 0.011,
                closed_window_gap_return: -0.006,
            },
        ],
        detected_intraday_jump_candidates: vec![
            ReturnComponentsDetectedIntradayJumpCandidate {
                jump_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 16, 5, 0),
                jump_return: 0.018,
                post_shock_activity_lift: 1.35,
            },
            ReturnComponentsDetectedIntradayJumpCandidate {
                jump_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 18, 20, 0),
                jump_return: -0.014,
                post_shock_activity_lift: 1.10,
            },
            ReturnComponentsDetectedIntradayJumpCandidate {
                jump_timestamp_utc_nanos: utc_nanos(2026, 2, 4, 19, 15, 0),
                jump_return: 0.010,
                post_shock_activity_lift: 0.95,
            },
        ],
        detected_close_open_gap_candidates: vec![
            ReturnComponentsDetectedCloseOpenGapCandidate {
                close_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 21, 0, 0),
                open_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 14, 30, 0),
                gap_return: -0.013,
                post_shock_activity_lift: 1.40,
            },
            ReturnComponentsDetectedCloseOpenGapCandidate {
                close_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 21, 0, 0),
                open_timestamp_utc_nanos: utc_nanos(2026, 2, 4, 14, 30, 0),
                gap_return: 0.008,
                post_shock_activity_lift: 1.05,
            },
        ],
        nig_shared_plus_shift_candidates: vec![
            NigSharedPlusShiftConfiguration::try_new(2.30, -0.80, 0.70, 0.10, -0.20)
                .expect("high-loss NIG configuration should validate"),
            NigSharedPlusShiftConfiguration::try_new(2.45, -0.70, 0.62, 0.08, -0.15)
                .expect("low-loss NIG configuration should validate"),
        ],
        shared_mark_law_template: SharedNewsMarkLawConfiguration {
            gamma_severity_marks: GammaSeverityMarkConfiguration::try_new(1.8, 0.7)
                .expect("gamma severity mark configuration should validate"),
            double_exponential_base_shock: DoubleExponentialBaseShockConfiguration::try_new(
                0.45, 8.0, 10.0,
            )
            .expect("double-exponential base-shock configuration should validate"),
            coupled_gap_marks: CoupledGapMarkConfiguration::new(0.22),
        },
        objective_metric_weights: ReturnComponentsStageObjectiveMetricWeights::new(1.0, 1.0, 1.0),
    }
}

pub(super) fn utc_nanos(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> i64 {
    Utc.with_ymd_and_hms(year, month, day, hour, minute, second)
        .single()
        .expect("fixture timestamp should be valid")
        .timestamp_nanos_opt()
        .expect("fixture timestamp nanos should fit in i64")
}

pub(super) fn calibration_stage_test_registry() -> ExchangeCalendarRegistry {
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
      "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]
    },
    {
      "weekday": "Tuesday",
      "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]
    },
    {
      "weekday": "Wednesday",
      "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]
    }
  ],
  "holiday_dates": [],
  "early_close_sessions": []
}
  ]
}
"#,
    )
    .expect("calendar registry fixture should load")
}

pub(super) fn calibration_stage_timestamp_fixture() -> Vec<TimestampEventInputRecord> {
    vec![
        timestamp_input_record(0, 2026, 2, 2, 14, 30, 0),
        timestamp_input_record(1, 2026, 2, 2, 15, 30, 0),
        timestamp_input_record(2, 2026, 2, 3, 14, 30, 0),
        timestamp_input_record(3, 2026, 2, 3, 18, 45, 0),
        timestamp_input_record(4, 2026, 2, 4, 14, 30, 0),
        timestamp_input_record(5, 2026, 2, 4, 20, 30, 0),
    ]
}

pub(super) fn timestamp_input_record(
    source_record_position: u64,
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> TimestampEventInputRecord {
    TimestampEventInputRecord {
        source_record_position,
        timestamp_utc: Utc
            .with_ymd_and_hms(year, month, day, hour, minute, second)
            .single()
            .expect("test timestamp should be valid"),
    }
}
