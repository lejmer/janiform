use std::sync::mpsc;
use std::sync::mpsc::Sender;

use serde::Serialize;

use janiform_calibration_artifacts::{
    CALIBRATION_LONG_VARIANCE_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_LONG_VARIANCE_STAGE_HANDOFF_POLICY,
    CALIBRATION_LONG_VARIANCE_STAGE_OUTPUT_SCHEMA_VERSION, CalibrationExecutionMode,
    CalibrationOptimizerSettings, CalibrationRunSpec, CalibrationStage,
    CalibrationStageHandoffTransport, CalibrationStageSeedSource,
    CalibrationStageSelectedCandidateSource, LongVarianceStageDiagnostics,
    LongVarianceStageHorizonDiagnostic, LongVarianceStagePersistenceLagDiagnostic,
    stage_solver_settings_for_stage,
};
use janiform_foundations::cancellation::CancellationToken;
use janiform_foundations::time::{NANOS_PER_FIVE_MINUTES, NANOS_PER_HOUR};
use janiform_market_time::calendar::ExchangeCalendarRegistry;
use janiform_model_kernels::long_variance::LongVarianceParameters;
use janiform_work_scheduler::{DynamicWorkPoolStatus, StaticWorkPoolInput, run_static_work_pool};

use crate::market_data::loading::MarketDataNormalizedCoarseDataRecord;
use crate::market_data::preparation::CalibrationTickObjectiveDataUsagePolicy;
use crate::stages::hawkes::HawkesStageFittedPayload;
use crate::stages::joint_metrics::JointRefinementAggregatedIntervalObservation;
use crate::stages::joint_metrics::build_observed_open_session_interval_observations;
use crate::stages::long_variance::estimation::{
    LongVarianceParameterEstimationDiagnostics, estimate_long_variance_parameters,
};
use crate::stages::long_variance::metrics::{
    LONG_VARIANCE_HORIZON_DEFINITIONS, LONG_VARIANCE_HORIZON_MOMENT_METRIC_COUNT,
    LongVarianceHorizonInputIntervals, LongVarianceHorizonMomentSummary,
    LongVarianceHorizonSummaryComputation, compute_long_variance_horizon_summary,
    long_variance_horizon_summary_computations,
    ordered_long_variance_horizon_summaries_from_outputs,
};
use crate::stages::long_variance::policy::long_variance_stationary_event_rates_for_hawkes_kernel;
use crate::stages::long_variance::progress::{
    LongVarianceStageProgressPhase, LongVarianceStageProgressUpdate,
};
use crate::stages::long_variance::source::{
    LONG_VARIANCE_INTERVAL_SOURCE_SELECTION_POLICY, select_long_variance_observed_intervals,
};
use crate::stages::parallelism::{bounded_stage_worker_count, build_stage_thread_pool};
use crate::stages::progress::{StageProgressUpdate, emit_stage_progress};
use crate::stages::return_components::ReturnComponentsStageCandidateGenerationInput;
use crate::stages::return_components::ReturnComponentsStageFittedPayload;
use crate::stages::seasonality::CalibrationStageExecutionStatus;
use crate::stages::tick_objective_data::{
    retained_open_session_returns_for_tick_objectives, source_window_for_retained_tick_objectives,
};

const LONG_VARIANCE_ESTIMATION_POLICY: &str = concat!(
    "fixed_lag_median_return_persistence_with_future_activity_loading_",
    "and_standard_cir_dispersion_projection.v1",
);
const LONG_VARIANCE_STATIONARY_NORMALIZATION_POLICY: &str = "unit_stationary_mean_with_mean_normalized_seasonal_baseline_and_uncompensated_event_feedback.v1";
pub(crate) const LONG_VARIANCE_ACTIVE_PARAMETER_COUNT: usize = 4;
pub(crate) const LONG_VARIANCE_OBJECTIVE_TERM_COUNT: usize =
    LONG_VARIANCE_HORIZON_DEFINITIONS.len() * LONG_VARIANCE_HORIZON_MOMENT_METRIC_COUNT;
