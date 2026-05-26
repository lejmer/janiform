use std::time::Instant;

use chrono::{DateTime, Utc};
use janiform::model::factors::hourly::{HourlyFactorContractOutput, NANOS_PER_HOUR};
use janiform::model::hawkes::HawkesExcitationState;
use janiform::simulation::{
    MeanReversionBoundaryState, SeededTickSimulationDebugTraceOptions,
    SeededTickSimulationRunOptions, SimulationProgressEvent,
};
use janiform::time::calendar::MarketState;

use crate::error::WorkflowError;
use crate::model::{
    FittedModelContext, ModelStateObservationData, build_coarse_simulation_model_inputs,
    build_seeded_tick_simulation_model_inputs, build_sufficient_statistics_interval_factors,
    build_sufficient_statistics_model_inputs, estimate_current_state_from_observation_data,
    fitted_model_context,
};
use crate::simulation::direct::{
    CoarseIntervalExportSettings, DirectSimulationJob, SeededTickExportSettings,
};

use super::artifacts::write_model_artifact_simulation_sidecars;
use super::output::ModelArtifactSimulationWorkflowOutput;
use super::request::{ModelArtifactSimulationKind, ModelArtifactSimulationWorkflowRequest};

const CLOSED_INTERVAL_SEASONALITY_FACTOR: f64 = 0.0;

/// Run a simulation workflow from a persisted model artifact payload.
pub fn run_simulation_from_model_artifact(
    request: ModelArtifactSimulationWorkflowRequest,
    observer: &mut dyn FnMut(SimulationProgressEvent),
) -> Result<ModelArtifactSimulationWorkflowOutput, WorkflowError> {
    validate_request(&request)?;

    let started_at = Instant::now();

    let context = runtime_context(&request)?;
    let hourly_factor_contract = build_hourly_factor_contract(&request, &context)?;
    let initial_state = resolve_initial_state(&request, &context)?;

    let job = build_direct_job(&request, &context, hourly_factor_contract, initial_state)?;
    let completed = job.run(observer)?;

    let export_summary = completed.export(request.output_path.as_deref())?;

    let output = ModelArtifactSimulationWorkflowOutput {
        completed,
        export_summary,
        execution_duration: started_at.elapsed(),
    };

    if let Some(output_path) = request.output_path.as_deref() {
        write_model_artifact_simulation_sidecars(
            request.run_id.as_deref().unwrap_or("simulation"),
            context.model.run_id.as_str(),
            output_path,
            &output.response_payload()?,
        )?;
    }

    Ok(output)
}

fn validate_request(request: &ModelArtifactSimulationWorkflowRequest) -> Result<(), WorkflowError> {
    if request.horizon_interval_count == 0 {
        return Err(validation_error(
            "horizon_interval_count",
            "must be a positive integer",
        ));
    }

    if request.interval_duration_nanos <= 0 {
        return Err(validation_error(
            "interval_duration_nanos",
            "must be a positive integer number of nanoseconds",
        ));
    }

    if !request.reference_price.is_finite() || request.reference_price <= 0.0 {
        return Err(validation_error(
            "reference_price",
            "must be finite and strictly positive",
        ));
    }

    if request.export_batch_size == 0 {
        return Err(validation_error("export_batch_size", "must be positive"));
    }

    Ok(())
}

fn runtime_context(
    request: &ModelArtifactSimulationWorkflowRequest,
) -> Result<FittedModelContext, WorkflowError> {
    fitted_model_context(
        &request.model_payload,
        request.start_time_utc,
        request.interval_duration_nanos,
        request.horizon_interval_count,
    )
}

fn build_hourly_factor_contract(
    request: &ModelArtifactSimulationWorkflowRequest,
    context: &FittedModelContext,
) -> Result<HourlyFactorContractOutput, WorkflowError> {
    let mut hourly_factor_contract = crate::model::build_hourly_factor_contract(
        context,
        request.start_time_utc,
        context.range_end_utc,
        request.interval_duration_nanos,
    )?;

    if request.simulation_kind == ModelArtifactSimulationKind::CoarseInterval {
        zero_closed_coarse_interval_seasonality_factors(&mut hourly_factor_contract, context)?;
    }

    Ok(hourly_factor_contract)
}

