use std::collections::BTreeMap;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use janiform_calibration_artifacts::{CalibrationRunSpec, ReturnComponentsStageFittedParameters};
use janiform_foundations::cancellation::CancellationToken;
use janiform_foundations::time::NANOS_PER_HOUR;
use janiform_market_time::calendar::ExchangeCalendarRegistry;

use crate::stages::joint_metrics::build_continuous_calibration_sample_from_aggregated_intervals;
use crate::stages::joint_refinement::contracts::{
    JointRefinementStageExecutionFailure, JointRefinementStageExecutionInput,
    JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate,
};
use crate::stages::parallelism::build_stage_thread_pool;
use crate::stages::seasonality::{
    RuntimeIntervalSeasonalityFactorsBuildInput,
    build_runtime_interval_seasonality_factors_from_components,
};

use super::super::super::super::setup::{
    ParameterBoundsBuildInput, build_active_parameter_layout, build_baseline_parameter_vector,
    build_parameter_bounds, checked_parameter_vector_to_fitted_parameters,
    selected_hawkes_observed_open_event_rate_per_hour, selected_return_components_candidate,
};
use super::super::super::super::summary_simulation::JointRefinementSummarySimulationSessionContext;
use super::super::super::state::JointRefinementOptionPricingContext;
use super::super::super::{
    JointRefinementActiveParameterLayout, JointRefinementContext, JointRefinementObservedData,
    JointRefinementObservedDataBuildInput, JointRefinementProfilingState, build_observed_data,
    build_option_pricing_context, build_summary_simulation_session_context,
};
use super::super::infrastructure::build_summary_simulation_scheduler;
use super::super::progress::run_single_pass_joint_refinement_progress_phase;
use super::super::types::{
    PreparedBootstrapContext, PreparedObservedInputs, PreparedParameterization,
    ResolvedStagePayloads,
};
use super::return_components_fitted_parameters_with_mean_reversion;

const PREPARATION_OBSERVED_INPUTS_LABEL: &str = "prepare observed inputs";
const PREPARATION_PARAMETERIZATION_LABEL: &str = "prepare parameterization";
const PREPARATION_RUNTIME_MATERIALIZATION_LABEL: &str = "materialize runtime state";
const PREPARATION_RUNTIME_CONTEXT_LABEL: &str = "assemble runtime context";

pub(in crate::stages::joint_refinement::execution::pipeline) fn prepare_bootstrap_context(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: &JointRefinementStageExecutionInput,
    resolved_stage_payloads: &ResolvedStagePayloads<'_>,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    cancellation_token: CancellationToken,
) -> Result<PreparedBootstrapContext, JointRefinementStageExecutionFailure> {
    let PreparedObservedInputs {
        observed_data,
        baseline_15m_calibration_sample,
    } = run_single_pass_joint_refinement_progress_phase(
        progress_sender,
        JointRefinementStageProgressPhase::ObservedInputPreparation,
        PREPARATION_OBSERVED_INPUTS_LABEL,
        || {
            prepare_observed_inputs(
                calendar_registry,
                run_spec,
                stage_input,
                resolved_stage_payloads,
            )
        },
    )?;

    let PreparedParameterization {
        active_parameter_layout,
        baseline_parameter_vector,
        lower_bounds,
        upper_bounds,
        option_pricing_refinement_enabled,
        option_pricing_context,
    } = run_single_pass_joint_refinement_progress_phase(
        progress_sender,
        JointRefinementStageProgressPhase::ParameterizationPreparation,
        PREPARATION_PARAMETERIZATION_LABEL,
        || {
            prepare_parameterization(
                run_spec,
                calendar_registry,
                stage_input,
                resolved_stage_payloads,
                &baseline_15m_calibration_sample,
                &observed_data,
            )
        },
    )?;

    let baseline_return_components_fitted_parameters =
        return_components_fitted_parameters_with_mean_reversion(
            resolved_stage_payloads.return_components_payload,
            resolved_stage_payloads.mean_reversion_payload,
        );

    let runtime_materialization = run_single_pass_joint_refinement_progress_phase(
        progress_sender,
        JointRefinementStageProgressPhase::RuntimeMaterialization,
        PREPARATION_RUNTIME_MATERIALIZATION_LABEL,
        || {
            materialize_runtime_state(
                run_spec,
                calendar_registry,
                stage_input,
                &observed_data,
                &baseline_return_components_fitted_parameters,
            )
        },
    )?;

    let bootstrap_context = run_single_pass_joint_refinement_progress_phase(
        progress_sender,
        JointRefinementStageProgressPhase::RuntimeContextAssembly,
        PREPARATION_RUNTIME_CONTEXT_LABEL,
        || {
            let bootstrap_context = build_bootstrap_context(BootstrapContextBuildInput {
                run_spec,
                calendar_registry,
                resolved_stage_payloads,
                observed_data,
                active_parameter_layout,
                lower_bounds,
                upper_bounds,
                option_pricing_refinement_enabled,
                option_pricing_context,
                runtime_materialization,
                // Intentional copy so the joint-refinement context can materialize interval grids after stage input borrows end.
                seasonality_clock_model: stage_input.seasonality_factors_input.clock_model.clone(),
                baseline_return_components_fitted_parameters,
                cancellation_token: cancellation_token.shared_handle(),
            })?;

            validate_baseline_parameter_round_trip(
                baseline_parameter_vector.as_slice(),
                &bootstrap_context,
            )?;

            Ok(bootstrap_context)
        },
    )?;

    Ok(PreparedBootstrapContext {
        bootstrap_context,
        baseline_parameter_vector,
        baseline_15m_calibration_sample,
    })
}