pub(crate) const LONG_VARIANCE_CANDIDATE_GEOMETRY_COUNT: usize =
    LONG_VARIANCE_ACTIVE_PARAMETER_COUNT;

struct LongVarianceFittedStagePayloadBuildInput<'a> {
    run_spec: &'a CalibrationRunSpec,
    calendar_registry: &'a ExchangeCalendarRegistry,
    return_components_input: CalibrationStageSelectedCandidateSource,
    hawkes_stage_payload: &'a HawkesStageFittedPayload,
    return_components_stage_payload: &'a ReturnComponentsStageFittedPayload,
    candidate_generation_input: &'a ReturnComponentsStageCandidateGenerationInput,
    coarse_data_records: Option<&'a [MarketDataNormalizedCoarseDataRecord]>,
    tick_objective_data_usage_policy: CalibrationTickObjectiveDataUsagePolicy,
    solver_settings: &'a CalibrationOptimizerSettings,
    progress_sender: Option<&'a Sender<LongVarianceStageProgressUpdate>>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LongVarianceStageExecutionInput {
    pub return_components_input: Option<CalibrationStageSelectedCandidateSource>,
    pub hawkes_stage_payload: Option<HawkesStageFittedPayload>,
    pub return_components_stage_payload: Option<ReturnComponentsStageFittedPayload>,
    pub candidate_generation_input: Option<ReturnComponentsStageCandidateGenerationInput>,
    pub coarse_data_records: Option<Vec<MarketDataNormalizedCoarseDataRecord>>,
    pub tick_objective_data_usage_policy: CalibrationTickObjectiveDataUsagePolicy,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LongVarianceStageFittedPayload {
    pub schema_version: &'static str,
    pub fitted_parameters: LongVarianceParameters,
    pub diagnostics: LongVarianceStageDiagnostics,
    pub return_components_input_transport: CalibrationStageHandoffTransport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_components_selected_candidate_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_components_artifact_reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LongVarianceStageExecutionMetadata {
    pub stage_output_schema_version: &'static str,
    pub run_global_seed: u64,
    pub stage_seed: u64,
    pub stage_seed_source: CalibrationStageSeedSource,
    pub stage_seed_derivation_policy: &'static str,
    pub solver_settings: CalibrationOptimizerSettings,
    pub horizon_target_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "failure_kind", rename_all = "snake_case")]
pub enum LongVarianceStageExecutionFailure {
    InvalidRunSpec { error_message: String },
    StageIsNotSelectedInRunSpec,
    MissingStageSolverSettings,
    MissingReturnComponentsInput,
    MissingHawkesStagePayload,
    InvalidHawkesStagePayload { error_message: String },
    MissingReturnComponentsStagePayload,
    MissingCandidateGenerationInput,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LongVarianceStageExecutionReport {
    pub schema_version: &'static str,
    pub stage: CalibrationStage,
    pub status: CalibrationStageExecutionStatus,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub handoff_policy: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fitted_stage_payload: Option<LongVarianceStageFittedPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<LongVarianceStageExecutionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<LongVarianceStageExecutionFailure>,
}

pub fn run_long_variance_calibration_stage(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    execution_input: LongVarianceStageExecutionInput,
) -> LongVarianceStageExecutionReport {
    run_long_variance_calibration_stage_internal(run_spec, calendar_registry, execution_input, None)
}

pub(crate) fn run_long_variance_calibration_stage_with_progress_observer(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    execution_input: LongVarianceStageExecutionInput,
    progress_observer: Option<&mut dyn FnMut(LongVarianceStageProgressUpdate)>,
) -> LongVarianceStageExecutionReport {
    let Some(progress_observer) = progress_observer else {
        return run_long_variance_calibration_stage_internal(
            run_spec,
            calendar_registry,
            execution_input,
            None,
        );
    };

    let (progress_sender, progress_receiver) = mpsc::channel::<LongVarianceStageProgressUpdate>();

    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            run_long_variance_calibration_stage_internal(
                run_spec,
                calendar_registry,
                execution_input,
                Some(progress_sender),
            )
        });

        for progress_update in progress_receiver {
            progress_observer(progress_update);
        }

        match worker.join() {
            Ok(report) => report,
            Err(_) => failure_report(
                run_spec,
                LongVarianceStageExecutionFailure::InvalidHawkesStagePayload {
                    error_message: "long variance progress worker terminated unexpectedly"
                        .to_string(),
                },
            ),
        }
    })
}

