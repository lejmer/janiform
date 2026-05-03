use janiform::model::factors::hourly::{
    HourlyFactorContractDiagnostics, HourlyFactorContractOutput,
    HourlyFactorMaterializationMetadata,
};
use janiform::pricing::{
    AnchoredVanillaOptionPricingBoundaryInput, PricingIntervalSchedule,
    VanillaOptionGridBatchPricingExecutionInput, VanillaOptionGridBatchPricingRequest,
    VanillaOptionPricingExecutionInput, VanillaOptionPricingModelParameters,
    VanillaOptionPricingOptionalOutputSelection, VanillaOptionPricingRequest,
    VanillaOptionSurfacePricingExecutionInput, VanillaOptionSurfacePricingRequest,
    materialize_schedule_interval_factors,
};
use janiform_workflows::pricing::FittedModelSingleOptionPricingRequest;
use janiform_workflows::terminal::TerminalProgressMode;
use pyo3::prelude::{Bound, PyResult};
use pyo3::types::PyAny;

use crate::PricingExecutionError;
use crate::python_value::py_any_to_json_value;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct VanillaOptionPricingExecutionInputPayload {
    pub(super) request: VanillaOptionPricingRequest,
    pub(super) runtime_interval_schedule: PricingIntervalSchedule,
    pub(super) hourly_factor_contract: PricingHourlyFactorContractPayload,
    #[serde(default)]
    pub(super) optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct VanillaOptionGridBatchPricingExecutionInputPayload {
    request: VanillaOptionGridBatchPricingRequest,
    boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
    model_parameters: VanillaOptionPricingModelParameters,
    runtime_interval_schedule: PricingIntervalSchedule,
    interval_delta: f64,
    hourly_factor_contract: PricingHourlyFactorContractPayload,
    #[serde(default)]
    optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    #[serde(default)]
    spectral_collocation_node_count_override: Option<usize>,
    cpu_count: Option<usize>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct VanillaOptionSurfacePricingExecutionInputPayload {
    request: VanillaOptionSurfacePricingRequest,
    boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
    model_parameters: VanillaOptionPricingModelParameters,
    runtime_interval_schedule: PricingIntervalSchedule,
    interval_delta: f64,
    hourly_factor_contract: PricingHourlyFactorContractPayload,
    #[serde(default)]
    optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    #[serde(default)]
    spectral_collocation_node_count_override: Option<usize>,
    cpu_count: Option<usize>,
}

#[derive(serde::Deserialize)]
pub(super) struct PricingHourlyFactorContractPayload {
    interval_duration_nanos: i64,
    interval_start_utc_nanos: Vec<i64>,
    interval_end_utc_nanos: Vec<i64>,
    s_h: Vec<f64>,
    known_component_nu_h: Vec<f64>,
    surprise_component_nu_h: Vec<f64>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PricingActivityCompressedScheduleRequestPayload {
    pub(super) valuation_time_utc_nanos: i64,
    pub(super) expiry_timestamps_utc_nanos: Vec<i64>,
    pub(super) hourly_factor_contract: PricingHourlyFactorContractPayload,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FittedModelSingleOptionPricingWorkflowPayload {
    #[serde(flatten)]
    pub(super) request: FittedModelSingleOptionPricingRequest,
    #[serde(default)]
    pub(super) progress_mode: TerminalProgressMode,
}

pub(super) fn vanilla_option_pricing_execution_input_from_py(
    execution_input: &Bound<'_, PyAny>,
) -> PyResult<VanillaOptionPricingExecutionInput> {
    let execution_input: VanillaOptionPricingExecutionInputPayload =
        serde_json::from_value(py_any_to_json_value(execution_input)?).map_err(|source| {
            PricingExecutionError::new_err(format!(
                "failed to deserialize vanilla option pricing execution input: {source}"
            ))
        })?;

    let hourly_factor_contract =
        hourly_factor_contract_output_from_payload(execution_input.hourly_factor_contract);

    let interval_factors = materialize_schedule_interval_factors(
        &execution_input.runtime_interval_schedule,
        &hourly_factor_contract,
    )
    .map_err(|source| PricingExecutionError::new_err(source.to_string()))?;

    VanillaOptionPricingExecutionInput::try_new(
        execution_input.request,
        execution_input.runtime_interval_schedule,
        interval_factors,
        execution_input.optional_output_selection,
    )
    .map_err(|source| PricingExecutionError::new_err(source.to_string()))
}

pub(super) fn grid_batch_execution_input_from_py(
    execution_input: &Bound<'_, PyAny>,
) -> PyResult<VanillaOptionGridBatchPricingExecutionInput> {
    let execution_input: VanillaOptionGridBatchPricingExecutionInputPayload =
        serde_json::from_value(py_any_to_json_value(execution_input)?).map_err(|source| {
            PricingExecutionError::new_err(format!(
                "failed to deserialize vanilla option grid batch pricing execution input: {source}"
            ))
        })?;

    let hourly_factor_contract =
        hourly_factor_contract_output_from_payload(execution_input.hourly_factor_contract);

    let interval_factors = materialize_schedule_interval_factors(
        &execution_input.runtime_interval_schedule,
        &hourly_factor_contract,
    )
    .map_err(|source| PricingExecutionError::new_err(source.to_string()))?;

    Ok(VanillaOptionGridBatchPricingExecutionInput {
        request: execution_input.request,
        boundary_input: execution_input.boundary_input,
        model_parameters: execution_input.model_parameters,
        runtime_interval_schedule: execution_input.runtime_interval_schedule,
        interval_delta: execution_input.interval_delta,
        interval_factors,
        optional_output_selection: execution_input.optional_output_selection,
        spectral_collocation_node_count_override: execution_input
            .spectral_collocation_node_count_override,
        cpu_count: execution_input.cpu_count,
    })
}

pub(super) fn surface_execution_input_from_py(
    execution_input: &Bound<'_, PyAny>,
) -> PyResult<VanillaOptionSurfacePricingExecutionInput> {
    let execution_input: VanillaOptionSurfacePricingExecutionInputPayload =
        serde_json::from_value(py_any_to_json_value(execution_input)?).map_err(|source| {
            PricingExecutionError::new_err(format!(
                "failed to deserialize vanilla option surface pricing execution input: {source}"
            ))
        })?;

    let hourly_factor_contract =
        hourly_factor_contract_output_from_payload(execution_input.hourly_factor_contract);

    let interval_factors = materialize_schedule_interval_factors(
        &execution_input.runtime_interval_schedule,
        &hourly_factor_contract,
    )
    .map_err(|source| PricingExecutionError::new_err(source.to_string()))?;

    Ok(VanillaOptionSurfacePricingExecutionInput {
        request: execution_input.request,
        boundary_input: execution_input.boundary_input,
        model_parameters: execution_input.model_parameters,
        runtime_interval_schedule: execution_input.runtime_interval_schedule,
        interval_delta: execution_input.interval_delta,
        interval_factors,
        optional_output_selection: execution_input.optional_output_selection,
        spectral_collocation_node_count_override: execution_input
            .spectral_collocation_node_count_override,
        cpu_count: execution_input.cpu_count,
    })
}

pub(super) fn hourly_factor_contract_output_from_payload(
    payload: PricingHourlyFactorContractPayload,
) -> HourlyFactorContractOutput {
    let interval_count = payload.interval_start_utc_nanos.len();

    let first_interval_start_utc_nanos = payload
        .interval_start_utc_nanos
        .first()
        .copied()
        .unwrap_or_default();

    let last_interval_end_utc_nanos = payload
        .interval_end_utc_nanos
        .last()
        .copied()
        .unwrap_or_default();

    HourlyFactorContractOutput {
        schema_version: "",
        exchange_code: String::new(),
        exchange_timezone_name: String::new(),
        request_start_utc_nanos: first_interval_start_utc_nanos,
        request_end_utc_nanos: last_interval_end_utc_nanos,
        interval_duration_nanos: payload.interval_duration_nanos,
        interval_indices: (0..interval_count).map(|index| index as u64).collect(),
        interval_start_utc_nanos: payload.interval_start_utc_nanos,
        interval_end_utc_nanos: payload.interval_end_utc_nanos,
        interval_start_exchange_local: vec![String::new(); interval_count],
        interval_end_exchange_local: vec![String::new(); interval_count],
        nu_h: payload
            .known_component_nu_h
            .iter()
            .zip(payload.surprise_component_nu_h.iter())
            .map(|(known_component_nu_h, surprise_component_nu_h)| {
                known_component_nu_h + surprise_component_nu_h
            })
            .collect(),
        s_h: payload.s_h,
        known_component_nu_h: payload.known_component_nu_h,
        surprise_component_nu_h: payload.surprise_component_nu_h,
        metadata: HourlyFactorMaterializationMetadata {
            seasonality_materialization_method: String::new(),
            news_intensity_materialization_method: String::new(),
            interval_boundary_policy: "",
            interval_alignment_policy: "",
            timezone_transition_policy: "",
            spring_forward_policy: "",
            fall_back_policy: "",
        },
        diagnostics: HourlyFactorContractDiagnostics {
            interval_count,
            interval_duration_nanos: payload.interval_duration_nanos,
            first_interval_start_utc_nanos,
            last_interval_end_utc_nanos,
            skipped_local_hour_interval_count: 0,
            repeated_local_hour_interval_count: 0,
        },
    }
}
