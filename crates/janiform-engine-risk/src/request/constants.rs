pub use janiform_affine_oracle::sufficient_statistics::{
    DEFAULT_INTRADAY_RISK_INTERVAL_DELTA, SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY,
};

pub const SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION: &str =
    "risk.sufficient_statistics.run_request.v1";
pub const SUFFICIENT_STATISTICS_RISK_PATH_SCHEMA_VERSION: &str =
    "risk.sufficient_statistics.path.v1";
pub const SUFFICIENT_STATISTICS_RISK_PATH_RECORD_SCHEMA_VERSION: &str =
    "risk.sufficient_statistics.path_record.v1";
pub const SUFFICIENT_STATISTICS_RISK_PATH_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "risk.sufficient_statistics.path_output_schema_metadata.v1";
pub const SUFFICIENT_STATISTICS_RISK_PATH_PROVENANCE_SCHEMA_VERSION: &str =
    "risk.sufficient_statistics.path_provenance.v1";
pub const SUFFICIENT_STATISTICS_RISK_PATH_DIAGNOSTICS_SCHEMA_VERSION: &str =
    "risk.sufficient_statistics.path_diagnostics.v1";
pub const SUFFICIENT_STATISTICS_RISK_PATH_ARTIFACT_WORKFLOW_POLICY: &str =
    "compatible_with_calibration_and_validation_artifact_workflows.v1";
pub const SUFFICIENT_STATISTICS_RISK_PATH_ENVIRONMENT_METADATA_POLICY: &str =
    "exclude_environment_specific_metadata.v1";
pub(crate) const SUFFICIENT_STATISTICS_RISK_PATH_REQUIRED_FIELDS: [&str; 11] = [
    "schema_version",
    "run_request_schema_version",
    "output_schema_metadata",
    "provenance",
    "diagnostics",
    "seed",
    "interval_delta",
    "simulated_interval_count",
    "initial_excitation_state",
    "terminal_excitation_state",
    "records",
];
pub(crate) const SUFFICIENT_STATISTICS_RISK_PATH_REQUIRED_PROVENANCE_FIELDS: [&str; 10] = [
    "schema_version",
    "artifact_workflow_policy",
    "environment_metadata_policy",
    "interval_factor_policy",
    "transform_slice_sampling_policy",
    "boundary_state_update_policy",
    "interval_return_generation_policy",
    "interval_return_component_routing_policy",
    "batch_seed_partition_policy",
    "artifact_workflow_tags",
];
pub(crate) const SUFFICIENT_STATISTICS_RISK_PATH_REQUIRED_DIAGNOSTICS_FIELDS: [&str; 17] = [
    "schema_version",
    "simulated_interval_count",
    "first_interval_index",
    "last_interval_index",
    "cumulative_a_plus",
    "cumulative_a_minus",
    "cumulative_n_plus",
    "cumulative_n_minus",
    "cumulative_trading_time_return",
    "cumulative_open_jump_return",
    "cumulative_closed_gap_return",
    "cumulative_total_return",
    "cumulative_observed_return",
    "max_abs_total_return",
    "max_abs_deviation_from_anchor",
    "terminal_observed_log_price",
    "terminal_deviation_from_anchor",
];
pub(crate) const SUFFICIENT_STATISTICS_RISK_PATH_ARTIFACT_WORKFLOW_TAGS: [&str; 2] =
    ["calibration", "validation"];
