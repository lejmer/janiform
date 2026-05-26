use janiform::model::news::NewsProcessConfiguration;
use janiform::model::return_clock::ReturnClockExposureParameters;
use janiform::model::sufficient_statistics::SufficientStatisticsIntervalFactors;
use janiform::simulation::{
    CoarseSimulationModelInputs, CoarseSimulationRunRequest, SeededTickEventStateSnapshotCapture,
    SeededTickSimulationDebugTraceOptions, SeededTickSimulationModelInputs,
    SeededTickSimulationRunConfig, SeededTickSimulationRunOptions,
};
use janiform_workflows::simulation::ModelArtifactSimulationWorkflowRequest;
use pyo3::prelude::{Bound, PyRef, PyResult};
use pyo3::types::{PyAny, PyMapping};

use crate::SeasonalityInputValidationError;
use crate::SimulationExecutionError;
use crate::bindings::calendar::parse_timestamp_utc;
use crate::python_value::py_any_to_json_value;
use crate::types::PyHourlyFactorContractResult;

use super::fields::{
    parse_required_field, parse_required_hourly_factor_contract, parse_required_json_field,
    parse_required_string_field,
};
use super::model_inputs::{
    parse_seeded_simulation_initial_excitation_state, parse_seeded_simulation_kernel_parameters,
    parse_seeded_simulation_mean_reversion_boundary_state,
    parse_seeded_simulation_mean_reversion_fitted_parameters,
    parse_seeded_simulation_mean_reversion_runtime_interval_anchors,
    parse_seeded_simulation_nig_shared_plus_shift_configuration,
};
use super::output::map_seeded_tick_simulation_run_error;

#[derive(Debug)]
pub(super) struct ModelArtifactSimulationWorkflowPayload {
    request: ModelArtifactSimulationWorkflowRequest,
}

impl ModelArtifactSimulationWorkflowPayload {
    pub(super) fn from_py(raw_request: &Bound<'_, PyAny>) -> PyResult<Self> {
        let request: ModelArtifactSimulationWorkflowRequest = serde_json::from_value(
            py_any_to_json_value(raw_request)?,
        )
        .map_err(|source| {
            SimulationExecutionError::new_err(format!(
                "failed to deserialize simulation-from-model-artifact workflow request: {source}"
            ))
        })?;

        Ok(Self { request })
    }

    pub(super) fn into_workflow_request(self) -> ModelArtifactSimulationWorkflowRequest {
        self.request
    }
}

pub(super) struct CoarseSimulationWorkflowRequestPayload<'py> {
    seed: u64,
    start_time_utc_nanos: i64,
    horizon_interval_count: usize,
    interval_delta: f64,
    model_inputs: CoarseSimulationModelInputs,
    hourly_factor_contract: PyRef<'py, PyHourlyFactorContractResult>,
}

impl<'py> CoarseSimulationWorkflowRequestPayload<'py> {
    pub(super) fn from_py(raw_request: &Bound<'py, PyAny>) -> PyResult<Self> {
        let request = raw_request.cast::<PyMapping>().map_err(|_| {
            SimulationExecutionError::new_err("coarse simulation request must be a mapping")
        })?;

        Ok(Self {
            seed: parse_required_json_field(request, "coarse_simulation_request", "seed")?,
            start_time_utc_nanos: parse_required_json_field(
                request,
                "coarse_simulation_request",
                "start_time_utc_nanos",
            )?,
            horizon_interval_count: parse_required_json_field(
                request,
                "coarse_simulation_request",
                "horizon_interval_count",
            )?,
            interval_delta: parse_required_json_field(
                request,
                "coarse_simulation_request",
                "interval_delta",
            )?,
            model_inputs: parse_required_json_field(
                request,
                "coarse_simulation_request",
                "model_inputs",
            )?,
            hourly_factor_contract: parse_required_hourly_factor_contract(
                request,
                "coarse_simulation_request",
            )?,
        })
    }