fn run_long_variance_calibration_stage_internal(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    execution_input: LongVarianceStageExecutionInput,
    progress_sender: Option<Sender<LongVarianceStageProgressUpdate>>,
) -> LongVarianceStageExecutionReport {
    emit_long_variance_progress(
        progress_sender.as_ref(),
        LongVarianceStageProgressPhase::InputPreflight,
        0,
        6,
        Some("validate run spec"),
    );

    if let Err(error_message) = run_spec.validate() {
        return failure_report(
            run_spec,
            LongVarianceStageExecutionFailure::InvalidRunSpec {
                error_message: error_message.to_string(),
            },
        );
    }

    emit_long_variance_progress(
        progress_sender.as_ref(),
        LongVarianceStageProgressPhase::InputPreflight,
        1,
        6,
        Some("validate selected stages"),
    );

    if !run_spec
        .selected_stages
        .contains(&CalibrationStage::LongVariance)
    {
        return failure_report(
            run_spec,
            LongVarianceStageExecutionFailure::StageIsNotSelectedInRunSpec,
        );
    }

    emit_long_variance_progress(
        progress_sender.as_ref(),
        LongVarianceStageProgressPhase::InputPreflight,
        2,
        6,
        Some("resolve solver settings"),
    );

    let Some(solver_settings) =
        stage_solver_settings_for_stage(run_spec, CalibrationStage::LongVariance)
    else {
        return failure_report(
            run_spec,
            LongVarianceStageExecutionFailure::MissingStageSolverSettings,
        );
    };

    emit_long_variance_progress(
        progress_sender.as_ref(),
        LongVarianceStageProgressPhase::InputPreflight,
        3,
        6,
        Some("resolve return components input"),
    );

    let Some(return_components_input) = execution_input.return_components_input else {
        return failure_report(
            run_spec,
            LongVarianceStageExecutionFailure::MissingReturnComponentsInput,
        );
    };

    emit_long_variance_progress(
        progress_sender.as_ref(),
        LongVarianceStageProgressPhase::InputPreflight,
        4,
        6,
        Some("resolve upstream payloads"),
    );

    let Some(hawkes_stage_payload) = execution_input.hawkes_stage_payload else {
        return failure_report(
            run_spec,
            LongVarianceStageExecutionFailure::MissingHawkesStagePayload,
        );
    };

    let Some(return_components_stage_payload) = execution_input.return_components_stage_payload
    else {
        return failure_report(
            run_spec,
            LongVarianceStageExecutionFailure::MissingReturnComponentsStagePayload,
        );
    };

    let Some(candidate_generation_input) = execution_input.candidate_generation_input else {
        return failure_report(
            run_spec,
            LongVarianceStageExecutionFailure::MissingCandidateGenerationInput,
        );
    };

    emit_long_variance_progress(
        progress_sender.as_ref(),
        LongVarianceStageProgressPhase::InputPreflight,
        6,
        6,
        Some("ready"),
    );

    let fitted_stage_payload =
        match build_long_variance_fitted_stage_payload(LongVarianceFittedStagePayloadBuildInput {
            run_spec,
            calendar_registry,
            return_components_input,
            hawkes_stage_payload: &hawkes_stage_payload,
            return_components_stage_payload: &return_components_stage_payload,
            candidate_generation_input: &candidate_generation_input,
            coarse_data_records: execution_input.coarse_data_records.as_deref(),
            tick_objective_data_usage_policy: execution_input.tick_objective_data_usage_policy,
            solver_settings: &solver_settings,
            progress_sender: progress_sender.as_ref(),
        }) {
            Ok(fitted_stage_payload) => fitted_stage_payload,
            Err(failure) => return failure_report(run_spec, failure),
        };

    let seed_metadata = janiform_calibration_artifacts::resolve_calibration_stage_seed_metadata(
        run_spec,
        CalibrationStage::LongVariance,
    );

    LongVarianceStageExecutionReport {
        schema_version: CALIBRATION_LONG_VARIANCE_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::LongVariance,
        status: CalibrationStageExecutionStatus::Success,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_LONG_VARIANCE_STAGE_HANDOFF_POLICY,
        fitted_stage_payload: Some(fitted_stage_payload),
        metadata: Some(LongVarianceStageExecutionMetadata {
            stage_output_schema_version: CALIBRATION_LONG_VARIANCE_STAGE_OUTPUT_SCHEMA_VERSION,
            run_global_seed: seed_metadata.run_global_seed,
            stage_seed: seed_metadata.stage_seed,
            stage_seed_source: seed_metadata.stage_seed_source,
            stage_seed_derivation_policy:
                janiform_calibration_artifacts::CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
            solver_settings,
            horizon_target_count: LONG_VARIANCE_HORIZON_DEFINITIONS.len(),
        }),
        failure: None,
    }
}

