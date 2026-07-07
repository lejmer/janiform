use janiform::model::factors::interval_factor_grid::{
    IntervalFactorGrid, IntervalFactorGridDiagnostics, IntervalFactorGridMetadata,
};
use janiform::pricing::{
    AnchoredVanillaOptionPricingBoundaryInput, PricingIntervalSchedule,
    VanillaOptionGridBatchPricingExecutionInput, VanillaOptionGridBatchPricingRequest,
    VanillaOptionPricingExecutionInput, VanillaOptionPricingModelParameters,
    VanillaOptionPricingOptionalOutputSelection, VanillaOptionPricingRequest,
    VanillaOptionSurfacePricingExecutionInput, VanillaOptionSurfacePricingRequest,
    materialize_schedule_interval_factor_grids,
};
use janiform_workflows::pricing::{
    FittedModelSingleOptionPricingRequest, ObservedOptionsPricingRequest,
};
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
    pub(super) interval_factor_grid: PricingIntervalFactorGridPayload,
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
    interval_factor_grid: PricingIntervalFactorGridPayload,
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
    interval_factor_grid: PricingIntervalFactorGridPayload,
    #[serde(default)]
    optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    #[serde(default)]
    spectral_collocation_node_count_override: Option<usize>,
    cpu_count: Option<usize>,
}

#[derive(serde::Deserialize)]
pub(super) struct PricingIntervalFactorGridPayload {
    exchange_code: String,
    exchange_timezone_name: String,
    request_start_utc_nanos: i64,
    request_end_utc_nanos: i64,
    interval_duration_nanos: i64,
    interval_indices: Vec<u64>,
    interval_start_utc_nanos: Vec<i64>,
    interval_end_utc_nanos: Vec<i64>,
    interval_start_exchange_local: Vec<String>,
    interval_end_exchange_local: Vec<String>,
    seasonality_factor: Vec<f64>,
    known_news_intensity: Vec<f64>,
    surprise_news_intensity: Vec<f64>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PricingActivityCompressedScheduleRequestPayload {
    pub(super) valuation_time_utc_nanos: i64,
    pub(super) expiry_timestamps_utc_nanos: Vec<i64>,
    pub(super) interval_factor_grid: PricingIntervalFactorGridPayload,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FittedModelSingleOptionPricingWorkflowPayload {
    #[serde(flatten)]
    pub(super) request: FittedModelSingleOptionPricingRequest,
    #[serde(default)]
    pub(super) progress_mode: TerminalProgressMode,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ObservedOptionsPricingWorkflowPayload {
    #[serde(flatten)]
    pub(super) request: ObservedOptionsPricingRequest,
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

    let interval_factor_grid =
        interval_factor_grid_output_from_payload(execution_input.interval_factor_grid);

    let interval_factor_grids = materialize_schedule_interval_factor_grids(
        &execution_input.runtime_interval_schedule,
        &interval_factor_grid,
    )
    .map_err(|source| PricingExecutionError::new_err(source.to_string()))?;

    VanillaOptionPricingExecutionInput::try_new(
        execution_input.request,
        execution_input.runtime_interval_schedule,
        interval_factor_grids,
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

    let interval_factor_grid =
        interval_factor_grid_output_from_payload(execution_input.interval_factor_grid);

    let interval_factor_grids = materialize_schedule_interval_factor_grids(
        &execution_input.runtime_interval_schedule,
        &interval_factor_grid,
    )
    .map_err(|source| PricingExecutionError::new_err(source.to_string()))?;

    Ok(VanillaOptionGridBatchPricingExecutionInput {
        request: execution_input.request,
        boundary_input: execution_input.boundary_input,
        model_parameters: execution_input.model_parameters,
        runtime_interval_schedule: execution_input.runtime_interval_schedule,
        interval_delta: execution_input.interval_delta,
        interval_factor_grids,
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

    let interval_factor_grid =
        interval_factor_grid_output_from_payload(execution_input.interval_factor_grid);

    let interval_factor_grids = materialize_schedule_interval_factor_grids(
        &execution_input.runtime_interval_schedule,
        &interval_factor_grid,
    )
    .map_err(|source| PricingExecutionError::new_err(source.to_string()))?;

    Ok(VanillaOptionSurfacePricingExecutionInput {
        request: execution_input.request,
        boundary_input: execution_input.boundary_input,
        model_parameters: execution_input.model_parameters,
        runtime_interval_schedule: execution_input.runtime_interval_schedule,
        interval_delta: execution_input.interval_delta,
        interval_factor_grids,
        optional_output_selection: execution_input.optional_output_selection,
        spectral_collocation_node_count_override: execution_input
            .spectral_collocation_node_count_override,
        cpu_count: execution_input.cpu_count,
    })
}

pub(super) fn interval_factor_grid_output_from_payload(
    payload: PricingIntervalFactorGridPayload,
) -> IntervalFactorGrid {
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

    IntervalFactorGrid {
        schema_version: "",
        exchange_code: payload.exchange_code,
        exchange_timezone_name: payload.exchange_timezone_name,
        request_start_utc_nanos: payload.request_start_utc_nanos,
        request_end_utc_nanos: payload.request_end_utc_nanos,
        interval_duration_nanos: payload.interval_duration_nanos,
        interval_indices: payload.interval_indices,
        interval_start_utc_nanos: payload.interval_start_utc_nanos,
        interval_end_utc_nanos: payload.interval_end_utc_nanos,
        interval_start_exchange_local: payload.interval_start_exchange_local,
        interval_end_exchange_local: payload.interval_end_exchange_local,
        news_intensity: payload
            .known_news_intensity
            .iter()
            .zip(payload.surprise_news_intensity.iter())
            .map(|(known_news_intensity, surprise_news_intensity)| {
                known_news_intensity + surprise_news_intensity
            })
            .collect(),
        seasonality_factor: payload.seasonality_factor,
        known_news_intensity: payload.known_news_intensity,
        surprise_news_intensity: payload.surprise_news_intensity,
        metadata: IntervalFactorGridMetadata {
            seasonality_materialization_method: String::new(),
            news_intensity_materialization_method: String::new(),
            interval_boundary_policy: "",
            interval_alignment_policy: "",
            timezone_transition_policy: "",
            spring_forward_policy: "",
            fall_back_policy: "",
        },
        diagnostics: IntervalFactorGridDiagnostics {
            interval_count,
            interval_duration_nanos: payload.interval_duration_nanos,
            first_interval_start_utc_nanos,
            last_interval_end_utc_nanos,
            skipped_exchange_local_interval_count: 0,
            repeated_exchange_local_interval_count: 0,
        },
    }
}