    pub(super) fn into_run_request(self) -> PyResult<CoarseSimulationRunRequest> {
        let interval_factors = SufficientStatisticsIntervalFactors::try_new(
            self.hourly_factor_contract.output.s_h.clone(),
            self.hourly_factor_contract
                .output
                .known_component_nu_h
                .clone(),
            self.hourly_factor_contract
                .output
                .surprise_component_nu_h
                .clone(),
            Some(self.interval_delta),
        )
        .map_err(|source| SimulationExecutionError::new_err(source.to_string()))?;

        CoarseSimulationRunRequest::try_new(
            self.seed,
            self.start_time_utc_nanos,
            self.horizon_interval_count,
            self.interval_delta,
            self.model_inputs,
            interval_factors,
        )
        .map_err(|source| SimulationExecutionError::new_err(source.to_string()))
    }
}

pub(super) struct SeededTickSimulationWorkflowRequestPayload<'py> {
    seed: u64,
    start_time_utc: String,
    horizon_interval_count: usize,
    hourly_factor_contract: PyRef<'py, PyHourlyFactorContractResult>,
    model_inputs: SeededTickSimulationModelInputsPayload,
    run_options: SeededTickSimulationRunOptions,
}

impl<'py> SeededTickSimulationWorkflowRequestPayload<'py> {
    pub(super) fn from_py(raw_request: &Bound<'py, PyAny>) -> PyResult<Self> {
        let request = raw_request.cast::<PyMapping>().map_err(|_| {
            SimulationExecutionError::new_err("seeded tick simulation request must be a mapping")
        })?;

        Ok(Self {
            seed: parse_required_json_field(request, "seeded_tick_simulation_request", "seed")?,
            start_time_utc: parse_required_json_field(
                request,
                "seeded_tick_simulation_request",
                "start_time_utc",
            )?,
            horizon_interval_count: parse_required_json_field(
                request,
                "seeded_tick_simulation_request",
                "horizon_interval_count",
            )?,
            hourly_factor_contract: parse_required_hourly_factor_contract(
                request,
                "seeded_tick_simulation_request",
            )?,
            model_inputs: SeededTickSimulationModelInputsPayload::from_mapping(request)?,
            run_options: SeededTickSimulationRunOptions {
                debug_trace_enabled: parse_required_json_field(
                    request,
                    "seeded_tick_simulation_request",
                    "debug_trace_enabled",
                )?,
                debug_trace_options: SeededTickSimulationDebugTraceOptions {
                    sample_every_nth_transition: parse_required_json_field(
                        request,
                        "seeded_tick_simulation_request",
                        "sample_every_nth_transition",
                    )?,
                    max_records_per_chunk: parse_required_json_field(
                        request,
                        "seeded_tick_simulation_request",
                        "max_records_per_chunk",
                    )?,
                },
                state_snapshot_capture: parse_seeded_simulation_state_snapshot_capture(
                    parse_required_string_field(
                        request,
                        "seeded_tick_simulation_request",
                        "state_snapshot_capture",
                    )?
                    .as_str(),
                )?,
            },
        })
    }

    pub(super) fn into_run_config(self) -> PyResult<SeededTickSimulationRunConfig> {
        let calendar_registry = self
            .hourly_factor_contract
            .calendar_registry
            .clone()
            .ok_or_else(|| {
                SimulationExecutionError::new_err(
                    "seeded tick-event simulation requires a runtime hourly factor contract with calendar registry state",
                )
            })?;

        SeededTickSimulationRunConfig::try_new(
            self.seed,
            parse_timestamp_utc(self.start_time_utc.as_str())?,
            self.horizon_interval_count,
            SeededTickSimulationModelInputs {
                kernel_parameters: self.model_inputs.kernel_parameters,
                nig_shared_plus_shift_configuration: self
                    .model_inputs
                    .nig_shared_plus_shift_configuration,
                news_process_configuration: self.model_inputs.news_process_configuration,
                initial_excitation_state: self.model_inputs.initial_excitation_state,
                mean_reversion_fitted_parameters: self
                    .model_inputs
                    .mean_reversion_fitted_parameters,
                mean_reversion_runtime_interval_anchors: self
                    .model_inputs
                    .mean_reversion_runtime_interval_anchors,
                initial_mean_reversion_boundary_state: self
                    .model_inputs
                    .initial_mean_reversion_boundary_state,
                hourly_factor_contract: self.hourly_factor_contract.output.clone(),
                return_clock_exposure_parameters: self
                    .model_inputs
                    .return_clock_exposure_parameters,
                observed_price_tick_size: self.model_inputs.observed_price_tick_size,
                calendar_registry,
            },
            Some(self.run_options),
        )
        .map_err(map_seeded_tick_simulation_run_error)
    }
}