fn zero_closed_coarse_interval_seasonality_factors(
    hourly_factor_contract: &mut HourlyFactorContractOutput,
    context: &FittedModelContext,
) -> Result<(), WorkflowError> {
    if hourly_factor_contract.interval_duration_nanos >= NANOS_PER_HOUR {
        return Ok(());
    }

    for (interval_start_utc_nanos, seasonality_factor) in hourly_factor_contract
        .interval_start_utc_nanos
        .iter()
        .zip(hourly_factor_contract.s_h.iter_mut())
    {
        let timestamp_utc = DateTime::<Utc>::from_timestamp_nanos(*interval_start_utc_nanos);

        let market_state = context
            .calendar_registry
            .resolve_market_state_at_utc(
                context.model.session_resolution.exchange_code.as_str(),
                timestamp_utc,
            )
            .map_err(|source| validation_error("calendar_registry", source.to_string()))?;

        if market_state == MarketState::Closed {
            *seasonality_factor = CLOSED_INTERVAL_SEASONALITY_FACTOR;
        }
    }

    Ok(())
}

struct ResolvedInitialState {
    excitation_state: HawkesExcitationState,
    mean_reversion_boundary_state: Option<MeanReversionBoundaryState>,
    start_anchor_value: f64,
}

fn resolve_initial_state(
    request: &ModelArtifactSimulationWorkflowRequest,
    context: &FittedModelContext,
) -> Result<ResolvedInitialState, WorkflowError> {
    let estimated_state = if request.continuation_data.is_some()
        && (request.initial_excitation_state.is_none()
            || request.initial_mean_reversion_boundary_state.is_none())
    {
        Some(estimate_current_state(request, context)?)
    } else {
        None
    };

    let excitation_state = match (request.initial_excitation_state, estimated_state.as_ref()) {
        (Some(excitation_state), _) => excitation_state,
        (None, Some(estimated_state)) => estimated_state.hawkes_excitation_state,
        (None, None) => context
            .fitted_parameters
            .hawkes_fitted_parameters
            .kernel_parameters
            .stationary_excitation_state()
            .map_err(|source| validation_error("kernel_parameters", source.to_string()))?,
    };

    // Intentional copy because the resolved state is stored and also inspected
    // for the runtime anchor start value.
    let mean_reversion_boundary_state = request
        .initial_mean_reversion_boundary_state
        .clone()
        .or_else(|| estimated_state.map(|state| state.mean_reversion_boundary_state));

    let start_anchor_value = mean_reversion_boundary_state
        .as_ref()
        .map(|boundary_state| boundary_state.anchor_value)
        .unwrap_or(0.0);

    Ok(ResolvedInitialState {
        excitation_state,
        mean_reversion_boundary_state,
        start_anchor_value,
    })
}

fn estimate_current_state(
    request: &ModelArtifactSimulationWorkflowRequest,
    context: &FittedModelContext,
) -> Result<janiform_calibration_runtime::CurrentStateEstimationOutput, WorkflowError> {
    let continuation_data = request
        .continuation_data
        .as_ref()
        .ok_or_else(|| validation_error("continuation_data", "missing continuation data"))?;

    estimate_current_state_from_observation_data(
        context,
        request.start_time_utc,
        &ModelStateObservationData {
            // Intentional copies because the shared model-state estimator owns
            // observation columns while the workflow request remains borrowed.
            observation_timestamp_utc_nanos: continuation_data
                .observation_timestamp_utc_nanos
                .clone(),
            observation_trade_prices: continuation_data.observation_trade_prices.clone(),
            observation_trade_sides: continuation_data.observation_trade_sides.clone(),
            observed_price: continuation_data.observed_price,
        },
        None,
    )
    .map_err(|source| validation_error("continuation_data", source.to_string()))
}

