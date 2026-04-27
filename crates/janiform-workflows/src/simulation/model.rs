use std::path::PathBuf;
use std::time::{Duration, Instant};

use chrono::{DateTime, Datelike, Duration as ChronoDuration, Timelike, Utc};
use janiform::affine_oracle::{
    ExponentialAffineTransformEquationSpecification, materialize_endogenous_anchor_path,
};
use janiform::calendar::{ExchangeCalendarRegistry, ExchangeCalendarRegistryArtifact, MarketState};
use janiform::calibration::{
    CurrentStateEstimationInput, estimate_current_state_from_observed_data,
    materialize_runtime_structural_seasonality,
};
use janiform::hawkes::{HawkesEventSide, HawkesExcitationState};
use janiform::hourly_factors::{
    DEFAULT_SEASONALITY_AGGREGATION_METHOD, DEFAULT_SEASONALITY_INTERPOLATION_METHOD,
    DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY, DEFAULT_SPARSE_HOUR_MINIMUM_OBSERVATION_COUNT,
    HourlyFactorContractOutput, HourlyFactorProviderRequest, HourlyNewsIntensityFactorSource,
    HourlySeasonalityFactorSource, ONE_HOUR_NANOS, duration_nanos_to_hours,
};
use janiform::simulation::{
    CoarseSimulationModelInputs, MeanReversionBoundaryState, SeededTickEventStateSnapshotCapture,
    SeededTickSimulationDebugTraceOptions, SeededTickSimulationModelInputs,
    SeededTickSimulationRunOptions,
    materialize_mean_reversion_runtime_interval_anchors_from_core_expected_increments,
};
use janiform::sufficient_statistics::{
    SufficientStatisticsIntervalFactors, SufficientStatisticsModelInputs,
};
use janiform_calibration_artifacts::{
    HawkesStageFittedParameters, ReturnComponentsStageFittedParameters,
};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

use crate::error::WorkflowError;

use super::artifacts::{
    DEFAULT_SIMULATION_EXPORT_BATCH_SIZE, SimulationExportSummary, SimulationOutputColumns,
};
use super::direct::{
    CoarseIntervalExportSettings, DirectSimulationCompleted, DirectSimulationJob,
    SeededTickExportSettings,
};

const MODEL_SCHEMA_VERSION: &str = "calibration.high_level.model.v1";
const DEFAULT_SIMULATION_INTERVAL_DURATION_NANOS: i64 = ONE_HOUR_NANOS;
const RUNTIME_COMPACT_SEASONALITY_MATERIALIZATION_METHOD: &str =
    "runtime_compact_seasonality_parameters_mean_renormalized.v1";
const MEAN_REVERSION_RUNTIME_INTERVAL_ALIGNMENT_POLICY: &str =
    "explicit_runtime_interval_grid_alignment.v1";
const SURPRISE_MULTIPLIER_HOURS_PER_WEEK: usize = 168;
const MINUTES_PER_DAY: usize = 1_440;
const DEFAULT_MEAN_REVERSION_START_PRESSURE_STATE: f64 = 0.0;
const DEFAULT_INITIAL_MEAN_REVERSION_DEVIATION: f64 = 0.0;
const CLOSED_INTERVAL_SEASONALITY_FACTOR: f64 = 0.0;
const DEFAULT_SEEDED_DEBUG_TRACE_MAX_RECORDS_PER_CHUNK: usize = 1024;

/// Complete fitted-model simulation workflow request.
#[derive(Debug, Deserialize)]
pub struct ModelSimulationWorkflowRequest {
    /// Persisted `JaniformModel` payload.
    pub model_payload: Value,
    /// Simulation kind selected by the public API.
    pub simulation_kind: ModelSimulationKind,
    /// Deterministic run seed.
    pub seed: u64,
    /// First simulated interval start.
    #[serde(deserialize_with = "deserialize_utc_datetime")]
    pub start_time_utc: DateTime<Utc>,
    /// Number of simulated intervals.
    pub horizon_interval_count: usize,
    /// Runtime interval width in UTC nanoseconds.
    #[serde(default = "default_simulation_interval_duration_nanos")]
    pub interval_duration_nanos: i64,
    /// Optional artifact path for direct workflow export.
    pub output_path: Option<PathBuf>,
    /// Output column contract.
    pub output_columns: SimulationOutputColumns,
    /// Optional output reference price.
    #[serde(default = "default_reference_price")]
    pub reference_price: f64,
    /// Batch size for exported simulation records.
    #[serde(default = "default_simulation_export_batch_size")]
    pub export_batch_size: usize,
    /// Optional continuation observations used to estimate missing initial state.
    pub continuation_data: Option<ModelSimulationContinuationData>,
    /// Optional explicit initial Hawkes state.
    pub initial_excitation_state: Option<HawkesExcitationState>,
    /// Optional explicit initial mean-reversion boundary state.
    pub initial_mean_reversion_boundary_state: Option<MeanReversionBoundaryState>,
    /// Enable seeded debug trace collection.
    #[serde(default)]
    pub debug_trace_enabled: bool,
    /// Seeded debug trace sampling stride.
    #[serde(default = "default_sample_every_nth_transition")]
    pub sample_every_nth_transition: u64,
    /// Seeded debug trace chunk size.
    #[serde(default = "default_max_records_per_chunk")]
    pub max_records_per_chunk: usize,
    /// Seeded state snapshot capture mode.
    #[serde(default)]
    pub state_snapshot_capture: SeededTickEventStateSnapshotCapture,
}

