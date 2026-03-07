pub const CALIBRATION_RUN_SPEC_SCHEMA_VERSION: &str = "calibration.orchestration.run_spec.v1";
pub const CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION: &str = "calibration.orchestration.stage_graph.v1";
pub const CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION: &str =
    "calibration.orchestration.run_contract.v1";
pub const CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION: &str =
    "calibration.orchestration.staged_run.report.v1";
pub const CALIBRATION_STAGED_RUN_FAILURE_TRIAGE_POLICY: &str =
    "mode_aware_failure_kind_and_checkpoint_context.v1";
pub const CALIBRATION_SOLVER_SETTINGS_CAPTURE_POLICY: &str =
    "capture_stage_solver_settings_per_run_contract.v1";
pub const CALIBRATION_STAGE_SEED_DERIVATION_POLICY: &str =
    "global_seed_splitmix64_per_stage_with_override_precedence.v1";

/// Caller-surface default policy for execution-mode resolution.
pub const CALIBRATION_EXECUTION_MODE_DEFAULT_POLICY: &str =
    "cli_defaults_to_disk_python_importable_api_defaults_to_memory.v1";
pub const CALIBRATION_SEASONALITY_STAGE_EXECUTION_REPORT_SCHEMA_VERSION: &str =
    "calibration.orchestration.stage_execution.seasonality.report.v1";
pub const CALIBRATION_SEASONALITY_STAGE_OUTPUT_SCHEMA_VERSION: &str =
    "calibration.orchestration.stage_output.seasonality.v1";
pub const CALIBRATION_SEASONALITY_STAGE_HANDOFF_POLICY: &str =
    "explicit_run_spec_handoffs_mode_aware_transport.v1";
pub const CALIBRATION_SEASONALITY_STAGE_DEFAULT_ESTIMATOR_VERSION: &str =
    "seasonality_estimator.v1";
pub const CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION: &str =
    "calibration.orchestration.stage_execution.hawkes.report.v1";
pub const CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION: &str =
    "calibration.orchestration.stage_output.hawkes.v1";
pub const CALIBRATION_HAWKES_STAGE_HANDOFF_POLICY: &str =
    "explicit_run_spec_handoffs_mode_aware_transport.v1";
pub const CALIBRATION_RETURN_COMPONENTS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION: &str =
    "calibration.orchestration.stage_execution.return_components.report.v1";
pub const CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION: &str =
    "calibration.orchestration.stage_output.return_components.v1";
pub const CALIBRATION_RETURN_COMPONENTS_STAGE_HANDOFF_POLICY: &str =
    "explicit_run_spec_handoffs_mode_aware_transport.v1";
pub const CALIBRATION_RETURN_COMPONENTS_STAGE_OBJECTIVE_WEIGHTING_POLICY: &str =
    "weighted_sum_of_configured_objective_terms.v1";
pub const CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION: &str =
    "calibration.orchestration.stage_checkpoint.v1";
pub const CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY: &str =
    "checkpoint_uri_with_run_id_and_stage_token.v1";
pub const CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY: &str = "atomic_tempfile_then_rename.v1";
pub const CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_FAMILY: &str =
    "calibration.artifact_registry.manifest";
pub const CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_GENERATION: &str = "v1";
pub const CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION: &str =
    "calibration.artifact_registry.manifest.v1";