fn prepare_observed_inputs(
    calendar_registry: &ExchangeCalendarRegistry,
    run_spec: &CalibrationRunSpec,
    stage_input: &JointRefinementStageExecutionInput,
    resolved_stage_payloads: &ResolvedStagePayloads<'_>,
) -> Result<PreparedObservedInputs, JointRefinementStageExecutionFailure> {
    let observed_open_event_rate_per_hour =
        selected_hawkes_observed_open_event_rate_per_hour(resolved_stage_payloads.hawkes_payload)
            .ok_or(JointRefinementStageExecutionFailure::MissingHawkesPayload)?;

    let observed_data = build_observed_data(JointRefinementObservedDataBuildInput {
        calendar_registry,
        exchange_code: run_spec.data_inputs.exchange_code.as_str(),
        return_components_fitted_parameters: &resolved_stage_payloads
            .return_components_payload
            .fitted_parameters,
        observed_signal_preparation: &resolved_stage_payloads.observed_signal_preparation,
        candidate_generation_input: &stage_input.candidate_generation_input,
        coarse_data_records: stage_input.coarse_data_records.as_deref(),
        tick_objective_data_usage_policy: stage_input.tick_objective_data_usage_policy,
        observed_open_event_rate_per_hour,
    })
    .map_err(|error_message| {
        JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput { error_message }
    })?;

    let baseline_15m_calibration_sample =
        build_continuous_calibration_sample_from_aggregated_intervals(
            observed_data.observed_15m_intervals.as_slice(),
            resolved_stage_payloads.baseline_modeled_open_event_rate_per_hour,
            observed_data.observed_open_event_rate_per_hour,
            observed_data.fallback_sell_share_proxy,
        );

    Ok(PreparedObservedInputs {
        observed_data,
        baseline_15m_calibration_sample,
    })
}

fn prepare_parameterization(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: &JointRefinementStageExecutionInput,
    resolved_stage_payloads: &ResolvedStagePayloads<'_>,
    baseline_15m_calibration_sample: &crate::stages::return_components::ReturnComponentsStageContinuousReturnCalibrationSample,
    observed_data: &JointRefinementObservedData,
) -> Result<PreparedParameterization, JointRefinementStageExecutionFailure> {
    let active_parameter_layout = build_active_parameter_layout(
        &resolved_stage_payloads
            .return_components_payload
            .fitted_parameters,
    );

    let baseline_parameter_vector = build_baseline_parameter_vector(
        &active_parameter_layout,
        &resolved_stage_payloads.hawkes_payload.fitted_parameters,
        &resolved_stage_payloads
            .return_components_payload
            .fitted_parameters,
        baseline_15m_calibration_sample,
    )
    .map_err(|error_message| {
        JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput { error_message }
    })?;

    let (mut lower_bounds, mut upper_bounds) = build_parameter_bounds(ParameterBoundsBuildInput {
        active_parameter_layout: &active_parameter_layout,
        baseline_parameter_vector: baseline_parameter_vector.as_slice(),
        baseline_15m_calibration_sample,
        selected_return_nig_configuration: resolved_stage_payloads
            .return_components_payload
            .fitted_parameters
            .nig_shared_plus_shift_configuration,
        return_component_basin_diagnostics: selected_return_components_candidate(
            resolved_stage_payloads.return_components_payload,
        )
        .map(|candidate| {
            candidate
                .continuous_distribution_fit_breakdown
                .optimizer_basin_diagnostics
                .as_slice()
        })
        .unwrap_or(&[]),
        baseline_hawkes_kernel_parameters: resolved_stage_payloads
            .hawkes_payload
            .fitted_parameters
            .kernel_parameters,
        baseline_long_variance_parameters: resolved_stage_payloads
            .return_components_payload
            .fitted_parameters
            .long_variance_parameters,
        observed_data,
        baseline_modeled_open_event_rate_per_hour: resolved_stage_payloads
            .baseline_modeled_open_event_rate_per_hour,
        solver_settings: &resolved_stage_payloads.stage_solver_settings,
    })
    .map_err(|error_message| {
        JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput { error_message }
    })?;

    let option_pricing_refinement_enabled = false;

    let option_pricing_context = build_option_pricing_context(
        run_spec,
        calendar_registry,
        stage_input,
        resolved_stage_payloads.hawkes_payload,
        resolved_stage_payloads.return_components_payload,
        resolved_stage_payloads.mean_reversion_payload,
        resolved_stage_payloads.stage_solver_settings.cpu_count,
    )?;

    freeze_physical_parameter_bounds_for_option_pricing(
        &active_parameter_layout,
        baseline_parameter_vector.as_slice(),
        lower_bounds.as_mut_slice(),
        upper_bounds.as_mut_slice(),
        option_pricing_refinement_enabled,
    );

    Ok(PreparedParameterization {
        active_parameter_layout,
        baseline_parameter_vector,
        lower_bounds,
        upper_bounds,
        option_pricing_refinement_enabled,
        option_pricing_context,
    })
}

