use janiform_affine_oracle::sufficient_statistics::{
    SUFFICIENT_STATISTICS_BATCH_SEED_PARTITION_POLICY,
    SUFFICIENT_STATISTICS_BOUNDARY_STATE_UPDATE_POLICY,
    SUFFICIENT_STATISTICS_INTERVAL_FACTOR_POLICY,
    SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_ROUTING_POLICY,
    SUFFICIENT_STATISTICS_INTERVAL_RETURN_GENERATION_POLICY,
    SUFFICIENT_STATISTICS_TRANSFORM_SLICE_SAMPLING_POLICY,
};

use super::constants::{
    COARSE_SIMULATION_ARTIFACT_WORKFLOW_POLICY, COARSE_SIMULATION_COMPATIBLE_INTERVAL_GRID_POLICY,
    COARSE_SIMULATION_ENVIRONMENT_METADATA_POLICY,
    COARSE_SIMULATION_INTERVAL_FACTOR_ALIGNMENT_POLICY,
    COARSE_SIMULATION_INTERVAL_IDENTIFIER_POLICY, COARSE_SIMULATION_INTERVAL_TIMING_POLICY,
    COARSE_SIMULATION_PATH_DIAGNOSTICS_SCHEMA_VERSION,
    COARSE_SIMULATION_PATH_OUTPUT_SCHEMA_METADATA_VERSION,
    COARSE_SIMULATION_PATH_PROVENANCE_SCHEMA_VERSION,
    COARSE_SIMULATION_PATH_RECORD_REQUIRED_FIELDS,
    COARSE_SIMULATION_PATH_REQUIRED_DIAGNOSTICS_FIELDS, COARSE_SIMULATION_PATH_REQUIRED_FIELDS,
    COARSE_SIMULATION_PATH_REQUIRED_PROVENANCE_FIELDS,
};
use super::path::{
    CoarseSimulationPathDiagnostics, CoarseSimulationPathError,
    CoarseSimulationPathOutputSchemaMetadata, CoarseSimulationPathProvenance,
};
use super::record::CoarseSimulationPathRecord;
use super::time_index::{
    build_string_list, compute_horizon_end_time_utc_nanos, interval_delta_to_nanos,
};

pub(super) fn build_coarse_simulation_path_output_schema_metadata()
-> CoarseSimulationPathOutputSchemaMetadata {
    CoarseSimulationPathOutputSchemaMetadata {
        schema_version: COARSE_SIMULATION_PATH_OUTPUT_SCHEMA_METADATA_VERSION.to_string(),
        provenance_schema_version: COARSE_SIMULATION_PATH_PROVENANCE_SCHEMA_VERSION.to_string(),
        diagnostics_schema_version: COARSE_SIMULATION_PATH_DIAGNOSTICS_SCHEMA_VERSION.to_string(),
        artifact_workflow_policy: COARSE_SIMULATION_ARTIFACT_WORKFLOW_POLICY.to_string(),
        environment_metadata_policy: COARSE_SIMULATION_ENVIRONMENT_METADATA_POLICY.to_string(),
        required_path_fields: build_string_list(&COARSE_SIMULATION_PATH_REQUIRED_FIELDS),
        required_record_fields: build_string_list(&COARSE_SIMULATION_PATH_RECORD_REQUIRED_FIELDS),
        required_provenance_fields: build_string_list(
            &COARSE_SIMULATION_PATH_REQUIRED_PROVENANCE_FIELDS,
        ),
        required_diagnostics_fields: build_string_list(
            &COARSE_SIMULATION_PATH_REQUIRED_DIAGNOSTICS_FIELDS,
        ),
    }
}