/// Fitted-model simulation kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ModelSimulationKind {
    /// Seeded tick-event simulation.
    SeededTick,
    /// Coarse interval simulation.
    CoarseInterval,
}

/// Boundary payload for continuation-based current state estimation.
#[derive(Debug, Deserialize)]
pub struct ModelSimulationContinuationData {
    /// Observation timestamp column in UTC nanoseconds.
    pub observation_timestamp_utc_nanos: Vec<i64>,
    /// Observed trade prices.
    pub observation_trade_prices: Vec<f64>,
    /// Observed trade sides encoded as `buy` or `sell`.
    pub observation_trade_sides: Vec<String>,
    /// Optional current spot price used for observed log-price derivation.
    pub observed_price: Option<f64>,
}

/// Completed fitted-model simulation workflow.
pub struct ModelSimulationWorkflowOutput {
    /// Completed native simulation result.
    pub completed: DirectSimulationCompleted,
    /// Optional export summary.
    pub export_summary: Option<SimulationExportSummary>,
    /// Native workflow execution duration.
    pub execution_duration: Duration,
}

struct ModelRuntimeContext {
    model: PersistedModelPayload,
    calendar_registry: ExchangeCalendarRegistry,
    calendar_registry_artifact: ExchangeCalendarRegistryArtifact,
    interval_delta: f64,
    range_end_utc: DateTime<Utc>,
    hawkes_fitted_parameters: HawkesStageFittedParameters,
    return_components_fitted_parameters: ReturnComponentsStageFittedParameters,
}

#[derive(Debug, Deserialize)]
struct PersistedModelPayload {
    schema_version: String,
    parameters: Value,
    metadata: Value,
    resolved_configuration: Value,
    session_resolution: ModelSessionResolutionPayload,
}

#[derive(Debug, Deserialize)]
struct ModelSessionResolutionPayload {
    exchange_code: String,
    resolved_session_definition_json: String,
}

struct ResolvedInitialState {
    excitation_state: HawkesExcitationState,
    mean_reversion_boundary_state: Option<MeanReversionBoundaryState>,
    start_anchor_value: f64,
}

/// Run a fitted-model simulation workflow from a persisted model payload.
pub fn run_model_simulation_workflow(
    request: ModelSimulationWorkflowRequest,
    observer: &mut dyn FnMut(janiform::simulation::SimulationProgressEvent),
) -> Result<ModelSimulationWorkflowOutput, WorkflowError> {
    validate_request(&request)?;

    let started_at = Instant::now();

    let context = runtime_context(&request)?;

    let hourly_factor_contract = build_hourly_factor_contract(&request, &context)?;
    let initial_state = resolve_initial_state(&request, &context)?;
    let job = build_direct_job(&request, &context, hourly_factor_contract, initial_state)?;
    let completed = job.run(observer)?;

    let export_summary = completed.export(request.output_path.as_deref())?;

    Ok(ModelSimulationWorkflowOutput {
        completed,
        export_summary,
        execution_duration: started_at.elapsed(),
    })
}

