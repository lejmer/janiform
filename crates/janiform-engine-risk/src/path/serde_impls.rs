use serde::Deserialize;

use janiform_model_kernels::hawkes::HawkesExcitationState;

use super::super::request::{
    DEFAULT_INTRADAY_RISK_INTERVAL_DELTA, SUFFICIENT_STATISTICS_RISK_PATH_RECORD_SCHEMA_VERSION,
    SUFFICIENT_STATISTICS_RISK_PATH_SCHEMA_VERSION,
    SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION, SufficientStatisticsRiskIntervalFactors,
    SufficientStatisticsRiskModelInputs, SufficientStatisticsRiskRunRequest,
};
use super::super::returns::SufficientStatisticsRiskReturnComponents;
use super::super::transform_sampling::SufficientStatisticsIntervalStatistics;
use super::contracts::{
    SUFFICIENT_STATISTICS_RISK_PATH_OUTPUT_SCHEMA_METADATA,
    SUFFICIENT_STATISTICS_RISK_PATH_PROVENANCE, SufficientStatisticsRiskPath,
    SufficientStatisticsRiskPathDiagnostics, SufficientStatisticsRiskPathRecord,
};

const fn default_intraday_risk_interval_delta() -> f64 {
    DEFAULT_INTRADAY_RISK_INTERVAL_DELTA
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsRiskRunRequestPayload {
    seed: u64,
    horizon_interval_count: usize,
    #[serde(default = "default_intraday_risk_interval_delta")]
    interval_delta: f64,
    model_inputs: SufficientStatisticsRiskModelInputs,
    interval_factors: SufficientStatisticsRiskIntervalFactors,
}

impl<'de> Deserialize<'de> for SufficientStatisticsRiskRunRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = SufficientStatisticsRiskRunRequestPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.seed,
            payload.horizon_interval_count,
            payload.interval_delta,
            payload.model_inputs,
            payload.interval_factors,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsRiskPathRecordPayload {
    #[serde(default)]
    schema_version: Option<String>,
    interval_index: usize,
    sufficient_statistics: SufficientStatisticsIntervalStatistics,
    return_components: SufficientStatisticsRiskReturnComponents,
    core_total_return: f64,
    observed_interval_return: f64,
    observed_log_price_end: f64,
    anchor_value_end: f64,
    pressure_state_end: f64,
    deviation_from_anchor_end: f64,
}

impl<'de> Deserialize<'de> for SufficientStatisticsRiskPathRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = SufficientStatisticsRiskPathRecordPayload::deserialize(deserializer)?;

        if let Some(schema_version) = payload.schema_version.as_deref()
            && schema_version != SUFFICIENT_STATISTICS_RISK_PATH_RECORD_SCHEMA_VERSION
        {
            return Err(serde::de::Error::custom(format!(
                "schema_version must be {}, found {schema_version}",
                SUFFICIENT_STATISTICS_RISK_PATH_RECORD_SCHEMA_VERSION
            )));
        }

        Self::try_new(
            payload.interval_index,
            payload.sufficient_statistics,
            payload.return_components,
            payload.core_total_return,
            payload.observed_interval_return,
            payload.observed_log_price_end,
            payload.anchor_value_end,
            payload.pressure_state_end,
            payload.deviation_from_anchor_end,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsRiskPathPayload {
    #[serde(default)]
    schema_version: Option<String>,
    #[serde(default)]
    run_request_schema_version: Option<String>,
    #[serde(default)]
    output_schema_metadata: Option<SufficientStatisticsRiskPathOutputSchemaMetadataPayload>,
    #[serde(default)]
    provenance: Option<SufficientStatisticsRiskPathProvenancePayload>,
    #[serde(default)]
    diagnostics: Option<SufficientStatisticsRiskPathDiagnosticsPayload>,
    seed: u64,
    interval_delta: f64,
    simulated_interval_count: usize,
    initial_excitation_state: HawkesExcitationState,
    terminal_excitation_state: HawkesExcitationState,
    records: Vec<SufficientStatisticsRiskPathRecord>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsRiskPathOutputSchemaMetadataPayload {
    schema_version: String,
    provenance_schema_version: String,
    diagnostics_schema_version: String,
    artifact_workflow_policy: String,
    environment_metadata_policy: String,
    required_path_fields: Vec<String>,
    required_provenance_fields: Vec<String>,
    required_diagnostics_fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsRiskPathProvenancePayload {
    schema_version: String,
    artifact_workflow_policy: String,
    environment_metadata_policy: String,
    interval_factor_policy: String,
    transform_slice_sampling_policy: String,
    boundary_state_update_policy: String,
    interval_return_generation_policy: String,
    interval_return_component_routing_policy: String,
    batch_seed_partition_policy: String,
    artifact_workflow_tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsRiskPathDiagnosticsPayload {
    schema_version: String,
    simulated_interval_count: usize,
    first_interval_index: Option<usize>,
    last_interval_index: Option<usize>,
    cumulative_a_plus: f64,
    cumulative_a_minus: f64,
    cumulative_n_plus: u64,
    cumulative_n_minus: u64,
    cumulative_trading_time_return: f64,
    cumulative_open_jump_return: f64,
    cumulative_closed_gap_return: f64,
    cumulative_total_return: f64,
    cumulative_observed_return: f64,
    max_abs_total_return: f64,
    max_abs_deviation_from_anchor: f64,
    terminal_observed_log_price: f64,
    terminal_deviation_from_anchor: f64,
}

impl<'de> Deserialize<'de> for SufficientStatisticsRiskPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = SufficientStatisticsRiskPathPayload::deserialize(deserializer)?;

        if let Some(schema_version) = payload.schema_version.as_deref()
            && schema_version != SUFFICIENT_STATISTICS_RISK_PATH_SCHEMA_VERSION
        {
            return Err(serde::de::Error::custom(format!(
                "schema_version must be {}, found {schema_version}",
                SUFFICIENT_STATISTICS_RISK_PATH_SCHEMA_VERSION
            )));
        }

        if let Some(run_request_schema_version) = payload.run_request_schema_version.as_deref()
            && run_request_schema_version != SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION
        {
            return Err(serde::de::Error::custom(format!(
                "run_request_schema_version must be {}, found {run_request_schema_version}",
                SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION
            )));
        }

        let path = Self::try_new(
            payload.seed,
            payload.interval_delta,
            payload.simulated_interval_count,
            payload.initial_excitation_state,
            payload.terminal_excitation_state,
            payload.records,
        )
        .map_err(serde::de::Error::custom)?;

        if let Some(output_schema_metadata) = payload.output_schema_metadata.as_ref() {
            validate_path_output_schema_metadata_payload(output_schema_metadata)
                .map_err(serde::de::Error::custom)?;
        }

        if let Some(provenance) = payload.provenance.as_ref() {
            validate_path_provenance_payload(provenance).map_err(serde::de::Error::custom)?;
        }

        if let Some(diagnostics) = payload.diagnostics.as_ref() {
            validate_path_diagnostics_payload(diagnostics, path.diagnostics)
                .map_err(serde::de::Error::custom)?;
        }

        Ok(path)
    }
}

fn validate_path_output_schema_metadata_payload(
    payload: &SufficientStatisticsRiskPathOutputSchemaMetadataPayload,
) -> Result<(), String> {
    let expected = SUFFICIENT_STATISTICS_RISK_PATH_OUTPUT_SCHEMA_METADATA;

    if payload.schema_version != expected.schema_version {
        return Err(format!(
            "output_schema_metadata.schema_version must be {}, found {}",
            expected.schema_version, payload.schema_version
        ));
    }
    if payload.provenance_schema_version != expected.provenance_schema_version {
        return Err(format!(
            "output_schema_metadata.provenance_schema_version must be {}, found {}",
            expected.provenance_schema_version, payload.provenance_schema_version
        ));
    }
    if payload.diagnostics_schema_version != expected.diagnostics_schema_version {
        return Err(format!(
            "output_schema_metadata.diagnostics_schema_version must be {}, found {}",
            expected.diagnostics_schema_version, payload.diagnostics_schema_version
        ));
    }
    if payload.artifact_workflow_policy != expected.artifact_workflow_policy {
        return Err(format!(
            "output_schema_metadata.artifact_workflow_policy must be {}, found {}",
            expected.artifact_workflow_policy, payload.artifact_workflow_policy
        ));
    }
    if payload.environment_metadata_policy != expected.environment_metadata_policy {
        return Err(format!(
            "output_schema_metadata.environment_metadata_policy must be {}, found {}",
            expected.environment_metadata_policy, payload.environment_metadata_policy
        ));
    }

    validate_required_field_list(
        "output_schema_metadata.required_path_fields",
        payload.required_path_fields.as_slice(),
        expected.required_path_fields,
    )?;
    validate_required_field_list(
        "output_schema_metadata.required_provenance_fields",
        payload.required_provenance_fields.as_slice(),
        expected.required_provenance_fields,
    )?;
    validate_required_field_list(
        "output_schema_metadata.required_diagnostics_fields",
        payload.required_diagnostics_fields.as_slice(),
        expected.required_diagnostics_fields,
    )?;

    Ok(())
}

fn validate_path_provenance_payload(
    payload: &SufficientStatisticsRiskPathProvenancePayload,
) -> Result<(), String> {
    let expected = SUFFICIENT_STATISTICS_RISK_PATH_PROVENANCE;

    if payload.schema_version != expected.schema_version {
        return Err(format!(
            "provenance.schema_version must be {}, found {}",
            expected.schema_version, payload.schema_version
        ));
    }
    if payload.artifact_workflow_policy != expected.artifact_workflow_policy {
        return Err(format!(
            "provenance.artifact_workflow_policy must be {}, found {}",
            expected.artifact_workflow_policy, payload.artifact_workflow_policy
        ));
    }
    if payload.environment_metadata_policy != expected.environment_metadata_policy {
        return Err(format!(
            "provenance.environment_metadata_policy must be {}, found {}",
            expected.environment_metadata_policy, payload.environment_metadata_policy
        ));
    }
    if payload.interval_factor_policy != expected.interval_factor_policy {
        return Err(format!(
            "provenance.interval_factor_policy must be {}, found {}",
            expected.interval_factor_policy, payload.interval_factor_policy
        ));
    }
    if payload.transform_slice_sampling_policy != expected.transform_slice_sampling_policy {
        return Err(format!(
            "provenance.transform_slice_sampling_policy must be {}, found {}",
            expected.transform_slice_sampling_policy, payload.transform_slice_sampling_policy
        ));
    }
    if payload.boundary_state_update_policy != expected.boundary_state_update_policy {
        return Err(format!(
            "provenance.boundary_state_update_policy must be {}, found {}",
            expected.boundary_state_update_policy, payload.boundary_state_update_policy
        ));
    }
    if payload.interval_return_generation_policy != expected.interval_return_generation_policy {
        return Err(format!(
            "provenance.interval_return_generation_policy must be {}, found {}",
            expected.interval_return_generation_policy, payload.interval_return_generation_policy
        ));
    }
    if payload.interval_return_component_routing_policy
        != expected.interval_return_component_routing_policy
    {
        return Err(format!(
            "provenance.interval_return_component_routing_policy must be {}, found {}",
            expected.interval_return_component_routing_policy,
            payload.interval_return_component_routing_policy
        ));
    }
    if payload.batch_seed_partition_policy != expected.batch_seed_partition_policy {
        return Err(format!(
            "provenance.batch_seed_partition_policy must be {}, found {}",
            expected.batch_seed_partition_policy, payload.batch_seed_partition_policy
        ));
    }

    validate_required_field_list(
        "provenance.artifact_workflow_tags",
        payload.artifact_workflow_tags.as_slice(),
        expected.artifact_workflow_tags,
    )?;

    Ok(())
}

fn validate_path_diagnostics_payload(
    payload: &SufficientStatisticsRiskPathDiagnosticsPayload,
    expected: SufficientStatisticsRiskPathDiagnostics,
) -> Result<(), String> {
    if payload.schema_version != expected.schema_version {
        return Err(format!(
            "diagnostics.schema_version must be {}, found {}",
            expected.schema_version, payload.schema_version
        ));
    }
    if payload.simulated_interval_count != expected.simulated_interval_count {
        return Err(format!(
            "diagnostics.simulated_interval_count must be {}, found {}",
            expected.simulated_interval_count, payload.simulated_interval_count
        ));
    }
    if payload.first_interval_index != expected.first_interval_index {
        return Err(format!(
            "diagnostics.first_interval_index must be {:?}, found {:?}",
            expected.first_interval_index, payload.first_interval_index
        ));
    }
    if payload.last_interval_index != expected.last_interval_index {
        return Err(format!(
            "diagnostics.last_interval_index must be {:?}, found {:?}",
            expected.last_interval_index, payload.last_interval_index
        ));
    }
    if payload.cumulative_a_plus != expected.cumulative_a_plus {
        return Err(format!(
            "diagnostics.cumulative_a_plus must be {}, found {}",
            expected.cumulative_a_plus, payload.cumulative_a_plus
        ));
    }
    if payload.cumulative_a_minus != expected.cumulative_a_minus {
        return Err(format!(
            "diagnostics.cumulative_a_minus must be {}, found {}",
            expected.cumulative_a_minus, payload.cumulative_a_minus
        ));
    }
    if payload.cumulative_n_plus != expected.cumulative_n_plus {
        return Err(format!(
            "diagnostics.cumulative_n_plus must be {}, found {}",
            expected.cumulative_n_plus, payload.cumulative_n_plus
        ));
    }
    if payload.cumulative_n_minus != expected.cumulative_n_minus {
        return Err(format!(
            "diagnostics.cumulative_n_minus must be {}, found {}",
            expected.cumulative_n_minus, payload.cumulative_n_minus
        ));
    }
    if payload.cumulative_trading_time_return != expected.cumulative_trading_time_return {
        return Err(format!(
            "diagnostics.cumulative_trading_time_return must be {}, found {}",
            expected.cumulative_trading_time_return, payload.cumulative_trading_time_return
        ));
    }
    if payload.cumulative_open_jump_return != expected.cumulative_open_jump_return {
        return Err(format!(
            "diagnostics.cumulative_open_jump_return must be {}, found {}",
            expected.cumulative_open_jump_return, payload.cumulative_open_jump_return
        ));
    }
    if payload.cumulative_closed_gap_return != expected.cumulative_closed_gap_return {
        return Err(format!(
            "diagnostics.cumulative_closed_gap_return must be {}, found {}",
            expected.cumulative_closed_gap_return, payload.cumulative_closed_gap_return
        ));
    }
    if payload.cumulative_total_return != expected.cumulative_total_return {
        return Err(format!(
            "diagnostics.cumulative_total_return must be {}, found {}",
            expected.cumulative_total_return, payload.cumulative_total_return
        ));
    }
    if payload.cumulative_observed_return != expected.cumulative_observed_return {
        return Err(format!(
            "diagnostics.cumulative_observed_return must be {}, found {}",
            expected.cumulative_observed_return, payload.cumulative_observed_return
        ));
    }
    if payload.max_abs_total_return != expected.max_abs_total_return {
        return Err(format!(
            "diagnostics.max_abs_total_return must be {}, found {}",
            expected.max_abs_total_return, payload.max_abs_total_return
        ));
    }
    if payload.max_abs_deviation_from_anchor != expected.max_abs_deviation_from_anchor {
        return Err(format!(
            "diagnostics.max_abs_deviation_from_anchor must be {}, found {}",
            expected.max_abs_deviation_from_anchor, payload.max_abs_deviation_from_anchor
        ));
    }
    if payload.terminal_observed_log_price != expected.terminal_observed_log_price {
        return Err(format!(
            "diagnostics.terminal_observed_log_price must be {}, found {}",
            expected.terminal_observed_log_price, payload.terminal_observed_log_price
        ));
    }
    if payload.terminal_deviation_from_anchor != expected.terminal_deviation_from_anchor {
        return Err(format!(
            "diagnostics.terminal_deviation_from_anchor must be {}, found {}",
            expected.terminal_deviation_from_anchor, payload.terminal_deviation_from_anchor
        ));
    }

    Ok(())
}

fn validate_required_field_list(
    field_path: &str,
    provided: &[String],
    expected: &[&str],
) -> Result<(), String> {
    if provided.len() != expected.len() {
        return Err(format!(
            "{field_path} must contain {} entries, found {}",
            expected.len(),
            provided.len()
        ));
    }

    for (field_index, expected_field_name) in expected.iter().enumerate() {
        if provided[field_index] != *expected_field_name {
            return Err(format!(
                "{field_path}[{field_index}] must be {}, found {}",
                expected_field_name, provided[field_index]
            ));
        }
    }

    Ok(())
}