fn build_bootstrap_context(
    input: BootstrapContextBuildInput<'_>,
) -> Result<JointRefinementContext, JointRefinementStageExecutionFailure> {
    let BootstrapContextBuildInput {
        run_spec,
        calendar_registry,
        resolved_stage_payloads,
        observed_data,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        option_pricing_refinement_enabled,
        option_pricing_context,
        runtime_materialization,
        seasonality_clock_model,
        baseline_return_components_fitted_parameters,
        cancellation_token,
    } = input;

    Ok(JointRefinementContext {
        cancellation_token,
        stage_thread_pool: build_stage_thread_pool(
            resolved_stage_payloads.stage_solver_settings.cpu_count,
        ),
        summary_simulation_scheduler: build_summary_simulation_scheduler(
            resolved_stage_payloads.stage_solver_settings.cpu_count,
            &runtime_materialization.summary_simulation_session_context,
        )?,
        simulation_infrastructure_failure: Arc::new(Mutex::new(None)),
        option_pricing_execution_failure: Arc::new(Mutex::new(None)),
        calendar_registry: calendar_registry.clone(),
        exchange_code: run_spec.data_inputs.exchange_code.clone(),
        seasonality_clock_model,
        runtime_interval_seasonality_factors: runtime_materialization
            .runtime_interval_seasonality_factors,
        observed_data,
        baseline_modeled_open_event_rate_per_hour: resolved_stage_payloads
            .baseline_modeled_open_event_rate_per_hour,
        baseline_hawkes_fitted_parameters: resolved_stage_payloads
            .hawkes_payload
            .fitted_parameters
            .clone(),
        baseline_return_components_fitted_parameters,
        baseline_mean_reversion_fitted_parameters: resolved_stage_payloads
            .mean_reversion_payload
            .fitted_parameters,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        option_pricing_refinement_enabled,
        local_simulation_seed: resolved_stage_payloads.resolved_stage_seed.stage_seed,
        final_simulation_seeds: resolved_stage_payloads.final_simulation_seeds.clone(),
        objective_cache: Arc::new(Mutex::new(BTreeMap::new())),
        deterministic_evaluation_cache: Arc::new(Mutex::new(BTreeMap::new())),
        aggregated_likelihood_context_cache: Arc::new(Mutex::new(BTreeMap::new())),
        observed_return_shape_statistic_cache: Arc::new(Mutex::new(BTreeMap::new())),
        observed_return_shape_covariance_cache: Arc::new(Mutex::new(BTreeMap::new())),
        seed_summary_cache: Arc::new(Mutex::new(BTreeMap::new())),
        option_pricing_cache: Arc::new(Mutex::new(BTreeMap::new())),
        option_pricing_context,
        profiling_state: Arc::new(JointRefinementProfilingState::default()),
    })
}