fn build_long_variance_fitted_stage_payload(
    input: LongVarianceFittedStagePayloadBuildInput<'_>,
) -> Result<LongVarianceStageFittedPayload, LongVarianceStageExecutionFailure> {
    let LongVarianceFittedStagePayloadBuildInput {
        run_spec,
        calendar_registry,
        return_components_input,
        hawkes_stage_payload,
        return_components_stage_payload,
        candidate_generation_input,
        coarse_data_records,
        tick_objective_data_usage_policy,
        solver_settings,
        progress_sender,
    } = input;

    let baseline_parameters = return_components_stage_payload
        .fitted_parameters
        .long_variance_parameters;

    let stationary_event_rates = long_variance_stationary_event_rates_for_hawkes_kernel(
        hawkes_stage_payload.fitted_parameters.kernel_parameters,
    )
    .ok_or_else(
        || LongVarianceStageExecutionFailure::InvalidHawkesStagePayload {
            error_message: "long variance stage requires finite stationary Hawkes event rates"
                .to_string(),
        },
    )?;

    emit_long_variance_progress(
        progress_sender,
        LongVarianceStageProgressPhase::IntervalSourcePreparation,
        0,
        5,
        Some("prepare retained returns"),
    );

    let observed_intervals = observed_long_variance_intervals(
        calendar_registry,
        run_spec.data_inputs.exchange_code.as_str(),
        candidate_generation_input,
        coarse_data_records,
        tick_objective_data_usage_policy,
        progress_sender,
    );

    emit_long_variance_progress(
        progress_sender,
        LongVarianceStageProgressPhase::ParameterEstimation,
        0,
        1,
        Some("estimate long variance parameters"),
    );

    let parameter_estimation = estimate_long_variance_parameters(
        baseline_parameters,
        stationary_event_rates,
        observed_intervals.hourly_return_intervals.as_slice(),
        observed_intervals.hourly_activity_intervals.as_slice(),
    );

    emit_long_variance_progress(
        progress_sender,
        LongVarianceStageProgressPhase::ParameterEstimation,
        1,
        1,
        Some("parameters estimated"),
    );

    let horizon_summaries = long_variance_horizon_summaries_with_progress(
        LongVarianceHorizonInputIntervals {
            five_minute_return_intervals: observed_intervals
                .five_minute_return_intervals
                .as_slice(),
            five_minute_activity_intervals: observed_intervals
                .five_minute_activity_intervals
                .as_slice(),
            hourly_return_intervals: observed_intervals.hourly_return_intervals.as_slice(),
            hourly_activity_intervals: observed_intervals.hourly_activity_intervals.as_slice(),
        },
        solver_settings.cpu_count,
        progress_sender,
    );

    emit_long_variance_progress(
        progress_sender,
        LongVarianceStageProgressPhase::ReportAssembly,
        0,
        2,
        Some("build diagnostics"),
    );

    let diagnostics = long_variance_stage_diagnostics(
        selected_candidate_id(&return_components_input, return_components_stage_payload),
        horizon_summaries.as_slice(),
        &observed_intervals,
        &parameter_estimation.diagnostics,
    );

    emit_long_variance_progress(
        progress_sender,
        LongVarianceStageProgressPhase::ReportAssembly,
        1,
        2,
        Some("resolve handoff metadata"),
    );

    let return_components_input_transport = return_components_input.transport();

    let (return_components_selected_candidate_id, return_components_artifact_reference) =
        match return_components_input {
            CalibrationStageSelectedCandidateSource::InMemoryOutput {
                selected_candidate_id,
            } => (Some(selected_candidate_id), None),
            CalibrationStageSelectedCandidateSource::PersistedArtifactReference {
                artifact_reference,
            } => (None, Some(artifact_reference)),
        };

    emit_long_variance_progress(
        progress_sender,
        LongVarianceStageProgressPhase::ReportAssembly,
        2,
        2,
        Some("report ready"),
    );

    Ok(LongVarianceStageFittedPayload {
        schema_version: CALIBRATION_LONG_VARIANCE_STAGE_OUTPUT_SCHEMA_VERSION,
        fitted_parameters: parameter_estimation.parameters,
        diagnostics,
        return_components_input_transport,
        return_components_selected_candidate_id,
        return_components_artifact_reference,
    })
}