fn validate_request(request: &ModelSimulationWorkflowRequest) -> Result<(), WorkflowError> {
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
    request: &ModelSimulationWorkflowRequest,
) -> Result<ModelRuntimeContext, WorkflowError> {
    // Intentional copy because the workflow request remains borrowed while the
    // persisted model payload is deserialized into typed runtime context.
    let model: PersistedModelPayload = serde_json::from_value(request.model_payload.clone())
        .map_err(|source| {
            validation_error(
                "model_payload",
                format!("failed to deserialize model: {source}"),
            )
        })?;
    if model.schema_version != MODEL_SCHEMA_VERSION {
        return Err(validation_error(
            "model_payload.schema_version",
            format!("must be `{MODEL_SCHEMA_VERSION}`"),
        ));
    }

    let calendar_registry_artifact: ExchangeCalendarRegistryArtifact = serde_json::from_str(
        &model.session_resolution.resolved_session_definition_json,
    )
    .map_err(|source| {
        validation_error(
            "model_payload.session_resolution.resolved_session_definition_json",
            format!("failed to deserialize calendar registry: {source}"),
        )
    })?;

    // Intentional copy because the registry loader consumes the artifact and
    // coarse simulation requests also own the original registry artifact.
    let calendar_registry = ExchangeCalendarRegistry::from_artifact(
        calendar_registry_artifact.clone(),
    )
    .map_err(|source| WorkflowError::CalendarRegistryLoadFailed {
        message: source.to_string(),
    })?;

    calendar_registry
        .get_required(model.session_resolution.exchange_code.as_str())
        .map_err(|source| {
            validation_error(
                "model_payload.session_resolution.exchange_code",
                source.to_string(),
            )
        })?;

    let hawkes_fitted_parameters = hawkes_fitted_parameters(&model.parameters)?;

    let return_components_fitted_parameters =
        return_components_fitted_parameters(&model.parameters)?;

    let interval_delta = duration_nanos_to_hours(request.interval_duration_nanos);

    let range_end_utc = request
        .start_time_utc
        .checked_add_signed(ChronoDuration::nanoseconds(interval_span_nanos(
            request.interval_duration_nanos,
            request.horizon_interval_count,
        )?))
        .ok_or_else(|| validation_error("range_end_utc", "simulation horizon is out of range"))?;

    Ok(ModelRuntimeContext {
        model,
        calendar_registry,
        calendar_registry_artifact,
        interval_delta,
        range_end_utc,
        hawkes_fitted_parameters,
        return_components_fitted_parameters,
    })
}

fn hawkes_fitted_parameters(
    parameters: &Value,
) -> Result<HawkesStageFittedParameters, WorkflowError> {
    let payload = parameters
        .pointer("/joint_refinement/fitted_parameters/hawkes_fitted_parameters")
        .or_else(|| parameters.pointer("/hawkes/fitted_parameters"))
        .ok_or_else(|| {
            validation_error(
                "model_payload.parameters.hawkes",
                "missing fitted parameters",
            )
        })?;

    // Intentional copy because serde_json deserializes typed contracts from
    // owned Value payloads while model parameters remain available to callers.
    serde_json::from_value(payload.clone()).map_err(|source| {
        validation_error(
            "model_payload.parameters.hawkes.fitted_parameters",
            format!("failed to deserialize Hawkes fitted parameters: {source}"),
        )
    })
}

fn return_components_fitted_parameters(
    parameters: &Value,
) -> Result<ReturnComponentsStageFittedParameters, WorkflowError> {
    let payload = parameters
        .pointer("/joint_refinement/fitted_parameters/return_components_fitted_parameters")
        .or_else(|| parameters.pointer("/return_components/fitted_parameters"))
        .ok_or_else(|| {
            validation_error(
                "model_payload.parameters.return_components",
                "missing fitted parameters",
            )
        })?;

    // Intentional copy because serde_json deserializes typed contracts from
    // owned Value payloads while model parameters remain available to callers.
    serde_json::from_value(payload.clone()).map_err(|source| {
        validation_error(
            "model_payload.parameters.return_components.fitted_parameters",
            format!("failed to deserialize return-components fitted parameters: {source}"),
        )
    })
}

fn build_hourly_factor_contract(
    request: &ModelSimulationWorkflowRequest,
    context: &ModelRuntimeContext,
) -> Result<HourlyFactorContractOutput, WorkflowError> {
    let seasonality_grid = materialize_runtime_seasonality_grid(request, context)?;

    let seasonality_source = hourly_seasonality_source(request, context, &seasonality_grid);

    let known_news_timestamps = known_news_timestamp_utc_nanos(&context.model)?;

    let surprise_multipliers = surprise_deterministic_time_multiplier_by_interval_index(
        request.start_time_utc,
        context.range_end_utc,
        request.interval_duration_nanos,
        context
            .return_components_fitted_parameters
            .surprise_deterministic_intensity_profile
            .weekly_hour_utc_multipliers
            .as_slice(),
    )?;

    let surprise_baseline_intensity = context
        .return_components_fitted_parameters
        .news_process_configuration
        .arrivals
        .surprise
        .baseline_intensity;

    let mut hourly_factor_contract =
        janiform_calibration_runtime::load_hourly_factor_contract_for_calibration(
            &context.calendar_registry,
            HourlyFactorProviderRequest {
                exchange_code: context.model.session_resolution.exchange_code.as_str(),
                range_start_utc: request.start_time_utc,
                range_end_utc: context.range_end_utc,
                interval_duration_nanos: request.interval_duration_nanos,
                seasonality_source: Some(seasonality_source),
                news_intensity_source: Some(HourlyNewsIntensityFactorSource::Materialized {
                    known_scheduled_event_timestamps_utc_nanos: known_news_timestamps.as_slice(),
                    surprise_baseline_intensity,
                    surprise_deterministic_time_multiplier_by_interval_index: surprise_multipliers
                        .as_slice(),
                }),
            },
        )
        .map_err(|source| validation_error("hourly_factor_contract", source.to_string()))?;

    if request.simulation_kind == ModelSimulationKind::CoarseInterval {
        zero_closed_coarse_interval_seasonality_factors(&mut hourly_factor_contract, context)?;
    }

    Ok(hourly_factor_contract)
}

