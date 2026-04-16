use std::collections::BTreeMap;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use janiform_calibration_artifacts::{
    CALIBRATION_JOINT_REFINEMENT_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_JOINT_REFINEMENT_STAGE_HANDOFF_POLICY,
    CALIBRATION_JOINT_REFINEMENT_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_STAGE_SEED_DERIVATION_POLICY, CalibrationOptimizerSettings,
    CalibrationResolvedStageSeed, CalibrationRunSpec, CalibrationStage,
    JointRefinementStageOptimizerDiagnostics, ReturnComponentsStageFittedParameters,
    resolve_calibration_stage_seed_metadata, stage_solver_settings_for_stage,
};
use janiform_foundations::splitmix64_output_mixer;
use janiform_foundations::time::ONE_HOUR_NANOS;

use crate::optimizer_adapter::CalibrationOptimizerAdapterRunMetadata;
use crate::stages::hawkes::{HawkesStageFittedPayload, stationary_open_event_rate_per_hour};
use crate::stages::joint_metrics::build_continuous_calibration_sample_from_aggregated_intervals;
use crate::stages::joint_refinement::contracts::{
    JointRefinementStageExecutionFailure, JointRefinementStageExecutionInput,
    JointRefinementStageExecutionMetadata, JointRefinementStageExecutionReport,
    JointRefinementStageFittedPayload, JointRefinementStageProgressPhase,
    JointRefinementStageProgressUpdate,
};
use crate::stages::joint_refinement::option_surface_tiering::JointRefinementOptionSurfaceTierKind;
use crate::stages::mean_reversion::MeanReversionStageFittedPayload;
use crate::stages::return_components::{
    ReturnComponentsStageFittedPayload,
    ReturnComponentsStageJointRefinementObservedSignalPreparation,
    prepare_observed_signal_inputs_for_joint_refinement, resolve_activity_calibration_context,
};
use crate::stages::seasonality::{
    CalibrationStageExecutionStatus, build_runtime_hourly_seasonality_factors_from_components,
    maximum_realizable_runtime_hourly_seasonality_factor_from_components,
};
use janiform_market_time::calendar::ExchangeCalendarRegistry;

use super::super::objective::evaluate_joint_fitted_parameters;
use super::super::setup::{
    build_active_parameter_layout, build_baseline_parameter_vector, build_parameter_bounds,
    build_restart_seeds, joint_refinement_final_simulation_seed_count, observed_activity_log_shift,
    selected_hawkes_observed_open_event_rate_per_hour, selected_return_components_candidate,
    stage_input_to_activity_input,
};
use super::super::worker_runtime::JointRefinementSummarySimulationScheduler;
use super::{
    JointRefinementActiveParameterLayout, JointRefinementBaselineBuckets, JointRefinementContext,
    JointRefinementFidelityTier, JointRefinementProfilingState, JointRefinementRestartOutcome,
    baseline_buckets_from_evaluation, build_joint_candidate_coarse_consistency_artifacts,
    build_joint_candidate_frontier_diagnostics, build_joint_finalist_seed_summaries,
    build_joint_refinement_stage_failure_report, build_joint_restart_diagnostics,
    build_joint_simulation_summaries, build_observed_data, build_option_calibration_diagnostics,
    build_option_pricing_context, build_summary_simulation_session_context,
    desired_joint_refinement_option_refinement_candidate_count,
    desired_joint_refinement_restart_count, joint_refinement_budget_schedule,
    optimize_restart_seeds, ordered_parameter_bits, resolve_option_surface_tier_pricing_artifact,
    run_option_refinement, select_restart_outcomes_with_lazy_coarse_consistency,
    simulation_infrastructure_failure_message,
};

const JOINT_REFINEMENT_SELECTION_POLICY: &str = "validity_then_admissibility_with_incumbent_baseline_and_direct_interval_coarse_consistency_then_pareto_then_lexicographic_tie_break.v1";

struct ResolvedStagePayloads<'a> {
    stage_solver_settings: CalibrationOptimizerSettings,
    hawkes_payload: &'a HawkesStageFittedPayload,
    return_components_payload: &'a ReturnComponentsStageFittedPayload,
    mean_reversion_payload: &'a MeanReversionStageFittedPayload,
    resolved_stage_seed: CalibrationResolvedStageSeed,
    observed_signal_preparation: ReturnComponentsStageJointRefinementObservedSignalPreparation,
    baseline_modeled_open_event_rate_per_hour: f64,
    final_simulation_seeds: Vec<u64>,
}

struct PreparedBootstrapContext {
    bootstrap_context: JointRefinementContext,
    baseline_parameter_vector: Vec<f64>,
    baseline_15m_calibration_sample:
        crate::stages::return_components::ReturnComponentsStageContinuousReturnCalibrationSample,
}

struct PreparedObservedInputs {
    observed_data: super::JointRefinementObservedData,
    baseline_15m_calibration_sample:
        crate::stages::return_components::ReturnComponentsStageContinuousReturnCalibrationSample,
}

struct PreparedParameterization {
    active_parameter_layout: JointRefinementActiveParameterLayout,
    baseline_parameter_vector: Vec<f64>,
    lower_bounds: Vec<f64>,
    upper_bounds: Vec<f64>,
    option_pricing_refinement_enabled: bool,
    option_pricing_context: Option<super::state::JointRefinementOptionPricingContext>,
}

struct BaselineEvaluationArtifacts {
    baseline_restart_outcome: JointRefinementRestartOutcome,
    baseline_coarse_consistency_artifacts:
        Option<super::coarse_consistency::JointRefinementCoarseConsistencyArtifacts>,
}

struct RestartSearchArtifacts {
    restart_outcomes: Vec<JointRefinementRestartOutcome>,
    valid_restart_count: usize,
    best_restart: JointRefinementRestartOutcome,
    option_refinement_candidate_frontier: Vec<JointRefinementRestartOutcome>,
    selected_option_search_tier_pricing_artifact:
        Option<Arc<super::state::JointRefinementOptionTierPricingArtifact>>,
    selected_option_selection_tier_pricing_artifact:
        Option<Arc<super::state::JointRefinementOptionTierPricingArtifact>>,
    selected_option_audit_tier_pricing_artifact:
        Option<Arc<super::state::JointRefinementOptionTierPricingArtifact>>,
    coarse_consistency_artifacts_by_restart_id:
        BTreeMap<String, super::coarse_consistency::JointRefinementCoarseConsistencyArtifacts>,
    selection_coarse_consistency_by_restart_id:
        BTreeMap<String, super::JointRefinementCoarseConsistencySummary>,
    restart_optimization_wall_clock_millis: u64,
    coarse_consistency_wall_clock_millis: u64,
}