pub(super) fn build_coarse_simulation_path_provenance() -> CoarseSimulationPathProvenance {
    CoarseSimulationPathProvenance {
        schema_version: COARSE_SIMULATION_PATH_PROVENANCE_SCHEMA_VERSION.to_string(),
        artifact_workflow_policy: COARSE_SIMULATION_ARTIFACT_WORKFLOW_POLICY.to_string(),
        environment_metadata_policy: COARSE_SIMULATION_ENVIRONMENT_METADATA_POLICY.to_string(),
        interval_timing_policy: COARSE_SIMULATION_INTERVAL_TIMING_POLICY.to_string(),
        interval_identifier_policy: COARSE_SIMULATION_INTERVAL_IDENTIFIER_POLICY.to_string(),
        interval_factor_alignment_policy: COARSE_SIMULATION_INTERVAL_FACTOR_ALIGNMENT_POLICY
            .to_string(),
        compatible_interval_grid_policy: COARSE_SIMULATION_COMPATIBLE_INTERVAL_GRID_POLICY
            .to_string(),
        interval_factor_policy: SUFFICIENT_STATISTICS_INTERVAL_FACTOR_POLICY.to_string(),
        transform_slice_sampling_policy: SUFFICIENT_STATISTICS_TRANSFORM_SLICE_SAMPLING_POLICY
            .to_string(),
        boundary_state_update_policy: SUFFICIENT_STATISTICS_BOUNDARY_STATE_UPDATE_POLICY
            .to_string(),
        interval_return_generation_policy: SUFFICIENT_STATISTICS_INTERVAL_RETURN_GENERATION_POLICY
            .to_string(),
        interval_return_component_routing_policy:
            SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_ROUTING_POLICY.to_string(),
        batch_seed_partition_policy: SUFFICIENT_STATISTICS_BATCH_SEED_PARTITION_POLICY.to_string(),
    }
}

pub(super) fn derive_coarse_simulation_path_diagnostics(
    simulated_interval_count: usize,
    records: &[CoarseSimulationPathRecord],
) -> Result<CoarseSimulationPathDiagnostics, CoarseSimulationPathError> {
    let first_record =
        records
            .first()
            .ok_or(CoarseSimulationPathError::InvalidSimulatedIntervalCount {
                value: simulated_interval_count,
            })?;

    let last_record =
        records
            .last()
            .ok_or(CoarseSimulationPathError::InvalidSimulatedIntervalCount {
                value: simulated_interval_count,
            })?;

    let mut cumulative_a_plus = 0.0;
    let mut cumulative_a_minus = 0.0;
    let mut cumulative_n_plus = 0_u64;
    let mut cumulative_n_minus = 0_u64;
    let mut cumulative_trading_time_return = 0.0;
    let mut cumulative_open_jump_return = 0.0;
    let mut cumulative_closed_gap_return = 0.0;
    let mut cumulative_total_return = 0.0;
    let mut cumulative_observed_return = 0.0;
    let mut max_abs_total_return: f64 = 0.0;
    let mut max_abs_deviation_from_anchor: f64 = 0.0;

    for record in records {
        cumulative_a_plus += record.sufficient_statistics.a_plus;
        cumulative_a_minus += record.sufficient_statistics.a_minus;

        cumulative_n_plus = cumulative_n_plus
            .checked_add(record.sufficient_statistics.n_plus)
            .ok_or(CoarseSimulationPathError::InvalidDiagnostics {
                field_path: "diagnostics.cumulative_n_plus",
                expected: "finite cumulative count".to_string(),
                actual: "overflow".to_string(),
            })?;

        cumulative_n_minus = cumulative_n_minus
            .checked_add(record.sufficient_statistics.n_minus)
            .ok_or(CoarseSimulationPathError::InvalidDiagnostics {
                field_path: "diagnostics.cumulative_n_minus",
                expected: "finite cumulative count".to_string(),
                actual: "overflow".to_string(),
            })?;

        cumulative_trading_time_return += record.return_components.trading_time_return;
        cumulative_open_jump_return += record.return_components.open_jump_return;
        cumulative_closed_gap_return += record.return_components.closed_gap_return;
        cumulative_total_return += record.return_components.total_return;
        cumulative_observed_return += record.observed_interval_return;

        max_abs_total_return =
            max_abs_total_return.max(record.return_components.total_return.abs());

        max_abs_deviation_from_anchor =
            max_abs_deviation_from_anchor.max(record.deviation_from_anchor_end.abs());
    }

    Ok(CoarseSimulationPathDiagnostics {
        schema_version: COARSE_SIMULATION_PATH_DIAGNOSTICS_SCHEMA_VERSION.to_string(),
        simulated_interval_count,
        first_interval_index: first_record.interval_index,
        last_interval_index: last_record.interval_index,
        first_interval_start_utc_nanos: first_record.interval_start_utc_nanos,
        last_interval_end_utc_nanos: last_record.interval_end_utc_nanos,
        cumulative_a_plus,
        cumulative_a_minus,
        cumulative_n_plus,
        cumulative_n_minus,
        cumulative_trading_time_return,
        cumulative_open_jump_return,
        cumulative_closed_gap_return,
        cumulative_total_return,
        cumulative_observed_return,
        max_abs_total_return,
        max_abs_deviation_from_anchor,
        terminal_observed_log_price: last_record.observed_log_price_end,
        terminal_deviation_from_anchor: last_record.deviation_from_anchor_end,
    })
}