fn selected_candidate_id<'a>(
    return_components_input: &'a CalibrationStageSelectedCandidateSource,
    return_components_stage_payload: &'a ReturnComponentsStageFittedPayload,
) -> &'a str {
    return_components_input.selected_candidate_id().unwrap_or(
        return_components_stage_payload
            .selected_candidate_id
            .as_str(),
    )
}

fn long_variance_horizon_summaries_with_progress(
    input_intervals: LongVarianceHorizonInputIntervals<'_>,
    cpu_count: usize,
    progress_sender: Option<&Sender<LongVarianceStageProgressUpdate>>,
) -> Vec<LongVarianceHorizonMomentSummary> {
    let computations = long_variance_horizon_summary_computations(input_intervals);
    let total_units = computations.len();

    emit_long_variance_progress(
        progress_sender,
        LongVarianceStageProgressPhase::HorizonDiagnostics,
        0,
        total_units,
        Some("start horizon diagnostics"),
    );

    if computations.is_empty() {
        return Vec::new();
    }

    let thread_pool = build_stage_thread_pool(cpu_count);
    let worker_count = bounded_stage_worker_count(cpu_count, total_units);

    let mut outputs = Vec::with_capacity(total_units);

    run_static_work_pool(
        thread_pool.as_deref(),
        StaticWorkPoolInput::new(
            &mut outputs,
            CancellationToken::uncancelled(),
            computations,
            worker_count,
            |computation: LongVarianceHorizonSummaryComputation, _cancellation_token| {
                compute_long_variance_horizon_summary(computation)
            },
            |outputs: &mut Vec<(usize, LongVarianceHorizonMomentSummary)>,
             output: (usize, LongVarianceHorizonMomentSummary),
             status: DynamicWorkPoolStatus| {
                let horizon_label = LONG_VARIANCE_HORIZON_DEFINITIONS
                    .get(output.0)
                    .map(|definition| definition.label)
                    .unwrap_or("horizon");

                outputs.push(output);

                emit_long_variance_progress(
                    progress_sender,
                    LongVarianceStageProgressPhase::HorizonDiagnostics,
                    status.completed_work_count,
                    total_units,
                    Some(horizon_label),
                );
            },
        ),
    );

    ordered_long_variance_horizon_summaries_from_outputs(outputs)
}