struct RestartSearchPlan {
    restart_seeds: Vec<(String, Vec<f64>)>,
    budget_schedule: super::optimization::JointRefinementBudgetSchedule,
}

struct RestartSelectionArtifacts {
    best_restart: JointRefinementRestartOutcome,
    option_refinement_candidate_frontier: Vec<JointRefinementRestartOutcome>,
    coarse_consistency_artifacts_by_restart_id:
        BTreeMap<String, super::coarse_consistency::JointRefinementCoarseConsistencyArtifacts>,
    selection_coarse_consistency_by_restart_id:
        BTreeMap<String, super::JointRefinementCoarseConsistencySummary>,
    coarse_consistency_wall_clock_millis: u64,
}

struct FinalAssemblyArtifacts {
    candidate_frontier:
        Vec<crate::stages::joint_refinement::contracts::JointRefinementStageCandidateDiagnostic>,
    simulation_summaries:
        Vec<crate::stages::joint_refinement::contracts::JointRefinementStageSimulationSummary>,
    coarse_tick_consistency: Vec<
        crate::stages::joint_refinement::contracts::JointRefinementStageCoarseTickConsistencyMetric,
    >,
    option_calibration_diagnostics:
        Option<janiform_calibration_artifacts::JointRefinementStageOptionCalibrationDiagnostics>,
    final_seed_evaluation_wall_clock_millis: u64,
    final_assembly_wall_clock_millis: u64,
}

pub(super) fn run_joint_refinement_calibration_stage_internal(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: JointRefinementStageExecutionInput,
    progress_sender: Option<Sender<JointRefinementStageProgressUpdate>>,
) -> JointRefinementStageExecutionReport {
    if stage_input.restart_count_override == Some(0) {
        return build_joint_refinement_stage_failure_report(
            run_spec,
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: "joint refinement restart_count_override must be greater than zero"
                    .to_string(),
            },
        );
    }
    if stage_input.option_refinement_candidate_count_override == Some(0) {
        return build_joint_refinement_stage_failure_report(
            run_spec,
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint refinement option_refinement_candidate_count_override must be greater than zero"
                        .to_string(),
            },
        );
    }
    if stage_input.audit_option_pricing_enabled && stage_input.option_pricing_input.is_none() {
        return build_joint_refinement_stage_failure_report(
            run_spec,
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint refinement audit_option_pricing_enabled requires option_pricing_input"
                        .to_string(),
            },
        );
    }

    let resolved_stage_payloads =
        match resolve_stage_payloads(run_spec, calendar_registry, &stage_input) {
            Ok(payloads) => payloads,
            Err(failure) => return build_joint_refinement_stage_failure_report(run_spec, failure),
        };

    let mut prepared_bootstrap = match prepare_bootstrap_context(
        run_spec,
        calendar_registry,
        &stage_input,
        &resolved_stage_payloads,
    ) {
        Ok(prepared_bootstrap) => prepared_bootstrap,
        Err(failure) => return build_joint_refinement_stage_failure_report(run_spec, failure),
    };

    let baseline_evaluation_artifacts = match establish_baseline_evaluation(
        &resolved_stage_payloads,
        &mut prepared_bootstrap.bootstrap_context,
        prepared_bootstrap.baseline_parameter_vector.as_slice(),
        progress_sender.as_ref(),
    ) {
        Ok(artifacts) => artifacts,
        Err(failure) => return build_joint_refinement_stage_failure_report(run_spec, failure),
    };

    let mut restart_search_artifacts = match run_restart_search(
        &stage_input,
        &resolved_stage_payloads,
        &prepared_bootstrap,
        &baseline_evaluation_artifacts,
        progress_sender.as_ref(),
    ) {
        Ok(artifacts) => artifacts,
        Err(failure) => return build_joint_refinement_stage_failure_report(run_spec, failure),
    };

    if let Err(failure) = ensure_valid_selected_restart_outcome(&restart_search_artifacts) {
        return build_joint_refinement_stage_failure_report(run_spec, failure);
    }

    if let Err(failure) = run_option_refinement_if_enabled(
        &stage_input,
        &resolved_stage_payloads,
        &prepared_bootstrap.bootstrap_context,
        &mut restart_search_artifacts,
        progress_sender.as_ref(),
    ) {
        return build_joint_refinement_stage_failure_report(run_spec, failure);
    }
    if let Err(failure) = run_audit_option_pricing_if_enabled(
        &stage_input,
        &prepared_bootstrap.bootstrap_context,
        &mut restart_search_artifacts,
        progress_sender.as_ref(),
    ) {
        return build_joint_refinement_stage_failure_report(run_spec, failure);
    }

    let final_assembly_artifacts = match assemble_final_stage_outputs(
        &prepared_bootstrap.bootstrap_context,
        &baseline_evaluation_artifacts,
        &restart_search_artifacts,
        progress_sender.as_ref(),
    ) {
        Ok(artifacts) => artifacts,
        Err(failure) => return build_joint_refinement_stage_failure_report(run_spec, failure),
    };

    build_success_report(
        run_spec,
        &resolved_stage_payloads,
        &prepared_bootstrap.bootstrap_context,
        &restart_search_artifacts,
        final_assembly_artifacts,
    )
}