pub(super) fn derive_path_end_time_utc_nanos(
    start_time_utc_nanos: i64,
    interval_delta: f64,
    simulated_interval_count: usize,
    records: &[CoarseSimulationPathRecord],
) -> Result<i64, CoarseSimulationPathError> {
    let interval_delta_nanos = interval_delta_to_nanos(interval_delta)
        .map_err(|source| CoarseSimulationPathError::InvalidIntervalDelta { source })?;

    if simulated_interval_count == 0 {
        return Err(CoarseSimulationPathError::InvalidSimulatedIntervalCount { value: 0 });
    }

    if records.len() != simulated_interval_count {
        return Err(CoarseSimulationPathError::RecordCountMismatch {
            simulated_interval_count,
            record_count: records.len(),
        });
    }

    let expected_end_time_utc_nanos = compute_horizon_end_time_utc_nanos(
        start_time_utc_nanos,
        interval_delta_nanos,
        simulated_interval_count,
    )
    .map_err(|source| CoarseSimulationPathError::InvalidTimeIndex { source })?;

    if records[simulated_interval_count - 1].interval_end_utc_nanos != expected_end_time_utc_nanos {
        return Err(CoarseSimulationPathError::InvalidEndTimeAlignment {
            expected_end_time_utc_nanos,
            actual_end_time_utc_nanos: records[simulated_interval_count - 1].interval_end_utc_nanos,
        });
    }

    Ok(expected_end_time_utc_nanos)
}

pub(super) fn validate_output_schema_metadata(
    metadata: &CoarseSimulationPathOutputSchemaMetadata,
) -> Result<(), CoarseSimulationPathError> {
    let expected = build_coarse_simulation_path_output_schema_metadata();

    validate_metadata_field(
        "output_schema_metadata.schema_version",
        &metadata.schema_version,
        &expected.schema_version,
    )?;

    validate_metadata_field(
        "output_schema_metadata.provenance_schema_version",
        &metadata.provenance_schema_version,
        &expected.provenance_schema_version,
    )?;

    validate_metadata_field(
        "output_schema_metadata.diagnostics_schema_version",
        &metadata.diagnostics_schema_version,
        &expected.diagnostics_schema_version,
    )?;

    validate_metadata_field(
        "output_schema_metadata.artifact_workflow_policy",
        &metadata.artifact_workflow_policy,
        &expected.artifact_workflow_policy,
    )?;

    validate_metadata_field(
        "output_schema_metadata.environment_metadata_policy",
        &metadata.environment_metadata_policy,
        &expected.environment_metadata_policy,
    )?;

    validate_metadata_list_field(
        "output_schema_metadata.required_path_fields",
        &metadata.required_path_fields,
        &expected.required_path_fields,
    )?;

    validate_metadata_list_field(
        "output_schema_metadata.required_record_fields",
        &metadata.required_record_fields,
        &expected.required_record_fields,
    )?;

    validate_metadata_list_field(
        "output_schema_metadata.required_provenance_fields",
        &metadata.required_provenance_fields,
        &expected.required_provenance_fields,
    )?;

    validate_metadata_list_field(
        "output_schema_metadata.required_diagnostics_fields",
        &metadata.required_diagnostics_fields,
        &expected.required_diagnostics_fields,
    )?;

    Ok(())
}