fn long_variance_stage_diagnostics(
    selected_candidate_id: &str,
    horizon_summaries: &[LongVarianceHorizonMomentSummary],
    observed_intervals: &ObservedLongVarianceIntervals,
    parameter_estimation_diagnostics: &LongVarianceParameterEstimationDiagnostics,
) -> LongVarianceStageDiagnostics {
    LongVarianceStageDiagnostics {
        estimation_policy: LONG_VARIANCE_ESTIMATION_POLICY.to_string(),
        stationary_normalization_policy: LONG_VARIANCE_STATIONARY_NORMALIZATION_POLICY.to_string(),
        interval_source_selection_policy: LONG_VARIANCE_INTERVAL_SOURCE_SELECTION_POLICY
            .to_string(),
        return_interval_tick_count: observed_intervals.return_interval_tick_count,
        return_interval_coarse_candidate_count: observed_intervals
            .return_interval_coarse_candidate_count,
        return_interval_coarse_gap_fill_count: observed_intervals
            .return_interval_coarse_gap_fill_count,
        return_interval_tick_coarse_overlap_count: observed_intervals
            .return_interval_tick_coarse_overlap_count,
        return_interval_count: observed_intervals.hourly_return_intervals.len(),
        activity_interval_count: observed_intervals.hourly_activity_intervals.len(),
        persistence_lag_diagnostics: parameter_estimation_diagnostics
            .persistence_lag_diagnostics
            .iter()
            .map(|diagnostic| LongVarianceStagePersistenceLagDiagnostic {
                lag_hours: diagnostic.lag_hours,
                pair_count: diagnostic.pair_count,
                correlation: diagnostic.correlation,
                implied_mean_reversion_rate: diagnostic.implied_mean_reversion_rate,
            })
            .collect(),
        selected_mean_reversion_rate: parameter_estimation_diagnostics.selected_mean_reversion_rate,
        event_loading_fit_candidate_pair_count: parameter_estimation_diagnostics
            .event_loading_fit_candidate_pair_count,
        event_loading_fit_pair_count: parameter_estimation_diagnostics.event_loading_fit_pair_count,
        event_loading_fit_coverage_ratio: parameter_estimation_diagnostics
            .event_loading_fit_coverage_ratio,
        selected_candidate_id: selected_candidate_id.to_string(),
        horizon_diagnostics: LONG_VARIANCE_HORIZON_DEFINITIONS
            .iter()
            .zip(horizon_summaries.iter())
            .map(
                |(horizon_definition, summary)| LongVarianceStageHorizonDiagnostic {
                    horizon_label: horizon_definition.label.to_string(),
                    horizon_hours: horizon_definition.hours,
                    sample_count: summary.sample_count,
                    realized_variance: summary.realized_variance,
                    variance_of_variance: summary.variance_of_variance,
                    return_skewness: summary.return_skewness,
                    return_excess_kurtosis: summary.return_excess_kurtosis,
                    activity_variance_correlation: summary.activity_variance_correlation,
                    signed_activity_future_variance_relation: summary
                        .signed_activity_future_variance_relation,
                    realized_variance_standard_error: summary.standard_errors.realized_variance,
                    variance_of_variance_standard_error: summary
                        .standard_errors
                        .variance_of_variance,
                    return_skewness_standard_error: summary.standard_errors.return_skewness,
                    return_excess_kurtosis_standard_error: summary
                        .standard_errors
                        .return_excess_kurtosis,
                    activity_variance_correlation_standard_error: summary
                        .standard_errors
                        .activity_variance_correlation,
                    signed_activity_future_variance_relation_standard_error: summary
                        .standard_errors
                        .signed_activity_future_variance_relation,
                },
            )
            .collect(),
    }
}

#[derive(Debug, Clone, PartialEq)]
struct ObservedLongVarianceIntervals {
    five_minute_return_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    five_minute_activity_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    hourly_return_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    hourly_activity_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    return_interval_tick_count: usize,
    return_interval_coarse_candidate_count: usize,
    return_interval_coarse_gap_fill_count: usize,
    return_interval_tick_coarse_overlap_count: usize,
}

