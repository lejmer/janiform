//! Coarse simulation contract constants.

pub const COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION: &str =
    "simulation.coarse_interval.run_request.v1";
pub const COARSE_SIMULATION_PATH_SCHEMA_VERSION: &str = "simulation.coarse_interval.path.v1";
pub const COARSE_SIMULATION_PATH_RECORD_SCHEMA_VERSION: &str =
    "simulation.coarse_interval.path_record.v1";
pub const COARSE_SIMULATION_PATH_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "simulation.coarse_interval.path_output_schema_metadata.v1";
pub const COARSE_SIMULATION_PATH_PROVENANCE_SCHEMA_VERSION: &str =
    "simulation.coarse_interval.path_provenance.v1";
pub const COARSE_SIMULATION_PATH_DIAGNOSTICS_SCHEMA_VERSION: &str =
    "simulation.coarse_interval.path_diagnostics.v1";
pub const COARSE_SIMULATION_ARTIFACT_WORKFLOW_POLICY: &str =
    "time_indexed_coarse_interval_simulation_path_for_validation_and_analytics.v1";
pub const COARSE_SIMULATION_ENVIRONMENT_METADATA_POLICY: &str =
    "exclude_environment_specific_metadata.v1";
pub const COARSE_SIMULATION_INTERVAL_TIMING_POLICY: &str =
    "start_time_utc_anchor_plus_fixed_interval_delta_half_open_utc_boundaries.v1";
pub const COARSE_SIMULATION_INTERVAL_IDENTIFIER_POLICY: &str =
    "interval_index_and_explicit_utc_boundary_timestamps.v1";
pub const COARSE_SIMULATION_INTERVAL_FACTOR_ALIGNMENT_POLICY: &str = "factor_row_zero_anchors_to_start_time_utc_and_rows_progress_in_request_interval_order_without_implicit_resampling.v1";
pub const COARSE_SIMULATION_COMPATIBLE_INTERVAL_GRID_POLICY: &str =
    "interval_delta_must_map_to_an_exact_fixed_width_utc_nanosecond_grid.v1";

pub(super) const COARSE_SIMULATION_PATH_REQUIRED_FIELDS: [&str; 13] = [
    "schema_version",
    "run_request_schema_version",
    "output_schema_metadata",
    "provenance",
    "diagnostics",
    "seed",
    "start_time_utc_nanos",
    "end_time_utc_nanos",
    "interval_delta",
    "simulated_interval_count",
    "initial_excitation_state",
    "terminal_excitation_state",
    "records",
];
pub(super) const COARSE_SIMULATION_PATH_RECORD_REQUIRED_FIELDS: [&str; 11] = [
    "schema_version",
    "interval_index",
    "interval_start_utc_nanos",
    "interval_end_utc_nanos",
    "sufficient_statistics",
    "return_components",
    "core_total_return",
    "observed_interval_return",
    "observed_log_price_end",
    "anchor_value_end",
    "deviation_from_anchor_end",
];
pub(super) const COARSE_SIMULATION_PATH_REQUIRED_PROVENANCE_FIELDS: [&str; 13] = [
    "schema_version",
    "artifact_workflow_policy",
    "environment_metadata_policy",
    "interval_timing_policy",
    "interval_identifier_policy",
    "interval_factor_alignment_policy",
    "compatible_interval_grid_policy",
    "interval_factor_policy",
    "transform_slice_sampling_policy",
    "boundary_state_update_policy",
    "interval_return_generation_policy",
    "interval_return_component_routing_policy",
    "batch_seed_partition_policy",
];
pub(super) const COARSE_SIMULATION_PATH_REQUIRED_DIAGNOSTICS_FIELDS: [&str; 19] = [
    "schema_version",
    "simulated_interval_count",
    "first_interval_index",
    "last_interval_index",
    "first_interval_start_utc_nanos",
    "last_interval_end_utc_nanos",
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