pub(super) fn validate_provenance(
    provenance: &CoarseSimulationPathProvenance,
) -> Result<(), CoarseSimulationPathError> {
    let expected = build_coarse_simulation_path_provenance();

    validate_provenance_field(
        "provenance.schema_version",
        &provenance.schema_version,
        &expected.schema_version,
    )?;

    validate_provenance_field(
        "provenance.artifact_workflow_policy",
        &provenance.artifact_workflow_policy,
        &expected.artifact_workflow_policy,
    )?;

    validate_provenance_field(
        "provenance.environment_metadata_policy",
        &provenance.environment_metadata_policy,
        &expected.environment_metadata_policy,
    )?;

    validate_provenance_field(
        "provenance.interval_timing_policy",
        &provenance.interval_timing_policy,
        &expected.interval_timing_policy,
    )?;

    validate_provenance_field(
        "provenance.interval_identifier_policy",
        &provenance.interval_identifier_policy,
        &expected.interval_identifier_policy,
    )?;

    validate_provenance_field(
        "provenance.interval_factor_alignment_policy",
        &provenance.interval_factor_alignment_policy,
        &expected.interval_factor_alignment_policy,
    )?;

    validate_provenance_field(
        "provenance.compatible_interval_grid_policy",
        &provenance.compatible_interval_grid_policy,
        &expected.compatible_interval_grid_policy,
    )?;

    validate_provenance_field(
        "provenance.interval_factor_policy",
        &provenance.interval_factor_policy,
        &expected.interval_factor_policy,
    )?;

    validate_provenance_field(
        "provenance.transform_slice_sampling_policy",
        &provenance.transform_slice_sampling_policy,
        &expected.transform_slice_sampling_policy,
    )?;

    validate_provenance_field(
        "provenance.boundary_state_update_policy",
        &provenance.boundary_state_update_policy,
        &expected.boundary_state_update_policy,
    )?;

    validate_provenance_field(
        "provenance.interval_return_generation_policy",
        &provenance.interval_return_generation_policy,
        &expected.interval_return_generation_policy,
    )?;

    validate_provenance_field(
        "provenance.interval_return_component_routing_policy",
        &provenance.interval_return_component_routing_policy,
        &expected.interval_return_component_routing_policy,
    )?;

    validate_provenance_field(
        "provenance.batch_seed_partition_policy",
        &provenance.batch_seed_partition_policy,
        &expected.batch_seed_partition_policy,
    )?;

    Ok(())
}