fn validate_baseline_parameter_round_trip(
    baseline_parameter_vector: &[f64],
    bootstrap_context: &JointRefinementContext,
) -> Result<(), JointRefinementStageExecutionFailure> {
    let _decoded_fitted_parameters = checked_parameter_vector_to_fitted_parameters(
        baseline_parameter_vector,
        bootstrap_context,
    )
    .map_err(|source| JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
        error_message: format!(
            "joint refinement baseline parameter vector failed to decode into fitted parameters: {source}"
        ),
    })?;

    Ok(())
}

struct BootstrapContextBuildInput<'a> {
    run_spec: &'a CalibrationRunSpec,
    calendar_registry: &'a ExchangeCalendarRegistry,
    resolved_stage_payloads: &'a ResolvedStagePayloads<'a>,
    observed_data: JointRefinementObservedData,
    active_parameter_layout: JointRefinementActiveParameterLayout,
    lower_bounds: Vec<f64>,
    upper_bounds: Vec<f64>,
    option_pricing_refinement_enabled: bool,
    option_pricing_context: Option<JointRefinementOptionPricingContext>,
    runtime_materialization: RuntimeMaterialization,
    seasonality_clock_model: crate::stages::seasonality::FourierBusinessTimeClockModel,
    baseline_return_components_fitted_parameters: ReturnComponentsStageFittedParameters,
    cancellation_token: CancellationToken,
}

struct RuntimeMaterialization {
    runtime_interval_seasonality_factors: Vec<f64>,
    summary_simulation_session_context: JointRefinementSummarySimulationSessionContext,
}

fn materialize_runtime_state(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: &JointRefinementStageExecutionInput,
    observed_data: &JointRefinementObservedData,
    return_components_fitted_parameters: &ReturnComponentsStageFittedParameters,
) -> Result<RuntimeMaterialization, JointRefinementStageExecutionFailure> {
    let runtime_range = runtime_interval_seasonality_range(observed_data)?;

    let runtime_interval_seasonality_factors =
        build_runtime_interval_seasonality_factors_from_components(
            RuntimeIntervalSeasonalityFactorsBuildInput {
                calendar_registry,
                exchange_code: stage_input.seasonality_factors_input.exchange_code.as_str(),
                clock_model: &stage_input.seasonality_factors_input.clock_model,
                range_start_utc_nanos: runtime_range.0,
                range_end_utc_nanos: runtime_range.1,
                interval_duration_nanos: NANOS_PER_HOUR,
            },
        )
        .map_err(|error_message| {
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: format!(
                    "joint refinement runtime seasonality materialization failed: {error_message}"
                ),
            }
        })?;

    let summary_simulation_session_context = build_summary_simulation_session_context(
        calendar_registry,
        observed_data,
        run_spec.data_inputs.exchange_code.as_str(),
        return_components_fitted_parameters
            .news_process_configuration
            .arrivals
            .known
            .scheduled_event_timestamps_utc_nanos
            .as_slice(),
        runtime_interval_seasonality_factors.as_slice(),
    )
    .map_err(|error_message| {
        JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput { error_message }
    })?;

    Ok(RuntimeMaterialization {
        runtime_interval_seasonality_factors,
        summary_simulation_session_context,
    })
}

fn runtime_interval_seasonality_range(
    observed_data: &JointRefinementObservedData,
) -> Result<(i64, i64), JointRefinementStageExecutionFailure> {
    let Some(range_start_utc_nanos) = observed_data
        .hourly_simulation_start_utc
        .timestamp_nanos_opt()
    else {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint refinement runtime seasonality start timestamp is out of range"
                        .to_string(),
            },
        );
    };

    let Some(range_end_utc_nanos) = observed_data
        .hourly_simulation_end_utc
        .timestamp_nanos_opt()
    else {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: "joint refinement runtime seasonality end timestamp is out of range"
                    .to_string(),
            },
        );
    };

    Ok((range_start_utc_nanos, range_end_utc_nanos))
}

fn freeze_physical_parameter_bounds_for_option_pricing(
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    baseline_parameter_vector: &[f64],
    lower_bounds: &mut [f64],
    upper_bounds: &mut [f64],
    option_pricing_enabled: bool,
) {
    if !option_pricing_enabled {
        return;
    }

    for parameter_index in active_parameter_layout
        .hawkes_block
        .iter()
        .chain(active_parameter_layout.nig_block.iter())
        .chain(active_parameter_layout.return_clock_block.iter())
        .chain(active_parameter_layout.long_variance_block.iter())
        .chain(active_parameter_layout.coupling_block.iter())
    {
        lower_bounds[*parameter_index] = baseline_parameter_vector[*parameter_index];
        upper_bounds[*parameter_index] = baseline_parameter_vector[*parameter_index];
    }
}