fn materialize_runtime_seasonality_grid(
    request: &ModelSimulationWorkflowRequest,
    context: &ModelRuntimeContext,
) -> Result<(Vec<i64>, Vec<f64>), WorkflowError> {
    let seasonality_parameters = context.model.parameters.get("seasonality").ok_or_else(|| {
        validation_error(
            "model_payload.parameters.seasonality",
            "missing compact seasonality parameters",
        )
    })?;

    let intraday_factor_by_bin = numeric_sequence(
        seasonality_parameters,
        "intraday_factor_by_bin",
        "seasonality.intraday_factor_by_bin",
    )?;

    let day_of_week_factor = numeric_sequence(
        seasonality_parameters,
        "day_of_week_factor",
        "seasonality.day_of_week_factor",
    )?;

    let day_of_year_factor = numeric_sequence(
        seasonality_parameters,
        "day_of_year_factor",
        "seasonality.day_of_year_factor",
    )?;

    let intraday_minutes_per_bin =
        intraday_minutes_per_bin(seasonality_parameters, intraday_factor_by_bin.len())?;

    let output = materialize_runtime_structural_seasonality(
        &context.calendar_registry,
        janiform_calibration_runtime::RuntimeStructuralSeasonalityMaterializationInput {
            // Intentional copy because the materialization input owns its exchange identifier.
            exchange_code: context.model.session_resolution.exchange_code.clone(),
            range_start_utc_nanos: request.start_time_utc.timestamp_nanos_opt().ok_or_else(
                || {
                    validation_error(
                        "start_time_utc",
                        "timestamp is outside the supported nanosecond range",
                    )
                },
            )?,
            range_end_utc_nanos: context.range_end_utc.timestamp_nanos_opt().ok_or_else(|| {
                validation_error(
                    "range_end_utc",
                    "timestamp is outside the supported nanosecond range",
                )
            })?,
            interval_duration_nanos: request.interval_duration_nanos,
            intraday_minutes_per_bin,
            intraday_factor_by_bin,
            day_of_week_factor,
            day_of_year_factor,
        },
    )
    .map_err(|source| validation_error("seasonality", source.to_string()))?;

    Ok((output.interval_start_utc_nanos, output.seasonality_factors))
}

fn hourly_seasonality_source<'a>(
    request: &ModelSimulationWorkflowRequest,
    context: &ModelRuntimeContext,
    seasonality_grid: &'a (Vec<i64>, Vec<f64>),
) -> HourlySeasonalityFactorSource<'a> {
    if seasonality_grid.1.is_empty() {
        return HourlySeasonalityFactorSource::Explicit {
            hourly_factors: seasonality_grid.1.as_slice(),
            materialization_method: RUNTIME_COMPACT_SEASONALITY_MATERIALIZATION_METHOD,
        };
    }

    if request.interval_duration_nanos < ONE_HOUR_NANOS
        || !runtime_interval_grid_is_hour_aligned(request.start_time_utc, context.range_end_utc)
    {
        return HourlySeasonalityFactorSource::Materialized {
            source_timestamp_utc_nanos: seasonality_grid.0.as_slice(),
            source_seasonality_factors: seasonality_grid.1.as_slice(),
            interpolation_method: DEFAULT_SEASONALITY_INTERPOLATION_METHOD,
            aggregation_method: DEFAULT_SEASONALITY_AGGREGATION_METHOD,
            sparse_fallback_policy: DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY,
            sparse_hour_minimum_observation_count: DEFAULT_SPARSE_HOUR_MINIMUM_OBSERVATION_COUNT,
        };
    }

    HourlySeasonalityFactorSource::Explicit {
        hourly_factors: seasonality_grid.1.as_slice(),
        materialization_method: RUNTIME_COMPACT_SEASONALITY_MATERIALIZATION_METHOD,
    }
}