fn resolve_stage_payloads<'a>(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: &'a JointRefinementStageExecutionInput,
) -> Result<ResolvedStagePayloads<'a>, JointRefinementStageExecutionFailure> {
    validate_joint_refinement_stage_request(run_spec)?;

    let stage_solver_settings =
        stage_solver_settings_for_stage(run_spec, CalibrationStage::JointRefinement)
            .ok_or(JointRefinementStageExecutionFailure::MissingStageSolverSettings)?;

    let hawkes_payload = stage_input
        .hawkes_input
        .payload()
        .ok_or(JointRefinementStageExecutionFailure::MissingHawkesPayload)?;
    let return_components_payload = stage_input
        .return_components_input
        .payload()
        .ok_or(JointRefinementStageExecutionFailure::MissingReturnComponentsPayload)?;
    let mean_reversion_payload = stage_input
        .mean_reversion_input
        .payload()
        .ok_or(JointRefinementStageExecutionFailure::MissingMeanReversionPayload)?;

    let resolved_stage_seed =
        resolve_calibration_stage_seed_metadata(run_spec, CalibrationStage::JointRefinement);

    let activity_context = resolve_activity_calibration_context(&stage_input_to_activity_input(
        &stage_input.hawkes_input,
    ));

    let observed_signal_preparation = prepare_observed_signal_inputs_for_joint_refinement(
        run_spec.data_inputs.exchange_code.as_str(),
        calendar_registry,
        stage_input.candidate_generation_input.clone(),
        activity_context,
    )
    .map_err(|source| {
        JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
            error_message: source.to_string(),
        }
    })?;

    let baseline_modeled_open_event_rate_per_hour =
        stationary_open_event_rate_per_hour(&hawkes_payload.fitted_parameters)
            .ok_or(JointRefinementStageExecutionFailure::MissingHawkesPayload)?;

    Ok(ResolvedStagePayloads {
        stage_solver_settings: stage_solver_settings.clone(),
        hawkes_payload,
        return_components_payload,
        mean_reversion_payload,
        resolved_stage_seed,
        observed_signal_preparation,
        baseline_modeled_open_event_rate_per_hour,
        final_simulation_seeds: build_final_simulation_seeds(
            resolved_stage_seed.stage_seed,
            &stage_solver_settings,
        ),
    })
}

fn validate_joint_refinement_stage_request(
    run_spec: &CalibrationRunSpec,
) -> Result<(), JointRefinementStageExecutionFailure> {
    run_spec.validate().map_err(
        |source| JointRefinementStageExecutionFailure::InvalidRunSpec {
            error_message: format!("{source} (classification={:?})", source.classification()),
        },
    )?;

    if run_spec
        .selected_stages
        .iter()
        .copied()
        .any(|selected_stage| selected_stage == CalibrationStage::JointRefinement)
    {
        return Ok(());
    }

    Err(JointRefinementStageExecutionFailure::StageIsNotSelectedInRunSpec)
}

fn build_final_simulation_seeds(
    stage_seed: u64,
    stage_solver_settings: &CalibrationOptimizerSettings,
) -> Vec<u64> {
    let all_final_simulation_seeds = [
        stage_seed,
        splitmix64_output_mixer(stage_seed ^ 0xA5A5_A5A5_A5A5_A5A5),
        splitmix64_output_mixer(stage_seed ^ 0x5A5A_5A5A_5A5A_5A5A),
    ];

    all_final_simulation_seeds
        .into_iter()
        .take(joint_refinement_final_simulation_seed_count(
            stage_solver_settings,
        ))
        .collect()
}

fn return_components_fitted_parameters_with_mean_reversion(
    return_components_payload: &ReturnComponentsStageFittedPayload,
    mean_reversion_payload: &MeanReversionStageFittedPayload,
) -> ReturnComponentsStageFittedParameters {
    let mut fitted_parameters = return_components_payload.fitted_parameters.clone();
    fitted_parameters.mean_reversion_fitted_parameters =
        mean_reversion_payload.fitted_parameters.clone();
    fitted_parameters
}