pub(super) fn validate_diagnostics(
    diagnostics: &CoarseSimulationPathDiagnostics,
    expected: &CoarseSimulationPathDiagnostics,
) -> Result<(), CoarseSimulationPathError> {
    validate_diagnostics_field(
        "diagnostics.schema_version",
        &diagnostics.schema_version,
        &expected.schema_version,
    )?;

    validate_diagnostics_field(
        "diagnostics.simulated_interval_count",
        &diagnostics.simulated_interval_count.to_string(),
        &expected.simulated_interval_count.to_string(),
    )?;

    validate_diagnostics_field(
        "diagnostics.first_interval_index",
        &diagnostics.first_interval_index.to_string(),
        &expected.first_interval_index.to_string(),
    )?;

    validate_diagnostics_field(
        "diagnostics.last_interval_index",
        &diagnostics.last_interval_index.to_string(),
        &expected.last_interval_index.to_string(),
    )?;

    validate_diagnostics_field(
        "diagnostics.first_interval_start_utc_nanos",
        &diagnostics.first_interval_start_utc_nanos.to_string(),
        &expected.first_interval_start_utc_nanos.to_string(),
    )?;

    validate_diagnostics_field(
        "diagnostics.last_interval_end_utc_nanos",
        &diagnostics.last_interval_end_utc_nanos.to_string(),
        &expected.last_interval_end_utc_nanos.to_string(),
    )?;

    validate_diagnostics_field(
        "diagnostics.cumulative_a_plus",
        &diagnostics.cumulative_a_plus.to_string(),
        &expected.cumulative_a_plus.to_string(),
    )?;

    validate_diagnostics_field(
        "diagnostics.cumulative_a_minus",
        &diagnostics.cumulative_a_minus.to_string(),
        &expected.cumulative_a_minus.to_string(),
    )?;

    validate_diagnostics_field(
        "diagnostics.cumulative_n_plus",
        &diagnostics.cumulative_n_plus.to_string(),
        &expected.cumulative_n_plus.to_string(),
    )?;

    validate_diagnostics_field(
        "diagnostics.cumulative_n_minus",
        &diagnostics.cumulative_n_minus.to_string(),
        &expected.cumulative_n_minus.to_string(),
    )?;

    validate_diagnostics_field(
        "diagnostics.cumulative_trading_time_return",
        &diagnostics.cumulative_trading_time_return.to_string(),
        &expected.cumulative_trading_time_return.to_string(),
    )?;

    validate_diagnostics_field(
        "diagnostics.cumulative_open_jump_return",
        &diagnostics.cumulative_open_jump_return.to_string(),
        &expected.cumulative_open_jump_return.to_string(),
    )?;

    validate_diagnostics_field(
        "diagnostics.cumulative_closed_gap_return",
        &diagnostics.cumulative_closed_gap_return.to_string(),
        &expected.cumulative_closed_gap_return.to_string(),
    )?;

    validate_diagnostics_field(
        "diagnostics.cumulative_total_return",
        &diagnostics.cumulative_total_return.to_string(),
        &expected.cumulative_total_return.to_string(),
    )?;

    validate_diagnostics_field(
        "diagnostics.cumulative_observed_return",
        &diagnostics.cumulative_observed_return.to_string(),
        &expected.cumulative_observed_return.to_string(),
    )?;

    validate_diagnostics_field(
        "diagnostics.max_abs_total_return",
        &diagnostics.max_abs_total_return.to_string(),
        &expected.max_abs_total_return.to_string(),
    )?;

    validate_diagnostics_field(
        "diagnostics.max_abs_deviation_from_anchor",
        &diagnostics.max_abs_deviation_from_anchor.to_string(),
        &expected.max_abs_deviation_from_anchor.to_string(),
    )?;

    validate_diagnostics_field(
        "diagnostics.terminal_observed_log_price",
        &diagnostics.terminal_observed_log_price.to_string(),
        &expected.terminal_observed_log_price.to_string(),
    )?;

    validate_diagnostics_field(
        "diagnostics.terminal_deviation_from_anchor",
        &diagnostics.terminal_deviation_from_anchor.to_string(),
        &expected.terminal_deviation_from_anchor.to_string(),
    )?;

    Ok(())
}

fn validate_metadata_field(
    field_path: &'static str,
    actual: &str,
    expected: &str,
) -> Result<(), CoarseSimulationPathError> {
    if actual != expected {
        return Err(CoarseSimulationPathError::InvalidOutputSchemaMetadata {
            field_path,
            expected: expected.to_string(),
            actual: actual.to_string(),
        });
    }

    Ok(())
}

fn validate_metadata_list_field(
    field_path: &'static str,
    actual: &[String],
    expected: &[String],
) -> Result<(), CoarseSimulationPathError> {
    if actual != expected {
        return Err(CoarseSimulationPathError::InvalidOutputSchemaMetadata {
            field_path,
            expected: format!("{expected:?}"),
            actual: format!("{actual:?}"),
        });
    }

    Ok(())
}

fn validate_provenance_field(
    field_path: &'static str,
    actual: &str,
    expected: &str,
) -> Result<(), CoarseSimulationPathError> {
    if actual != expected {
        return Err(CoarseSimulationPathError::InvalidProvenance {
            field_path,
            expected: expected.to_string(),
            actual: actual.to_string(),
        });
    }

    Ok(())
}

fn validate_diagnostics_field(
    field_path: &'static str,
    actual: &str,
    expected: &str,
) -> Result<(), CoarseSimulationPathError> {
    if actual != expected {
        return Err(CoarseSimulationPathError::InvalidDiagnostics {
            field_path,
            expected: expected.to_string(),
            actual: actual.to_string(),
        });
    }

    Ok(())
}
