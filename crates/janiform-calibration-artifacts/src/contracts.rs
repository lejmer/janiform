mod artifact_manifest;
mod constants;
mod data_inputs;
mod handoff_resume;
mod optimizer;
mod run_contract;
mod run_spec;
mod seed_settings;
mod stage_graph;
mod stage_seed;
mod stage_types;

pub use artifact_manifest::{
    CalibrationArtifactKind, CalibrationArtifactManifest, CalibrationArtifactManifestError,
    CalibrationArtifactManifestIndexMetadata, CalibrationArtifactManifestMetricStatus,
    CalibrationArtifactManifestMetricStatusCount, CalibrationArtifactManifestModelVersion,
    CalibrationArtifactManifestValidationMetricRole,
    CalibrationArtifactManifestValidationMetricSummary, CalibrationArtifactReference,
};
pub use constants::{
    CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_FAMILY, CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_GENERATION,
    CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION, CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY,
    CALIBRATION_EXECUTION_MODE_DEFAULT_POLICY,
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
    CALIBRATION_STAGE_SEED_DERIVATION_POLICY, CALIBRATION_STAGED_RUN_FAILURE_TRIAGE_POLICY,
    CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION,
};
pub use data_inputs::{CalibrationRunDataInputs, CalibrationRunDataInputsError};
pub use handoff_resume::{
    CalibrationResumeSettings, CalibrationResumeSettingsError, CalibrationResumeSource,
    CalibrationStageHandoff, CalibrationStageHandoffError, CalibrationStageHandoffRoute,
    CalibrationStageHandoffTransport,
};
pub use optimizer::{
    CalibrationOptimizerSettings, CalibrationOptimizerSettingsError, CalibrationStageSolverSettings,
};
pub use run_contract::{
    CalibrationRunContract, CalibrationRunContractError, resolve_calibration_stage_execution_order,
};
pub use run_spec::{
    CalibrationRunSpec, CalibrationRunSpecBuilder, CalibrationRunSpecError,
    CalibrationRunSpecErrorClassification,
};
pub use seed_settings::{
    CalibrationResolvedStageSeed, CalibrationRunSeedSettings, CalibrationRunSeedSettingsError,
    CalibrationStageSeedOverride, CalibrationStageSeedSource,
};
pub use stage_graph::{
    CalibrationStageDependency, CalibrationStageGraph, CalibrationStageGraphError,
};
pub use stage_seed::{
    derive_calibration_stage_seed_from_global_seed, resolve_calibration_stage_seed,
    resolve_calibration_stage_seed_metadata, stage_solver_settings_for_stage,
};
pub use stage_types::{
    CalibrationCallerSurface, CalibrationExecutionMode, CalibrationStage,
    default_calibration_execution_mode_for_surface, resolve_calibration_execution_mode_for_surface,
};