fn prepare_bootstrap_context(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: &JointRefinementStageExecutionInput,
    resolved_stage_payloads: &ResolvedStagePayloads<'_>,
) -> Result<PreparedBootstrapContext, JointRefinementStageExecutionFailure> {
    let PreparedObservedInputs {
        observed_data,
        baseline_15m_calibration_sample,
    } = prepare_observed_inputs(
        calendar_registry,
        run_spec,
        stage_input,
        resolved_stage_payloads,
    )?;

    let PreparedParameterization {
        active_parameter_layout,
        baseline_parameter_vector,
        lower_bounds,
        upper_bounds,
        option_pricing_refinement_enabled,
        option_pricing_context,
    } = prepare_parameterization(
        run_spec,
        calendar_registry,
        stage_input,
        resolved_stage_payloads,
        &baseline_15m_calibration_sample,
    )?;

    let baseline_return_components_fitted_parameters =
        return_components_fitted_parameters_with_mean_reversion(
            resolved_stage_payloads.return_components_payload,
            resolved_stage_payloads.mean_reversion_payload,
        );

    let runtime_materialization = materialize_runtime_state(
        run_spec,
        calendar_registry,
        stage_input,
        &observed_data,
        &baseline_return_components_fitted_parameters,
    )?;

    let bootstrap_context = build_bootstrap_context(
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
        baseline_return_components_fitted_parameters,
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

    let observed_data = build_observed_data(
        calendar_registry,
        run_spec.data_inputs.exchange_code.as_str(),
        &resolved_stage_payloads
            .return_components_payload
            .fitted_parameters,
        &resolved_stage_payloads.observed_signal_preparation,
        &stage_input.candidate_generation_input,
        observed_open_event_rate_per_hour,
    );

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
    );

    let (mut lower_bounds, mut upper_bounds) = build_parameter_bounds(
        &active_parameter_layout,
        baseline_parameter_vector.as_slice(),
        baseline_15m_calibration_sample,
    );
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

    freeze_hawkes_and_coupling_bounds_for_option_pricing(
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
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    resolved_stage_payloads: &ResolvedStagePayloads<'_>,
    observed_data: super::JointRefinementObservedData,
    active_parameter_layout: JointRefinementActiveParameterLayout,
    lower_bounds: Vec<f64>,
    upper_bounds: Vec<f64>,
    option_pricing_refinement_enabled: bool,
    option_pricing_context: Option<super::state::JointRefinementOptionPricingContext>,
    runtime_materialization: RuntimeMaterialization,
    baseline_return_components_fitted_parameters: ReturnComponentsStageFittedParameters,
) -> Result<JointRefinementContext, JointRefinementStageExecutionFailure> {
    Ok(JointRefinementContext {
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
        runtime_hourly_seasonality_factors: runtime_materialization
            .runtime_hourly_seasonality_factors,
        max_runtime_hourly_seasonality_factor: runtime_materialization
            .max_runtime_hourly_seasonality_factor,
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
            .fitted_parameters
            .clone(),
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        option_pricing_refinement_enabled,
        local_simulation_seed: resolved_stage_payloads.resolved_stage_seed.stage_seed,
        final_simulation_seeds: resolved_stage_payloads.final_simulation_seeds.clone(),
        local_baseline_buckets: unit_baseline_buckets(),
        final_baseline_buckets: unit_baseline_buckets(),
        objective_cache: Arc::new(Mutex::new(BTreeMap::new())),
        deterministic_evaluation_cache: Arc::new(Mutex::new(BTreeMap::new())),
        seed_summary_cache: Arc::new(Mutex::new(BTreeMap::new())),
        option_pricing_cache: Arc::new(Mutex::new(BTreeMap::new())),
        option_pricing_context,
        profiling_state: Arc::new(JointRefinementProfilingState::default()),
    })
}

struct RuntimeMaterialization {
    runtime_hourly_seasonality_factors: Vec<f64>,
    max_runtime_hourly_seasonality_factor: f64,
    summary_simulation_session_context:
        super::super::summary_simulation::JointRefinementSummarySimulationSessionContext,
}

fn materialize_runtime_state(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: &JointRefinementStageExecutionInput,
    observed_data: &super::JointRefinementObservedData,
    return_components_fitted_parameters:
        &janiform_calibration_artifacts::ReturnComponentsStageFittedParameters,
) -> Result<RuntimeMaterialization, JointRefinementStageExecutionFailure> {
    let runtime_range = runtime_hourly_seasonality_range(observed_data)?;

    let runtime_hourly_seasonality_factors =
        build_runtime_hourly_seasonality_factors_from_components(
            calendar_registry,
            run_spec.data_inputs.exchange_code.as_str(),
            stage_input
                .seasonality_factors_input
                .intraday_minutes_per_bin,
            stage_input
                .seasonality_factors_input
                .intraday_factor_by_bin
                .as_slice(),
            stage_input
                .seasonality_factors_input
                .day_of_week_factor
                .as_slice(),
            stage_input
                .seasonality_factors_input
                .day_of_year_factor
                .as_slice(),
            runtime_range.0,
            runtime_range.1,
            ONE_HOUR_NANOS,
        )
        .map_err(|error_message| {
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: format!(
                    "joint refinement runtime seasonality materialization failed: {error_message}"
                ),
            }
        })?;

    let max_runtime_hourly_seasonality_factor =
        maximum_realizable_runtime_hourly_seasonality_factor_from_components(
            stage_input
                .seasonality_factors_input
                .intraday_minutes_per_bin,
            stage_input
                .seasonality_factors_input
                .intraday_factor_by_bin
                .as_slice(),
            stage_input
                .seasonality_factors_input
                .day_of_week_factor
                .as_slice(),
            stage_input
                .seasonality_factors_input
                .day_of_year_factor
                .as_slice(),
        )
        .map_err(|error_message| {
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: format!(
                    "joint refinement runtime seasonality upper-bound estimation failed: {error_message}"
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
        runtime_hourly_seasonality_factors.as_slice(),
    )
    .map_err(|error_message| {
        JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput { error_message }
    })?;

    Ok(RuntimeMaterialization {
        runtime_hourly_seasonality_factors,
        max_runtime_hourly_seasonality_factor,
        summary_simulation_session_context,
    })
}

fn runtime_hourly_seasonality_range(
    observed_data: &super::JointRefinementObservedData,
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

fn freeze_hawkes_and_coupling_bounds_for_option_pricing(
    active_parameter_layout: &super::JointRefinementActiveParameterLayout,
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
        .chain(active_parameter_layout.coupling_block.iter())
    {
        lower_bounds[*parameter_index] = baseline_parameter_vector[*parameter_index];
        upper_bounds[*parameter_index] = baseline_parameter_vector[*parameter_index];
    }
}

fn build_stage_thread_pool(cpu_count: usize) -> Option<Arc<rayon::ThreadPool>> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(cpu_count.max(1))
        .build()
        .ok()
        .map(Arc::new)
}

fn build_summary_simulation_scheduler(
    cpu_count: usize,
    summary_simulation_session_context: &super::super::summary_simulation::JointRefinementSummarySimulationSessionContext,
) -> Result<Arc<JointRefinementSummarySimulationScheduler>, JointRefinementStageExecutionFailure> {
    JointRefinementSummarySimulationScheduler::new(cpu_count, summary_simulation_session_context)
        .map(Arc::new)
        .map_err(|error_message| {
            JointRefinementStageExecutionFailure::SimulationInfrastructureUnavailable {
                error_message,
            }
        })
}

fn establish_baseline_evaluation(
    resolved_stage_payloads: &ResolvedStagePayloads<'_>,
    bootstrap_context: &mut JointRefinementContext,
    baseline_parameter_vector: &[f64],
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Result<BaselineEvaluationArtifacts, JointRefinementStageExecutionFailure> {
    let baseline_fitted_parameters =
        janiform_calibration_artifacts::JointRefinementStageFittedParameters {
            hawkes_fitted_parameters: resolved_stage_payloads
                .hawkes_payload
                .fitted_parameters
                .clone(),
            return_components_fitted_parameters: bootstrap_context
                .baseline_return_components_fitted_parameters
                .clone(),
            pricing_block_fitted_parameters: None,
        };

    let baseline_parameter_bits = ordered_parameter_bits(baseline_parameter_vector);

    let baseline_local_evaluation = evaluate_joint_fitted_parameters(
        baseline_fitted_parameters.clone(),
        Some(baseline_parameter_bits.as_slice()),
        bootstrap_context,
        JointRefinementFidelityTier::Local,
        &resolved_stage_payloads.stage_solver_settings,
        progress_sender,
        Some(JointRefinementStageProgressPhase::BaselineLocalEvaluation),
    );

    let baseline_final_evaluation = evaluate_joint_fitted_parameters(
        baseline_fitted_parameters,
        Some(baseline_parameter_bits.as_slice()),
        bootstrap_context,
        JointRefinementFidelityTier::Final,
        &resolved_stage_payloads.stage_solver_settings,
        progress_sender,
        Some(JointRefinementStageProgressPhase::BaselineFinalEvaluation),
    );

    ensure_simulation_infrastructure_available(bootstrap_context)?;

    bootstrap_context.local_baseline_buckets =
        baseline_buckets_from_evaluation(&baseline_local_evaluation);
    bootstrap_context.final_baseline_buckets =
        baseline_buckets_from_evaluation(&baseline_final_evaluation);

    let baseline_candidate_is_valid = baseline_local_evaluation.invalid_reason.is_none()
        && baseline_final_evaluation.invalid_reason.is_none();

    let baseline_coarse_consistency_artifacts = if baseline_candidate_is_valid {
        emit_coarse_consistency_progress(progress_sender, 0, 1, "baseline coarse_consistency");
        build_joint_candidate_coarse_consistency_artifacts(
            bootstrap_context,
            &baseline_final_evaluation.fitted_parameters,
            Some(baseline_parameter_bits.as_slice()),
            None,
        )
        .inspect(|_| {
            emit_coarse_consistency_progress(progress_sender, 1, 1, "baseline coarse_consistency");
        })
    } else {
        None
    };

    let baseline_restart_outcome = JointRefinementRestartOutcome {
        restart_id: "baseline".to_string(),
        parameter_vector: baseline_parameter_vector.to_vec(),
        local_evaluation: baseline_local_evaluation.clone(),
        final_evaluation: baseline_final_evaluation.clone(),
        local_evaluation_count: 0,
        stop_reason: "baseline_incumbent",
    };

    if let Ok(mut cache_guard) = bootstrap_context.objective_cache.lock() {
        cache_guard.clear();
    }

    Ok(BaselineEvaluationArtifacts {
        baseline_restart_outcome,
        baseline_coarse_consistency_artifacts,
    })
}

fn ensure_valid_selected_restart_outcome(
    restart_search_artifacts: &RestartSearchArtifacts,
) -> Result<(), JointRefinementStageExecutionFailure> {
    if restart_search_artifacts
        .best_restart
        .local_evaluation
        .invalid_reason
        .is_none()
        && restart_search_artifacts
            .best_restart
            .final_evaluation
            .invalid_reason
            .is_none()
    {
        return Ok(());
    }

    let local_reason = restart_search_artifacts
        .best_restart
        .local_evaluation
        .invalid_reason
        .clone()
        .unwrap_or_else(|| "unknown_local_joint_refinement_failure".to_string());
    let final_reason = restart_search_artifacts
        .best_restart
        .final_evaluation
        .invalid_reason
        .clone()
        .unwrap_or_else(|| "unknown_final_joint_refinement_failure".to_string());

    Err(
        JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
            error_message: format!(
                "joint refinement could not identify a valid candidate after restart search: selected_restart={} valid_restart_count={} local={local_reason} final={final_reason}",
                restart_search_artifacts.best_restart.restart_id,
                restart_search_artifacts.valid_restart_count,
            ),
        },
    )
}

fn run_restart_search(
    stage_input: &JointRefinementStageExecutionInput,
    resolved_stage_payloads: &ResolvedStagePayloads<'_>,
    prepared_bootstrap: &PreparedBootstrapContext,
    baseline_evaluation_artifacts: &BaselineEvaluationArtifacts,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Result<RestartSearchArtifacts, JointRefinementStageExecutionFailure> {
    let restart_search_plan =
        build_restart_search_plan(stage_input, resolved_stage_payloads, prepared_bootstrap);

    let restart_optimization_started_at = Instant::now();
    let restart_outcomes = optimize_restart_seeds(
        restart_search_plan.restart_seeds.as_slice(),
        &prepared_bootstrap.bootstrap_context,
        &resolved_stage_payloads.stage_solver_settings,
        restart_search_plan.budget_schedule,
        progress_sender,
    );
    let restart_optimization_wall_clock_millis = elapsed_millis(restart_optimization_started_at);

    ensure_simulation_infrastructure_available(&prepared_bootstrap.bootstrap_context)?;

    let valid_restart_count = count_valid_restart_outcomes(&restart_outcomes);
    let restart_selection_artifacts = select_restart_outcome(
        stage_input,
        prepared_bootstrap,
        baseline_evaluation_artifacts,
        restart_outcomes.as_slice(),
        progress_sender,
    );

    Ok(RestartSearchArtifacts {
        restart_outcomes,
        valid_restart_count,
        best_restart: restart_selection_artifacts.best_restart,
        option_refinement_candidate_frontier: restart_selection_artifacts
            .option_refinement_candidate_frontier,
        selected_option_search_tier_pricing_artifact: None,
        selected_option_selection_tier_pricing_artifact: None,
        selected_option_audit_tier_pricing_artifact: None,
        coarse_consistency_artifacts_by_restart_id: restart_selection_artifacts
            .coarse_consistency_artifacts_by_restart_id,
        selection_coarse_consistency_by_restart_id: restart_selection_artifacts
            .selection_coarse_consistency_by_restart_id,
        restart_optimization_wall_clock_millis,
        coarse_consistency_wall_clock_millis: restart_selection_artifacts
            .coarse_consistency_wall_clock_millis,
    })
}

fn build_restart_search_plan(
    stage_input: &JointRefinementStageExecutionInput,
    resolved_stage_payloads: &ResolvedStagePayloads<'_>,
    prepared_bootstrap: &PreparedBootstrapContext,
) -> RestartSearchPlan {
    let restart_seeds = build_restart_seeds(
        prepared_bootstrap.baseline_parameter_vector.as_slice(),
        &prepared_bootstrap.bootstrap_context.active_parameter_layout,
        prepared_bootstrap.bootstrap_context.lower_bounds.as_slice(),
        prepared_bootstrap.bootstrap_context.upper_bounds.as_slice(),
        &prepared_bootstrap.baseline_15m_calibration_sample,
        selected_return_components_candidate(resolved_stage_payloads.return_components_payload)
            .map(|candidate| {
                candidate
                    .continuous_distribution_fit_breakdown
                    .optimizer_basin_diagnostics
                    .as_slice()
            })
            .unwrap_or(&[]),
        observed_activity_log_shift(
            prepared_bootstrap
                .bootstrap_context
                .observed_data
                .observed_open_event_rate_per_hour,
            resolved_stage_payloads.baseline_modeled_open_event_rate_per_hour,
        ),
        &prepared_bootstrap.bootstrap_context,
        desired_joint_refinement_restart_count(stage_input.restart_count_override),
    );

    let budget_schedule = joint_refinement_budget_schedule(
        &resolved_stage_payloads.stage_solver_settings,
        restart_seeds.len(),
    );

    RestartSearchPlan {
        restart_seeds,
        budget_schedule,
    }
}

fn count_valid_restart_outcomes(restart_outcomes: &[JointRefinementRestartOutcome]) -> usize {
    restart_outcomes
        .iter()
        .filter(|restart_outcome| {
            restart_outcome.local_evaluation.invalid_reason.is_none()
                && restart_outcome.final_evaluation.invalid_reason.is_none()
        })
        .count()
}

fn run_option_refinement_if_enabled(
    stage_input: &JointRefinementStageExecutionInput,
    resolved_stage_payloads: &ResolvedStagePayloads<'_>,
    bootstrap_context: &JointRefinementContext,
    restart_search_artifacts: &mut RestartSearchArtifacts,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Result<(), JointRefinementStageExecutionFailure> {
    if stage_input.option_pricing_input.is_none() {
        return Ok(());
    }

    let option_refinement_result = run_option_refinement(
        bootstrap_context,
        &resolved_stage_payloads.stage_solver_settings,
        restart_search_artifacts
            .option_refinement_candidate_frontier
            .as_slice(),
        progress_sender,
    )?;

    restart_search_artifacts.best_restart = option_refinement_result.selected_restart_outcome;
    restart_search_artifacts.selected_option_search_tier_pricing_artifact =
        Some(option_refinement_result.selected_search_tier_pricing_artifact);
    restart_search_artifacts.selected_option_selection_tier_pricing_artifact =
        Some(option_refinement_result.selected_selection_tier_pricing_artifact);
    Ok(())
}

fn run_audit_option_pricing_if_enabled(
    stage_input: &JointRefinementStageExecutionInput,
    bootstrap_context: &JointRefinementContext,
    restart_search_artifacts: &mut RestartSearchArtifacts,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Result<(), JointRefinementStageExecutionFailure> {
    if !stage_input.audit_option_pricing_enabled {
        return Ok(());
    }

    let selection_artifact = restart_search_artifacts
        .selected_option_selection_tier_pricing_artifact
        .as_ref()
        .ok_or_else(|| {
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint option audit pricing requires the selected selection-tier pricing artifact"
                        .to_string(),
            }
        })?;
    let audit_artifact = resolve_option_surface_tier_pricing_artifact(
        bootstrap_context,
        &restart_search_artifacts
            .best_restart
            .final_evaluation
            .fitted_parameters,
        JointRefinementOptionSurfaceTierKind::Audit,
        selection_artifact
            .cache_key
            .source_dynamics_parameter_bits
            .as_slice(),
        selection_artifact
            .cache_key
            .pricing_parameter_bits
            .as_slice(),
        progress_sender,
        Some(JointRefinementStageProgressPhase::OptionAuditTierEvaluation),
        Some(0),
        Some(1),
    )
    .map_err(|error_message| {
        JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput { error_message }
    })?;

    restart_search_artifacts.selected_option_audit_tier_pricing_artifact = Some(audit_artifact);
    Ok(())
}

fn select_restart_outcome(
    stage_input: &JointRefinementStageExecutionInput,
    prepared_bootstrap: &PreparedBootstrapContext,
    baseline_evaluation_artifacts: &BaselineEvaluationArtifacts,
    restart_outcomes: &[JointRefinementRestartOutcome],
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> RestartSelectionArtifacts {
    let selectable_restart_outcomes = build_selectable_restart_outcomes(
        restart_outcomes,
        &baseline_evaluation_artifacts.baseline_restart_outcome,
    );

    let coarse_consistency_started_at = Instant::now();
    let valid_selectable_restart_outcome_refs = selectable_restart_outcomes
        .iter()
        .filter(|restart_outcome| {
            restart_outcome.local_evaluation.invalid_reason.is_none()
                && restart_outcome.final_evaluation.invalid_reason.is_none()
        })
        .collect::<Vec<_>>();

    let selected_restart_count = if prepared_bootstrap
        .bootstrap_context
        .option_pricing_context
        .is_some()
    {
        desired_joint_refinement_option_refinement_candidate_count(
            stage_input.option_refinement_candidate_count_override,
        )
    } else {
        1
    };

    let selection_artifacts = select_restart_outcomes_with_lazy_coarse_consistency(
        valid_selectable_restart_outcome_refs.as_slice(),
        &prepared_bootstrap.bootstrap_context,
        baseline_evaluation_artifacts
            .baseline_coarse_consistency_artifacts
            .as_ref(),
        selected_restart_count,
        progress_sender,
    );

    let selection_coarse_consistency_by_restart_id = selection_artifacts
        .coarse_consistency_artifacts_by_restart_id
        .iter()
        .map(|(restart_id, artifacts)| (restart_id.clone(), artifacts.summary))
        .collect::<BTreeMap<_, _>>();

    let best_restart = selection_artifacts
        .selected_restart_outcomes
        .first()
        .copied()
        .cloned()
        .unwrap_or_else(|| {
            baseline_evaluation_artifacts
                .baseline_restart_outcome
                .clone()
        });
    let option_refinement_candidate_frontier = if prepared_bootstrap
        .bootstrap_context
        .option_pricing_context
        .is_some()
        && selection_artifacts.used_unadmissible_fallback
    {
        Vec::new()
    } else {
        selection_artifacts
            .selected_restart_outcomes
            .iter()
            .copied()
            .cloned()
            .collect::<Vec<_>>()
    };

    RestartSelectionArtifacts {
        best_restart,
        option_refinement_candidate_frontier,
        coarse_consistency_artifacts_by_restart_id: selection_artifacts
            .coarse_consistency_artifacts_by_restart_id,
        selection_coarse_consistency_by_restart_id,
        coarse_consistency_wall_clock_millis: elapsed_millis(coarse_consistency_started_at),
    }
}

fn ensure_simulation_infrastructure_available(
    bootstrap_context: &JointRefinementContext,
) -> Result<(), JointRefinementStageExecutionFailure> {
    if let Some(error_message) = simulation_infrastructure_failure_message(bootstrap_context) {
        return Err(
            JointRefinementStageExecutionFailure::SimulationInfrastructureUnavailable {
                error_message,
            },
        );
    }

    Ok(())
}

fn build_selectable_restart_outcomes(
    restart_outcomes: &[JointRefinementRestartOutcome],
    baseline_restart_outcome: &JointRefinementRestartOutcome,
) -> Vec<JointRefinementRestartOutcome> {
    let mut selectable_restart_outcomes = Vec::with_capacity(restart_outcomes.len() + 1);

    selectable_restart_outcomes.push(baseline_restart_outcome.clone());
    selectable_restart_outcomes.extend(restart_outcomes.iter().cloned());

    selectable_restart_outcomes
}

fn assemble_final_stage_outputs(
    bootstrap_context: &JointRefinementContext,
    baseline_evaluation_artifacts: &BaselineEvaluationArtifacts,
    restart_search_artifacts: &RestartSearchArtifacts,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Result<FinalAssemblyArtifacts, JointRefinementStageExecutionFailure> {
    let selectable_restart_outcomes = build_selectable_restart_outcomes(
        restart_search_artifacts.restart_outcomes.as_slice(),
        &baseline_evaluation_artifacts.baseline_restart_outcome,
    );

    let candidate_frontier_started_at = Instant::now();
    let candidate_frontier = build_joint_candidate_frontier_diagnostics(
        selectable_restart_outcomes.as_slice(),
        bootstrap_context,
        &restart_search_artifacts.selection_coarse_consistency_by_restart_id,
        baseline_evaluation_artifacts
            .baseline_coarse_consistency_artifacts
            .is_some(),
    );

    let candidate_frontier_wall_clock_millis = elapsed_millis(candidate_frontier_started_at);

    emit_final_assembly_progress(progress_sender, 1, 3, "candidate_frontier");

    let final_seed_evaluation_started_at = Instant::now();
    let finalist_seed_summaries = build_joint_finalist_seed_summaries(
        bootstrap_context,
        Some(
            ordered_parameter_bits(
                restart_search_artifacts
                    .best_restart
                    .parameter_vector
                    .as_slice(),
            )
            .as_slice(),
        ),
        &restart_search_artifacts
            .best_restart
            .final_evaluation
            .fitted_parameters,
        progress_sender,
    );

    let final_seed_evaluation_wall_clock_millis = elapsed_millis(final_seed_evaluation_started_at);

    let remaining_final_assembly_started_at = Instant::now();
    let simulation_summaries = build_joint_simulation_summaries(finalist_seed_summaries.as_slice());

    emit_final_assembly_progress(progress_sender, 2, 3, "simulation_summaries");

    let coarse_tick_consistency = resolve_selected_coarse_tick_consistency(
        bootstrap_context,
        baseline_evaluation_artifacts,
        restart_search_artifacts,
        finalist_seed_summaries.as_slice(),
    );

    emit_final_assembly_progress(progress_sender, 3, 3, "coarse_tick_consistency");

    let option_calibration_diagnostics = build_option_calibration_diagnostics(
        bootstrap_context,
        restart_search_artifacts
            .selected_option_selection_tier_pricing_artifact
            .as_deref(),
        restart_search_artifacts
            .selected_option_audit_tier_pricing_artifact
            .as_deref(),
    )
    .map_err(|error_message| {
        JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput { error_message }
    })?;

    let final_assembly_wall_clock_millis =
        candidate_frontier_wall_clock_millis + elapsed_millis(remaining_final_assembly_started_at);

    Ok(FinalAssemblyArtifacts {
        candidate_frontier,
        simulation_summaries,
        coarse_tick_consistency,
        option_calibration_diagnostics,
        final_seed_evaluation_wall_clock_millis,
        final_assembly_wall_clock_millis,
    })
}

fn resolve_selected_coarse_tick_consistency(
    bootstrap_context: &JointRefinementContext,
    baseline_evaluation_artifacts: &BaselineEvaluationArtifacts,
    restart_search_artifacts: &RestartSearchArtifacts,
    finalist_seed_summaries: &[(u64, super::JointRefinementSeedSummary)],
) -> Vec<crate::stages::joint_refinement::contracts::JointRefinementStageCoarseTickConsistencyMetric>
{
    if restart_search_artifacts.best_restart.restart_id
        == baseline_evaluation_artifacts
            .baseline_restart_outcome
            .restart_id
    {
        return baseline_evaluation_artifacts
            .baseline_coarse_consistency_artifacts
            .as_ref()
            .map(|artifacts| artifacts.metrics.clone())
            .unwrap_or_else(|| {
                super::build_joint_coarse_tick_consistency_metrics(
                    bootstrap_context,
                    &restart_search_artifacts
                        .best_restart
                        .final_evaluation
                        .fitted_parameters,
                    finalist_seed_summaries,
                )
            });
    }

    restart_search_artifacts
        .coarse_consistency_artifacts_by_restart_id
        .get(restart_search_artifacts.best_restart.restart_id.as_str())
        .map(|artifacts| artifacts.metrics.clone())
        .unwrap_or_else(|| {
            super::build_joint_coarse_tick_consistency_metrics(
                bootstrap_context,
                &restart_search_artifacts
                    .best_restart
                    .final_evaluation
                    .fitted_parameters,
                finalist_seed_summaries,
            )
        })
}

fn build_success_report(
    run_spec: &CalibrationRunSpec,
    resolved_stage_payloads: &ResolvedStagePayloads<'_>,
    bootstrap_context: &JointRefinementContext,
    restart_search_artifacts: &RestartSearchArtifacts,
    final_assembly_artifacts: FinalAssemblyArtifacts,
) -> JointRefinementStageExecutionReport {
    let fitted_stage_payload = build_fitted_stage_payload(
        resolved_stage_payloads,
        bootstrap_context,
        restart_search_artifacts,
        final_assembly_artifacts,
    );

    let metadata = build_execution_metadata(
        resolved_stage_payloads,
        restart_search_artifacts,
        &fitted_stage_payload,
    );

    JointRefinementStageExecutionReport {
        schema_version: CALIBRATION_JOINT_REFINEMENT_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::JointRefinement,
        status: CalibrationStageExecutionStatus::Success,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_JOINT_REFINEMENT_STAGE_HANDOFF_POLICY,
        fitted_stage_payload: Some(fitted_stage_payload),
        metadata: Some(metadata),
        failure: None,
    }
}

fn build_fitted_stage_payload(
    resolved_stage_payloads: &ResolvedStagePayloads<'_>,
    bootstrap_context: &JointRefinementContext,
    restart_search_artifacts: &RestartSearchArtifacts,
    final_assembly_artifacts: FinalAssemblyArtifacts,
) -> JointRefinementStageFittedPayload {
    let restart_diagnostics =
        build_joint_restart_diagnostics(restart_search_artifacts.restart_outcomes.as_slice());

    let runtime_profile = bootstrap_context.profiling_state.snapshot_runtime_profile(
        restart_search_artifacts.restart_optimization_wall_clock_millis,
        restart_search_artifacts.coarse_consistency_wall_clock_millis,
        final_assembly_artifacts.final_seed_evaluation_wall_clock_millis,
        final_assembly_artifacts.final_assembly_wall_clock_millis,
    );

    JointRefinementStageFittedPayload {
        schema_version: CALIBRATION_JOINT_REFINEMENT_STAGE_OUTPUT_SCHEMA_VERSION,
        selected_candidate_id: format!(
            "joint_refinement_{}",
            restart_search_artifacts.best_restart.restart_id
        ),
        fitted_parameters: restart_search_artifacts
            .best_restart
            .final_evaluation
            .fitted_parameters
            .clone(),
        baseline_hawkes_fitted_parameters: resolved_stage_payloads
            .hawkes_payload
            .fitted_parameters
            .clone(),
        baseline_return_components_fitted_parameters:
            return_components_fitted_parameters_with_mean_reversion(
                resolved_stage_payloads.return_components_payload,
                resolved_stage_payloads.mean_reversion_payload,
            ),
        objective_breakdown: restart_search_artifacts
            .best_restart
            .final_evaluation
            .breakdown
            .clone(),
        optimizer_diagnostics: JointRefinementStageOptimizerDiagnostics {
            local_objective: restart_search_artifacts
                .best_restart
                .local_evaluation
                .breakdown
                .weighted_objective,
            final_objective: restart_search_artifacts
                .best_restart
                .final_evaluation
                .breakdown
                .weighted_objective,
            local_evaluation_count: restart_search_artifacts.best_restart.local_evaluation_count,
            final_simulation_seeds: bootstrap_context.final_simulation_seeds.clone(),
            stop_reason: restart_search_artifacts
                .best_restart
                .stop_reason
                .to_string(),
            restart_diagnostics,
            runtime_profile,
        },
        candidate_frontier: final_assembly_artifacts.candidate_frontier,
        simulation_summaries: final_assembly_artifacts.simulation_summaries,
        coarse_tick_consistency: final_assembly_artifacts.coarse_tick_consistency,
        option_calibration_diagnostics: final_assembly_artifacts.option_calibration_diagnostics,
    }
}

fn build_execution_metadata(
    resolved_stage_payloads: &ResolvedStagePayloads<'_>,
    restart_search_artifacts: &RestartSearchArtifacts,
    fitted_stage_payload: &JointRefinementStageFittedPayload,
) -> JointRefinementStageExecutionMetadata {
    JointRefinementStageExecutionMetadata {
        stage_output_schema_version: CALIBRATION_JOINT_REFINEMENT_STAGE_OUTPUT_SCHEMA_VERSION,
        run_global_seed: resolved_stage_payloads.resolved_stage_seed.run_global_seed,
        stage_seed: resolved_stage_payloads.resolved_stage_seed.stage_seed,
        stage_seed_source: resolved_stage_payloads
            .resolved_stage_seed
            .stage_seed_source,
        stage_seed_derivation_policy: CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
        solver_settings: resolved_stage_payloads.stage_solver_settings.clone(),
        optimizer_adapter_metadata: CalibrationOptimizerAdapterRunMetadata {
            schema_version: "calibration.optimizer.adapter_run.v1",
            backend_id: "joint_refinement.block_coordinate_reconciliation.v1".to_string(),
            random_seed: resolved_stage_payloads.resolved_stage_seed.stage_seed,
            selection_policy: JOINT_REFINEMENT_SELECTION_POLICY,
            penalty_policy: "invalid_candidates_are_screened_before_lexicographic_frontier_selection.v1",
            bounds_policy: "bounded_joint_block_coordinate_refinement.v1",
            solver_settings: resolved_stage_payloads.stage_solver_settings.clone(),
        },
        input_candidate_count: restart_search_artifacts.restart_outcomes.len(),
        valid_candidate_count: restart_search_artifacts.valid_restart_count,
        selected_candidate_raw_objective: fitted_stage_payload
            .objective_breakdown
            .weighted_objective,
        selected_candidate_penalized_objective: fitted_stage_payload
            .objective_breakdown
            .weighted_objective
            + fitted_stage_payload
                .objective_breakdown
                .hard_constraint_penalty,
    }
}

fn elapsed_millis(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

fn emit_final_assembly_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    completed_units: usize,
    total_units: usize,
    current_item_label: &str,
) {
    if let Some(progress_sender) = progress_sender {
        let _ = progress_sender.send(JointRefinementStageProgressUpdate {
            current_item_label: Some(current_item_label.to_string()),
            ..JointRefinementStageProgressUpdate::new(
                JointRefinementStageProgressPhase::FinalAssembly,
                completed_units,
                total_units,
            )
        });
    }
}

fn emit_coarse_consistency_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    completed_units: usize,
    total_units: usize,
    current_item_label: &str,
) {
    if let Some(progress_sender) = progress_sender {
        let _ = progress_sender.send(JointRefinementStageProgressUpdate {
            current_item_label: Some(current_item_label.to_string()),
            ..JointRefinementStageProgressUpdate::new(
                JointRefinementStageProgressPhase::CoarseConsistencyEvaluation,
                completed_units,
                total_units,
            )
        });
    }
}

fn unit_baseline_buckets() -> JointRefinementBaselineBuckets {
    JointRefinementBaselineBuckets {
        aggregated_likelihood_fit: 1.0,
        aggregated_return_shape_fit: 1.0,
        return_asymmetry_fit: 1.0,
        tick_return_asymmetry_fit: 1.0,
        aggregated_return_scale_fit: 1.0,
        tick_return_scale_fit: 1.0,
        business_time_return_scale_fit: 1.0,
        intraday_return_level_fit: 1.0,
        activity_shape_fit: 1.0,
        activity_level_fit: 1.0,
        rv_activity_coupling_fit: 1.0,
        price_span_sanity_fit: 1.0,
    }
}