struct SeededTickSimulationModelInputsPayload {
    kernel_parameters: janiform::model::hawkes::HawkesKernelParameters,
    nig_shared_plus_shift_configuration: janiform::model::nig::NigSharedPlusShiftConfiguration,
    news_process_configuration: NewsProcessConfiguration,
    initial_excitation_state: janiform::model::hawkes::HawkesExcitationState,
    mean_reversion_fitted_parameters: janiform::simulation::MeanReversionFittedParameters,
    mean_reversion_runtime_interval_anchors:
        janiform::simulation::MeanReversionRuntimeIntervalAnchors,
    initial_mean_reversion_boundary_state: janiform::simulation::MeanReversionBoundaryState,
    return_clock_exposure_parameters: ReturnClockExposureParameters,
    observed_price_tick_size: Option<f64>,
}

impl SeededTickSimulationModelInputsPayload {
    fn from_mapping(request: &Bound<'_, PyMapping>) -> PyResult<Self> {
        Ok(Self {
            kernel_parameters: parse_seeded_simulation_kernel_parameters(&parse_required_field(
                request,
                "seeded_tick_simulation_request",
                "kernel_parameters",
            )?)?,
            nig_shared_plus_shift_configuration:
                parse_seeded_simulation_nig_shared_plus_shift_configuration(&parse_required_field(
                    request,
                    "seeded_tick_simulation_request",
                    "nig_shared_plus_shift_configuration",
                )?)?,
            news_process_configuration: parse_required_json_field::<NewsProcessConfiguration>(
                request,
                "seeded_tick_simulation_request",
                "news_process_configuration",
            )?,
            initial_excitation_state: parse_seeded_simulation_initial_excitation_state(
                &parse_required_field(
                    request,
                    "seeded_tick_simulation_request",
                    "initial_excitation_state",
                )?,
            )?,
            mean_reversion_fitted_parameters:
                parse_seeded_simulation_mean_reversion_fitted_parameters(&parse_required_field(
                    request,
                    "seeded_tick_simulation_request",
                    "mean_reversion_fitted_parameters",
                )?)?,
            mean_reversion_runtime_interval_anchors:
                parse_seeded_simulation_mean_reversion_runtime_interval_anchors(
                    &parse_required_field(
                        request,
                        "seeded_tick_simulation_request",
                        "mean_reversion_runtime_interval_anchors",
                    )?,
                )?,
            initial_mean_reversion_boundary_state:
                parse_seeded_simulation_mean_reversion_boundary_state(&parse_required_field(
                    request,
                    "seeded_tick_simulation_request",
                    "initial_mean_reversion_boundary_state",
                )?)?,
            return_clock_exposure_parameters: parse_required_json_field(
                request,
                "seeded_tick_simulation_request",
                "return_clock_exposure_parameters",
            )?,
            observed_price_tick_size: parse_required_json_field(
                request,
                "seeded_tick_simulation_request",
                "observed_price_tick_size",
            )?,
        })
    }
}

pub(super) fn parse_seeded_simulation_state_snapshot_capture(
    raw_state_snapshot_capture: &str,
) -> PyResult<SeededTickEventStateSnapshotCapture> {
    match raw_state_snapshot_capture {
        "disabled" => Ok(SeededTickEventStateSnapshotCapture::Disabled),
        "post_event_transition" => Ok(SeededTickEventStateSnapshotCapture::PostEventTransition),
        _ => Err(SeasonalityInputValidationError::new_err(format!(
            "state_snapshot_capture must be either 'disabled' or 'post_event_transition', \
             received '{raw_state_snapshot_capture}'"
        ))),
    }
}