fn resolve_initial_state(
    request: &ModelSimulationWorkflowRequest,
    context: &ModelRuntimeContext,
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
    request: &ModelSimulationWorkflowRequest,
    context: &ModelRuntimeContext,
) -> Result<janiform_calibration_runtime::CurrentStateEstimationOutput, WorkflowError> {
    let continuation_data = request
        .continuation_data
        .as_ref()
        .ok_or_else(|| validation_error("continuation_data", "missing continuation data"))?;

    let observed_price = continuation_data.observed_price.unwrap_or_else(|| {
        continuation_data
            .observation_trade_prices
            .last()
            .copied()
            .unwrap_or(0.0)
    });

    let reference_price = model_reference_price(&context.model)?;

    let observed_log_price = (observed_price / reference_price).ln();

    let observation_trade_sides = continuation_data
        .observation_trade_sides
        .iter()
        .map(|side| HawkesEventSide::try_from(side.as_str()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| {
            validation_error(
                "continuation_data.observation_trade_sides",
                source.to_string(),
            )
        })?;

    let state_time_utc_nanos = request
        .start_time_utc
        .timestamp_nanos_opt()
        .ok_or_else(|| {
            validation_error(
                "start_time_utc",
                "timestamp is outside the supported nanosecond range",
            )
        })?;

    estimate_current_state_from_observed_data(CurrentStateEstimationInput {
        kernel_parameters: context.hawkes_fitted_parameters.kernel_parameters,
        // Intentional copy because current-state estimation owns the fitted
        // payload while the runtime context remains available to request builders.
        mean_reversion_fitted_parameters: context
            .return_components_fitted_parameters
            .mean_reversion_fitted_parameters
            .clone(),
        state_time_utc_nanos,
        observed_log_price,
        // Intentional copies because current-state estimation owns observation
        // columns while the workflow request stays available for error context.
        observation_timestamp_utc_nanos: continuation_data.observation_timestamp_utc_nanos.clone(),
        observation_trade_prices: continuation_data.observation_trade_prices.clone(),
        observation_trade_sides,
    })
    .map_err(|source| validation_error("continuation_data", source.to_string()))
}

fn build_direct_job(
    request: &ModelSimulationWorkflowRequest,
    context: &ModelRuntimeContext,
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
        ModelSimulationKind::SeededTick => {
            let run_config = janiform::simulation::SeededTickSimulationRunConfig::try_new(
                request.seed,
                request.start_time_utc,
                request.horizon_interval_count,
                SeededTickSimulationModelInputs {
                    kernel_parameters: context.hawkes_fitted_parameters.kernel_parameters,
                    nig_shared_plus_shift_configuration: context
                        .return_components_fitted_parameters
                        .nig_shared_plus_shift_configuration,
                    // Intentional copy because seeded simulation owns model inputs
                    // while workflow export settings continue to use the shared context.
                    news_process_configuration: context
                        .return_components_fitted_parameters
                        .news_process_configuration
                        .clone(),
                    initial_excitation_state: initial_state.excitation_state,
                    // Intentional copy because both seeded and coarse request builders borrow the
                    // shared fitted-model context while engine contracts own validated inputs.
                    mean_reversion_fitted_parameters: context
                        .return_components_fitted_parameters
                        .mean_reversion_fitted_parameters
                        .clone(),
                    mean_reversion_runtime_interval_anchors: runtime_interval_anchors,
                    initial_mean_reversion_boundary_state,
                    hourly_factor_contract,
                    // Intentional copy because seeded simulation owns runtime inputs
                    // while export settings continue to use the shared context.
                    calendar_registry: context.calendar_registry.clone(),
                },
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
                run_config,
                export: SeededTickExportSettings {
                    // Intentional copy because export settings own the public
                    // output column contract independent of the borrowed request.
                    columns: request.output_columns.clone(),
                    reference_price: request.reference_price,
                    batch_size: request.export_batch_size,
                },
            })
        }
        ModelSimulationKind::CoarseInterval => {
            let equation_specification = ExponentialAffineTransformEquationSpecification::try_new(
                context.hawkes_fitted_parameters.kernel_parameters,
                context
                    .return_components_fitted_parameters
                    .news_process_configuration
                    .shared_mark_law
                    .gamma_severity_marks,
            )
            .map_err(|source| validation_error("equation_specification", source.to_string()))?;

            let model_inputs = SufficientStatisticsModelInputs::try_new(
                equation_specification,
                context
                    .return_components_fitted_parameters
                    .nig_shared_plus_shift_configuration,
                initial_state.excitation_state,
                context
                    .return_components_fitted_parameters
                    // Intentional copy because the sufficient-statistics contract owns
                    // validated fitted parameters independent of the shared context.
                    .mean_reversion_fitted_parameters
                    .clone(),
                runtime_interval_anchors,
                initial_mean_reversion_boundary_state,
            )
            .map_err(|source| {
                validation_error("sufficient_statistics_model_inputs", source.to_string())
            })?;

            let coarse_model_inputs = CoarseSimulationModelInputs::try_new(
                model_inputs,
                // Intentional copy because the coarse simulation contract owns model inputs
                // while export settings still borrow the shared workflow context.
                context
                    .return_components_fitted_parameters
                    .news_process_configuration
                    .clone(),
                // Intentional copies because coarse simulation owns serialized
                // calendar identity independent of the shared workflow context.
                context.calendar_registry_artifact.clone(),
                context.model.session_resolution.exchange_code.clone(),
            )
            .map_err(|source| validation_error("coarse_model_inputs", source.to_string()))?;

            let s_h = hourly_factor_contract.s_h;

            let known_component_nu_h = hourly_factor_contract.known_component_nu_h;
            let surprise_component_nu_h = hourly_factor_contract.surprise_component_nu_h;

            let interval_factors = SufficientStatisticsIntervalFactors::try_new(
                s_h,
                known_component_nu_h,
                surprise_component_nu_h,
                Some(context.interval_delta),
            )
            .map_err(|source| validation_error("interval_factors", source.to_string()))?;

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
                request: coarse_request,
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
    request: &ModelSimulationWorkflowRequest,
    context: &ModelRuntimeContext,
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
    let interval_factors =
        janiform_calibration_runtime::endogenous_interval_factors_from_hourly_contract(
            hourly_factor_contract,
        )
        .map_err(|source| validation_error("hourly_factor_contract", source))?;

    let anchor_materialization = materialize_endogenous_anchor_path(
        MEAN_REVERSION_RUNTIME_INTERVAL_ALIGNMENT_POLICY,
        start_anchor_value,
        ExponentialAffineTransformEquationSpecification::try_new(
            context.hawkes_fitted_parameters.kernel_parameters,
            context
                .return_components_fitted_parameters
                .news_process_configuration
                .shared_mark_law
                .gamma_severity_marks,
        )
        .map_err(|source| validation_error("equation_specification", source.to_string()))?,
        context
            .return_components_fitted_parameters
            .nig_shared_plus_shift_configuration,
        &context
            .return_components_fitted_parameters
            .news_process_configuration,
        initial_excitation_state,
        interval_factors.as_slice(),
    )
    .map_err(|source| validation_error("mean_reversion_runtime", source.to_string()))?;

    let core_expected_increments = anchor_materialization
        .interval_expectations
        .iter()
        .map(|expectation| expectation.core_expected_increment)
        .collect::<Vec<_>>();

    let interval_deltas = vec![context.interval_delta; request.horizon_interval_count];

    let runtime_interval_anchors =
        materialize_mean_reversion_runtime_interval_anchors_from_core_expected_increments(
            MEAN_REVERSION_RUNTIME_INTERVAL_ALIGNMENT_POLICY,
            start_anchor_value,
            DEFAULT_MEAN_REVERSION_START_PRESSURE_STATE,
            &context
                .return_components_fitted_parameters
                .mean_reversion_fitted_parameters
                .anchor_model,
            core_expected_increments.as_slice(),
            interval_deltas.as_slice(),
        )
        .map_err(|source| validation_error("mean_reversion_runtime", source.to_string()))?;

    let initial_boundary_state = match MeanReversionBoundaryState::try_new(
        runtime_interval_anchors.start_anchor_value,
        runtime_interval_anchors.start_anchor_value,
        runtime_interval_anchors.start_pressure_state,
        DEFAULT_INITIAL_MEAN_REVERSION_DEVIATION,
    ) {
        Ok(boundary_state) => boundary_state,
        Err(source) => {
            return Err(validation_error(
                "mean_reversion_boundary_state",
                source.to_string(),
            ));
        }
    };

    Ok((runtime_interval_anchors, initial_boundary_state))
}

fn zero_closed_coarse_interval_seasonality_factors(
    hourly_factor_contract: &mut HourlyFactorContractOutput,
    context: &ModelRuntimeContext,
) -> Result<(), WorkflowError> {
    if hourly_factor_contract.interval_duration_nanos >= ONE_HOUR_NANOS {
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

fn numeric_sequence(
    payload: &Value,
    field_name: &str,
    field_path: &'static str,
) -> Result<Vec<f64>, WorkflowError> {
    let values = payload
        .get(field_name)
        .and_then(Value::as_array)
        .ok_or_else(|| validation_error(field_path, "must be a sequence"))?;
    if values.is_empty() {
        return Err(validation_error(field_path, "must not be empty"));
    }

    values
        .iter()
        .enumerate()
        .map(|(value_index, value)| {
            value.as_f64().ok_or_else(|| {
                validation_error(
                    field_path,
                    format!("entry at position {value_index} must be numeric"),
                )
            })
        })
        .collect()
}

fn intraday_minutes_per_bin(
    seasonality_parameters: &Value,
    intraday_factor_count: usize,
) -> Result<u16, WorkflowError> {
    if let Some(raw_value) = seasonality_parameters
        .get("intraday_minutes_per_bin")
        .and_then(Value::as_u64)
    {
        return u16::try_from(raw_value).map_err(|_| {
            validation_error(
                "seasonality.intraday_minutes_per_bin",
                "must fit in an unsigned 32-bit integer",
            )
        });
    }

    let (minutes_per_bin, remainder) = (
        MINUTES_PER_DAY / intraday_factor_count,
        MINUTES_PER_DAY % intraday_factor_count,
    );
    if remainder != 0 {
        return Err(validation_error(
            "seasonality.intraday_minutes_per_bin",
            "must be present when intraday_factor_by_bin does not evenly partition a day",
        ));
    }

    u16::try_from(minutes_per_bin).map_err(|_| {
        validation_error(
            "seasonality.intraday_minutes_per_bin",
            "must fit in an unsigned 32-bit integer",
        )
    })
}

fn known_news_timestamp_utc_nanos(
    model: &PersistedModelPayload,
) -> Result<Vec<i64>, WorkflowError> {
    let Some(events) = model
        .resolved_configuration
        .pointer("/known_news/configuration/events")
        .and_then(Value::as_array)
    else {
        return Ok(Vec::new());
    };

    events
        .iter()
        .enumerate()
        .map(|(event_index, event)| {
            let scheduled_at = event
                .get("scheduled_at")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    validation_error(
                        "model_payload.resolved_configuration.known_news.configuration.events",
                        format!("event at position {event_index} is missing scheduled_at"),
                    )
                })?;

            let timestamp = DateTime::parse_from_rfc3339(scheduled_at).map_err(|source| {
                validation_error(
                    "model_payload.resolved_configuration.known_news.configuration.events.scheduled_at",
                    source.to_string(),
                )
            })?;

            timestamp.with_timezone(&Utc).timestamp_nanos_opt().ok_or_else(|| {
                validation_error(
                    "model_payload.resolved_configuration.known_news.configuration.events.scheduled_at",
                    "timestamp is outside the supported nanosecond range",
                )
            })
        })
        .collect()
}

fn surprise_deterministic_time_multiplier_by_interval_index(
    range_start_utc: DateTime<Utc>,
    range_end_utc: DateTime<Utc>,
    interval_duration_nanos: i64,
    weekly_hour_utc_multipliers: &[f64],
) -> Result<Vec<(u64, f64)>, WorkflowError> {
    if weekly_hour_utc_multipliers.len() != SURPRISE_MULTIPLIER_HOURS_PER_WEEK {
        return Err(validation_error(
            "surprise_deterministic_intensity_profile.weekly_hour_utc_multipliers",
            "must contain exactly 168 hourly multipliers",
        ));
    }
    if range_end_utc <= range_start_utc {
        return Ok(Vec::new());
    }

    let interval_count_nanos = range_end_utc.timestamp_nanos_opt().ok_or_else(|| {
        validation_error(
            "range_end_utc",
            "timestamp is outside the supported nanosecond range",
        )
    })? - range_start_utc.timestamp_nanos_opt().ok_or_else(|| {
        validation_error(
            "range_start_utc",
            "timestamp is outside the supported nanosecond range",
        )
    })?;

    let interval_count =
        usize::try_from(interval_count_nanos / interval_duration_nanos).map_err(|_| {
            validation_error(
                "interval_duration_nanos",
                "runtime interval count does not fit in memory on this platform",
            )
        })?;

    let mut multiplier_by_interval = Vec::new();

    for interval_index in 0..interval_count {
        let interval_start_utc = range_start_utc
            .checked_add_signed(ChronoDuration::nanoseconds(interval_span_nanos(
                interval_duration_nanos,
                interval_index,
            )?))
            .ok_or_else(|| {
                validation_error(
                    "interval_duration_nanos",
                    "interval timestamp is out of range",
                )
            })?;

        let interval_end_utc = interval_start_utc
            .checked_add_signed(ChronoDuration::nanoseconds(interval_duration_nanos))
            .map(|timestamp| timestamp.min(range_end_utc))
            .ok_or_else(|| {
                validation_error(
                    "interval_duration_nanos",
                    "interval timestamp is out of range",
                )
            })?;

        let multiplier = average_weekly_hour_multiplier_over_interval(
            interval_start_utc,
            interval_end_utc,
            weekly_hour_utc_multipliers,
        )?;

        let interval_index = u64::try_from(interval_index).map_err(|_| {
            validation_error(
                "interval_duration_nanos",
                "runtime interval index does not fit in u64",
            )
        })?;

        multiplier_by_interval.push((interval_index, multiplier));
    }

    Ok(multiplier_by_interval)
}

fn average_weekly_hour_multiplier_over_interval(
    interval_start_utc: DateTime<Utc>,
    interval_end_utc: DateTime<Utc>,
    weekly_hour_utc_multipliers: &[f64],
) -> Result<f64, WorkflowError> {
    if interval_end_utc <= interval_start_utc {
        return Ok(1.0);
    }

    let mut weighted_multiplier_sum = 0.0;
    let mut weighted_duration_seconds = 0.0;
    let mut current = interval_start_utc;

    while current < interval_end_utc {
        let hour_start_utc = current
            .with_minute(0)
            .and_then(|timestamp| timestamp.with_second(0))
            .and_then(|timestamp| timestamp.with_nanosecond(0))
            .ok_or_else(|| {
                validation_error(
                    "surprise_multiplier_interval",
                    "failed to derive UTC hour boundary",
                )
            })?;

        let next_hour_boundary_utc = if current == hour_start_utc {
            current + ChronoDuration::hours(1)
        } else {
            hour_start_utc + ChronoDuration::hours(1)
        };

        let segment_end_utc = next_hour_boundary_utc.min(interval_end_utc);

        let segment_duration_seconds = (segment_end_utc - current)
            .to_std()
            .map_err(|source| validation_error("surprise_multiplier_interval", source.to_string()))?
            .as_secs_f64();

        weighted_multiplier_sum += weekly_hour_utc_multipliers[weekly_hour_bucket_index(current)?]
            * segment_duration_seconds;
        weighted_duration_seconds += segment_duration_seconds;

        current = segment_end_utc;
    }

    if weighted_duration_seconds <= 0.0 {
        return Ok(1.0);
    }

    Ok(weighted_multiplier_sum / weighted_duration_seconds)
}

fn weekly_hour_bucket_index(timestamp_utc: DateTime<Utc>) -> Result<usize, WorkflowError> {
    let weekday_index =
        usize::try_from(timestamp_utc.weekday().num_days_from_monday()).map_err(|_| {
            validation_error(
                "surprise_multiplier_interval",
                "weekday index does not fit in usize",
            )
        })?;

    let hour_index = usize::try_from(timestamp_utc.hour()).map_err(|_| {
        validation_error(
            "surprise_multiplier_interval",
            "hour index does not fit in usize",
        )
    })?;

    Ok(weekday_index * 24 + hour_index)
}

fn runtime_interval_grid_is_hour_aligned(
    range_start_utc: DateTime<Utc>,
    range_end_utc: DateTime<Utc>,
) -> bool {
    let Some(range_start_utc_nanos) = range_start_utc.timestamp_nanos_opt() else {
        return false;
    };

    let Some(range_end_utc_nanos) = range_end_utc.timestamp_nanos_opt() else {
        return false;
    };

    range_start_utc_nanos % ONE_HOUR_NANOS == 0 && range_end_utc_nanos % ONE_HOUR_NANOS == 0
}

fn interval_span_nanos(
    interval_duration_nanos: i64,
    interval_count: usize,
) -> Result<i64, WorkflowError> {
    let interval_count = i64::try_from(interval_count).map_err(|_| {
        validation_error(
            "interval_duration_nanos",
            "interval count does not fit in signed 64-bit arithmetic",
        )
    })?;

    interval_duration_nanos
        .checked_mul(interval_count)
        .ok_or_else(|| {
            validation_error(
                "interval_duration_nanos",
                "interval span overflowed signed 64-bit nanosecond arithmetic",
            )
        })
}

fn model_reference_price(model: &PersistedModelPayload) -> Result<f64, WorkflowError> {
    let reference_price = model
        .metadata
        .pointer("/simulation_output_preferences/reference_price")
        .and_then(Value::as_f64)
        .ok_or_else(|| {
            validation_error(
                "model_payload.metadata.simulation_output_preferences.reference_price",
                "must be present so continuation state can be derived from spot",
            )
        })?;

    if !reference_price.is_finite() || reference_price <= 0.0 {
        return Err(validation_error(
            "model_payload.metadata.simulation_output_preferences.reference_price",
            "must be finite and strictly positive",
        ));
    }

    Ok(reference_price)
}

impl ModelSimulationWorkflowOutput {
    /// Serialize the public workflow metadata fields.
    pub fn response_payload(&self) -> Result<Value, WorkflowError> {
        let completion = self.completed.completion();

        let simulation_kind = match &self.completed {
            DirectSimulationCompleted::SeededTick { .. } => "seeded_tick",
            DirectSimulationCompleted::CoarseInterval { .. } => "coarse_interval",
        };

        let export_summary = match &self.export_summary {
            Some(summary) => Some(json!({
                "record_count": summary.record_count,
                "chunk_count": summary.chunk_count,
            })),
            None => None,
        };

        Ok(json!({
            "simulation_kind": simulation_kind,
            "completion": {
                "seed": completion.seed,
                "simulated_interval_count": completion.simulated_interval_count,
            },
            "export_summary": export_summary,
            "execution_duration_seconds": self.execution_duration.as_secs_f64(),
        }))
    }
}

fn default_simulation_interval_duration_nanos() -> i64 {
    DEFAULT_SIMULATION_INTERVAL_DURATION_NANOS
}

fn default_reference_price() -> f64 {
    1.0
}

fn default_simulation_export_batch_size() -> usize {
    DEFAULT_SIMULATION_EXPORT_BATCH_SIZE
}

fn default_sample_every_nth_transition() -> u64 {
    1
}

fn default_max_records_per_chunk() -> usize {
    DEFAULT_SEEDED_DEBUG_TRACE_MAX_RECORDS_PER_CHUNK
}

fn validation_error(field: &'static str, message: impl Into<String>) -> WorkflowError {
    WorkflowError::SimulationValidationFailed {
        field,
        message: message.into(),
    }
}

fn deserialize_utc_datetime<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    DateTime::parse_from_rfc3339(value.as_str())
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(serde::de::Error::custom)
}