fn build_direct_job(
    request: &ModelArtifactSimulationWorkflowRequest,
    context: &FittedModelContext,
    hourly_factor_contract: HourlyFactorContractOutput,
    initial_state: ResolvedInitialState,
) -> Result<DirectSimulationJob, WorkflowError> {
    let (runtime_interval_anchors, derived_boundary_state) =
        materialize_mean_reversion_runtime_inputs(
            request,
            context,
            &hourly_factor_contract,
            initial_state.excitation_state,
            initial_state.start_anchor_value,
        )?;

    let initial_mean_reversion_boundary_state = initial_state
        .mean_reversion_boundary_state
        .unwrap_or(derived_boundary_state);

    match request.simulation_kind {
        ModelArtifactSimulationKind::SeededTick => {
            let model_inputs = build_seeded_tick_simulation_model_inputs(
                context,
                initial_state.excitation_state,
                runtime_interval_anchors,
                initial_mean_reversion_boundary_state,
                hourly_factor_contract,
            );

            let run_config = janiform::simulation::SeededTickSimulationRunConfig::try_new(
                request.seed,
                request.start_time_utc,
                request.horizon_interval_count,
                model_inputs,
                Some(SeededTickSimulationRunOptions {
                    debug_trace_enabled: request.debug_trace_enabled,
                    debug_trace_options: SeededTickSimulationDebugTraceOptions {
                        sample_every_nth_transition: request.sample_every_nth_transition,
                        max_records_per_chunk: request.max_records_per_chunk,
                    },
                    state_snapshot_capture: request.state_snapshot_capture,
                }),
            )
            .map_err(|source| validation_error("seeded_tick_request", source.to_string()))?;

            Ok(DirectSimulationJob::SeededTick {
                run_config: Box::new(run_config),
                export: SeededTickExportSettings {
                    // Intentional copy because export settings own the public
                    // output column contract independent of the borrowed request.
                    columns: request.output_columns.clone(),
                    reference_price: request.reference_price,
                    batch_size: request.export_batch_size,
                },
            })
        }
        ModelArtifactSimulationKind::CoarseInterval => {
            let model_inputs = build_sufficient_statistics_model_inputs(
                context,
                initial_state.excitation_state,
                runtime_interval_anchors,
                initial_mean_reversion_boundary_state,
            )?;

            let coarse_model_inputs = build_coarse_simulation_model_inputs(context, model_inputs)?;

            let interval_factors =
                build_sufficient_statistics_interval_factors(context, hourly_factor_contract)?;

            let start_time_utc_nanos =
                request
                    .start_time_utc
                    .timestamp_nanos_opt()
                    .ok_or_else(|| {
                        validation_error(
                            "start_time_utc",
                            "timestamp is outside the supported nanosecond range",
                        )
                    })?;

            let coarse_request = janiform::simulation::CoarseSimulationRunRequest::try_new(
                request.seed,
                start_time_utc_nanos,
                request.horizon_interval_count,
                context.interval_delta,
                coarse_model_inputs,
                interval_factors,
            )
            .map_err(|source| validation_error("coarse_interval_request", source.to_string()))?;

            Ok(DirectSimulationJob::CoarseInterval {
                request: Box::new(coarse_request),
                export: CoarseIntervalExportSettings {
                    // Intentional copies because export settings own their column
                    // and calendar contracts after request construction completes.
                    columns: request.output_columns.clone(),
                    calendar_registry: context.calendar_registry.clone(),
                    exchange_code: context.model.session_resolution.exchange_code.clone(),
                    reference_price: request.reference_price,
                    batch_size: request.export_batch_size,
                },
            })
        }
    }
}

fn materialize_mean_reversion_runtime_inputs(
    request: &ModelArtifactSimulationWorkflowRequest,
    context: &FittedModelContext,
    hourly_factor_contract: &HourlyFactorContractOutput,
    initial_excitation_state: HawkesExcitationState,
    start_anchor_value: f64,
) -> Result<
    (
        janiform::simulation::MeanReversionRuntimeIntervalAnchors,
        MeanReversionBoundaryState,
    ),
    WorkflowError,
> {
    crate::model::materialize_mean_reversion_runtime_inputs(
        context,
        hourly_factor_contract,
        initial_excitation_state,
        start_anchor_value,
        context.interval_delta,
        request.horizon_interval_count,
    )
}

fn validation_error(field: &'static str, message: impl Into<String>) -> WorkflowError {
    WorkflowError::SimulationValidationFailed {
        field,
        message: message.into(),
    }
}
