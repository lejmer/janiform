use janiform::affine_oracle::{
    ExponentialAffineTransformEquationSpecification, materialize_endogenous_anchor_path,
};
use janiform::hawkes::HawkesExcitationState;
use janiform::hourly_factors::HourlyFactorContractOutput;
use janiform::simulation::{
    CoarseSimulationModelInputs, MeanReversionBoundaryState, SeededTickSimulationDebugTraceOptions,
    SeededTickSimulationModelInputs, SeededTickSimulationRunOptions,
    materialize_mean_reversion_runtime_interval_anchors_from_core_expected_increments,
};
use janiform::sufficient_statistics::{
    SufficientStatisticsIntervalFactors, SufficientStatisticsModelInputs,
};

use crate::error::WorkflowError;

use super::context::ModelRuntimeContext;
use super::state::ResolvedInitialState;
use super::support::validation_error;
use super::{ModelSimulationKind, ModelSimulationWorkflowRequest};
use crate::simulation::direct::{
    CoarseIntervalExportSettings, DirectSimulationJob, SeededTickExportSettings,
};

const MEAN_REVERSION_RUNTIME_INTERVAL_ALIGNMENT_POLICY: &str =
    "explicit_runtime_interval_grid_alignment.v1";
const DEFAULT_MEAN_REVERSION_START_PRESSURE_STATE: f64 = 0.0;
const DEFAULT_INITIAL_MEAN_REVERSION_DEVIATION: f64 = 0.0;

pub(super) fn build_direct_job(
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
                    // Intentional copy because seeded simulation owns a validated
                    // model input independent of the shared fitted-model context.
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
                // Intentional copy because the sufficient-statistics contract owns
                // validated fitted parameters independent of the shared context.
                context
                    .return_components_fitted_parameters
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

            let interval_factors = SufficientStatisticsIntervalFactors::try_new(
                hourly_factor_contract.s_h,
                hourly_factor_contract.known_component_nu_h,
                hourly_factor_contract.surprise_component_nu_h,
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