fn observed_long_variance_intervals(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    coarse_data_records: Option<&[MarketDataNormalizedCoarseDataRecord]>,
    tick_objective_data_usage_policy: CalibrationTickObjectiveDataUsagePolicy,
    progress_sender: Option<&Sender<LongVarianceStageProgressUpdate>>,
) -> ObservedLongVarianceIntervals {
    let observed_returns = retained_open_session_returns_for_tick_objectives(
        candidate_generation_input,
        tick_objective_data_usage_policy,
    );

    let source_window = source_window_for_retained_tick_objectives(
        candidate_generation_input,
        observed_returns.as_slice(),
    );

    emit_long_variance_progress(
        progress_sender,
        LongVarianceStageProgressPhase::IntervalSourcePreparation,
        1,
        5,
        Some("build five-minute intervals"),
    );

    let five_minute_tick_intervals = build_observed_open_session_interval_observations(
        calendar_registry,
        exchange_code,
        source_window,
        observed_returns.as_slice(),
        NANOS_PER_FIVE_MINUTES,
    );

    emit_long_variance_progress(
        progress_sender,
        LongVarianceStageProgressPhase::IntervalSourcePreparation,
        2,
        5,
        Some("build tick hourly intervals"),
    );

    let tick_hourly = build_observed_open_session_interval_observations(
        calendar_registry,
        exchange_code,
        source_window,
        observed_returns.as_slice(),
        NANOS_PER_HOUR,
    );

    emit_long_variance_progress(
        progress_sender,
        LongVarianceStageProgressPhase::IntervalSourcePreparation,
        3,
        5,
        Some("select return and activity intervals"),
    );

    let observed_interval_selection =
        select_long_variance_observed_intervals(tick_hourly.as_slice(), coarse_data_records);

    emit_long_variance_progress(
        progress_sender,
        LongVarianceStageProgressPhase::IntervalSourcePreparation,
        4,
        5,
        Some("return and activity intervals selected"),
    );

    emit_long_variance_progress(
        progress_sender,
        LongVarianceStageProgressPhase::IntervalSourcePreparation,
        5,
        5,
        Some("interval sources ready"),
    );

    // Five-minute tick intervals back both streams, while hourly sources diverge by policy.
    let five_minute_activity_intervals = five_minute_tick_intervals.clone();

    ObservedLongVarianceIntervals {
        five_minute_return_intervals: five_minute_tick_intervals,
        five_minute_activity_intervals,
        hourly_return_intervals: observed_interval_selection.return_intervals,
        hourly_activity_intervals: observed_interval_selection.activity_intervals,
        return_interval_tick_count: observed_interval_selection.return_interval_tick_count,
        return_interval_coarse_candidate_count: observed_interval_selection
            .return_interval_coarse_candidate_count,
        return_interval_coarse_gap_fill_count: observed_interval_selection
            .return_interval_coarse_gap_fill_count,
        return_interval_tick_coarse_overlap_count: observed_interval_selection
            .return_interval_tick_coarse_overlap_count,
    }
}

fn failure_report(
    run_spec: &CalibrationRunSpec,
    failure: LongVarianceStageExecutionFailure,
) -> LongVarianceStageExecutionReport {
    LongVarianceStageExecutionReport {
        schema_version: CALIBRATION_LONG_VARIANCE_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::LongVariance,
        status: CalibrationStageExecutionStatus::Failure,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_LONG_VARIANCE_STAGE_HANDOFF_POLICY,
        fitted_stage_payload: None,
        metadata: None,
        failure: Some(failure),
    }
}

fn emit_long_variance_progress(
    progress_sender: Option<&Sender<LongVarianceStageProgressUpdate>>,
    phase: LongVarianceStageProgressPhase,
    completed_units: usize,
    total_units: usize,
    current_item_label: Option<&str>,
) {
    emit_stage_progress(
        progress_sender,
        StageProgressUpdate {
            current_item_label: current_item_label.map(str::to_string),
            ..StageProgressUpdate::new(phase, completed_units, total_units)
        },
    );
}
