use std::sync::mpsc;
use std::sync::mpsc::Sender;

use serde::Serialize;

use janiform_calibration_artifacts::{
    CALIBRATION_MEAN_REVERSION_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_MEAN_REVERSION_STAGE_HANDOFF_POLICY,
    CALIBRATION_MEAN_REVERSION_STAGE_OUTPUT_SCHEMA_VERSION, CalibrationExecutionMode,
    CalibrationOptimizerSettings, CalibrationRunSpec, CalibrationStage,
    CalibrationStageHandoffTransport, CalibrationStageSeedSource,
    CalibrationStageSelectedCandidateSource, MeanReversionStageCalibrationGrid,
    MeanReversionStageCandidateDiagnostics, MeanReversionStageCandidateSeriesDiagnostics,
    MeanReversionStageDiagnostics, MeanReversionStageGridDiagnostics,
    MeanReversionStageSeriesSummary,
};
use janiform_foundations::linear_algebra::{
    RankAwareLeastSquaresSolution, rank_aware_one_column_least_squares,
};
use janiform_foundations::numeric::{GOLDEN_RATIO, coverage_ratio};
use janiform_market_time::calendar::ExchangeCalendarRegistry;
use janiform_model_kernels::mean_reversion::MeanReversionFittedParameters;

use crate::stages::hawkes::HawkesStageFittedPayload;
use crate::stages::model_selection::bayesian_information_criterion;
use crate::stages::progress::{StageProgressUpdate, emit_stage_progress};
use crate::stages::return_components::ReturnComponentsStageFittedPayload;
use crate::stages::seasonality::{
    CalibrationStageExecutionStatus, FourierBusinessTimeClockModel,
    SeasonalityStageNormalizedPayload,
};

mod evidence;
mod progress;

pub use progress::{MeanReversionStageProgressPhase, MeanReversionStageProgressUpdate};

const FLAT_ANCHOR_MEAN_REVERSION_IDENTIFIABLE_PARAMETER_COUNT: usize = 2;
const FLAT_ANCHOR_MEAN_REVERSION_CANDIDATE_FAMILY: &str = "flat_anchor_mean_reversion";
const RANDOM_WALK_NULL_CANDIDATE_FAMILY: &str = "random_walk_null";
const FLAT_ANCHOR_MEAN_REVERSION_REASON: &str =
    "flat_anchor_mean_reversion_profiled_over_phi_reference.v1";
const RANDOM_WALK_NULL_REASON: &str = "random_walk_null_baseline.v1";
const ENDOGENOUS_RUNTIME_PROJECTION_REQUIRED_REASON: &str =
    "endogenous_anchor_runtime_projection_required.v1";
const NO_MEAN_REVERSION_GRID_CANDIDATE_REASON: &str = "mean_reversion_grid_candidate_required.v1";

pub(crate) fn mean_reversion_full_model_required_source_observation_count() -> usize {
    mean_reversion_flat_anchor_required_source_observation_count()
}

pub(crate) fn mean_reversion_random_walk_required_source_observation_count() -> usize {
    mean_reversion_flat_anchor_required_source_observation_count()
}

fn mean_reversion_flat_anchor_required_source_observation_count() -> usize {
    // The flat-anchor mean-reversion candidate estimates both an anchor level and a decay. A
    // single transition can fit that pair exactly, so two transitions are the first identifiable
    // case for this candidate family.
    FLAT_ANCHOR_MEAN_REVERSION_IDENTIFIABLE_PARAMETER_COUNT.saturating_add(1)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeanReversionStageObservation {
    pub observation_timestamp_utc_nanos: i64,
    pub observed_log_price: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeanReversionStageSeasonalityInput {
    pub exchange_code: String,
    pub clock_model: FourierBusinessTimeClockModel,
}

impl MeanReversionStageSeasonalityInput {
    pub fn from_seasonality_stage_payload(payload: &SeasonalityStageNormalizedPayload) -> Self {
        Self {
            exchange_code: payload.exchange_code.clone(),
            clock_model: payload.clock_model.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeanReversionStageGridCandidate {
    pub calibration_grid_kind: String,
    pub calibration_grid_interval_hours: f64,
    pub source_observation_count: usize,
    pub coverage_ratio: f64,
    pub observations: Vec<MeanReversionStageObservation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeanReversionStageExecutionInput {
    pub return_components_input: Option<CalibrationStageSelectedCandidateSource>,
    pub seasonality_stage_input: Option<MeanReversionStageSeasonalityInput>,
    pub hawkes_stage_payload: Option<HawkesStageFittedPayload>,
    pub return_components_stage_payload: Option<ReturnComponentsStageFittedPayload>,
    pub grid_candidates: Vec<MeanReversionStageGridCandidate>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MeanReversionStageFittedPayload {
    pub schema_version: &'static str,
    pub fitted_parameters: MeanReversionFittedParameters,
    pub diagnostics: MeanReversionStageDiagnostics,
    pub return_components_input_transport: CalibrationStageHandoffTransport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_components_selected_candidate_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_components_artifact_reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MeanReversionStageExecutionMetadata {
    pub stage_output_schema_version: &'static str,
    pub run_global_seed: u64,
    pub stage_seed: u64,
    pub stage_seed_source: CalibrationStageSeedSource,
    pub stage_seed_derivation_policy: &'static str,
    pub solver_settings: CalibrationOptimizerSettings,
    pub source_observation_count: usize,
    pub retained_observation_count: usize,
    pub coverage_ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "failure_kind", rename_all = "snake_case")]
pub enum MeanReversionStageExecutionFailure {
    InvalidRunSpec { error_message: String },
    StageIsNotSelectedInRunSpec,
    MissingStageSolverSettings,
    MissingReturnComponentsInput,
    MissingSeasonalityStagePayload,
    MissingHawkesStagePayload,
    MissingReturnComponentsStagePayload,
    InvalidObservationTimestampGrid { error_message: String },
    IntervalFactorGridMaterializationFailed { error_message: String },
    EndogenousAnchorMaterializationFailed { error_message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MeanReversionStageExecutionReport {
    pub schema_version: &'static str,
    pub stage: CalibrationStage,
    pub status: CalibrationStageExecutionStatus,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub handoff_policy: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fitted_stage_payload: Option<MeanReversionStageFittedPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<MeanReversionStageExecutionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<MeanReversionStageExecutionFailure>,
}

pub fn run_mean_reversion_calibration_stage(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    execution_input: MeanReversionStageExecutionInput,
) -> MeanReversionStageExecutionReport {
    run_mean_reversion_calibration_stage_internal(
        run_spec,
        calendar_registry,
        execution_input,
        None,
    )
}

pub(crate) fn run_mean_reversion_calibration_stage_with_progress_observer(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    execution_input: MeanReversionStageExecutionInput,
    progress_observer: Option<&mut dyn FnMut(MeanReversionStageProgressUpdate)>,
) -> MeanReversionStageExecutionReport {
    let Some(progress_observer) = progress_observer else {
        return run_mean_reversion_calibration_stage_internal(
            run_spec,
            calendar_registry,
            execution_input,
            None,
        );
    };

    let (progress_sender, progress_receiver) = mpsc::channel::<MeanReversionStageProgressUpdate>();

    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            run_mean_reversion_calibration_stage_internal(
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
                MeanReversionStageExecutionFailure::InvalidRunSpec {
                    error_message: "mean reversion progress worker terminated unexpectedly"
                        .to_string(),
                },
            ),
        }
    })
}

fn run_mean_reversion_calibration_stage_internal(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    execution_input: MeanReversionStageExecutionInput,
    progress_sender: Option<Sender<MeanReversionStageProgressUpdate>>,
) -> MeanReversionStageExecutionReport {
    emit_mean_reversion_progress(
        progress_sender.as_ref(),
        MeanReversionStageProgressPhase::InputPreflight,
        0,
        4,
        Some("validate run spec"),
    );

    if let Err(error_message) = run_spec.validate() {
        return failure_report(
            run_spec,
            MeanReversionStageExecutionFailure::InvalidRunSpec {
                error_message: error_message.to_string(),
            },
        );
    }

    emit_mean_reversion_progress(
        progress_sender.as_ref(),
        MeanReversionStageProgressPhase::InputPreflight,
        1,
        4,
        Some("validate selected stages"),
    );

    if !run_spec
        .selected_stages
        .contains(&CalibrationStage::MeanReversion)
    {
        return failure_report(
            run_spec,
            MeanReversionStageExecutionFailure::StageIsNotSelectedInRunSpec,
        );
    }

    emit_mean_reversion_progress(
        progress_sender.as_ref(),
        MeanReversionStageProgressPhase::InputPreflight,
        2,
        4,
        Some("resolve solver settings"),
    );

    let Some(solver_settings) = janiform_calibration_artifacts::stage_solver_settings_for_stage(
        run_spec,
        CalibrationStage::MeanReversion,
    ) else {
        return failure_report(
            run_spec,
            MeanReversionStageExecutionFailure::MissingStageSolverSettings,
        );
    };

    emit_mean_reversion_progress(
        progress_sender.as_ref(),
        MeanReversionStageProgressPhase::InputPreflight,
        3,
        4,
        Some("resolve return components input"),
    );

    let seed_metadata = janiform_calibration_artifacts::resolve_calibration_stage_seed_metadata(
        run_spec,
        CalibrationStage::MeanReversion,
    );

    let Some(return_components_input) = execution_input.return_components_input.as_ref() else {
        return failure_report(
            run_spec,
            MeanReversionStageExecutionFailure::MissingReturnComponentsInput,
        );
    };

    emit_mean_reversion_progress(
        progress_sender.as_ref(),
        MeanReversionStageProgressPhase::InputPreflight,
        4,
        4,
        Some("ready"),
    );

    let (fitted_parameters, diagnostics) = match estimate_mean_reversion_fitted_parameters(
        calendar_registry,
        &execution_input,
        &solver_settings,
        progress_sender.as_ref(),
    ) {
        Ok(result) => result,
        Err(failure) => return failure_report(run_spec, failure),
    };

    let (return_components_selected_candidate_id, return_components_artifact_reference) =
        match return_components_input {
            CalibrationStageSelectedCandidateSource::InMemoryOutput {
                selected_candidate_id,
            } => (Some(selected_candidate_id.clone()), None),
            CalibrationStageSelectedCandidateSource::PersistedArtifactReference {
                artifact_reference,
            } => (None, Some(artifact_reference.clone())),
        };

    MeanReversionStageExecutionReport {
        schema_version: CALIBRATION_MEAN_REVERSION_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::MeanReversion,
        status: CalibrationStageExecutionStatus::Success,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_MEAN_REVERSION_STAGE_HANDOFF_POLICY,
        fitted_stage_payload: Some(MeanReversionStageFittedPayload {
            schema_version: CALIBRATION_MEAN_REVERSION_STAGE_OUTPUT_SCHEMA_VERSION,
            fitted_parameters,
            diagnostics: diagnostics.clone(),
            return_components_input_transport: return_components_input.transport(),
            return_components_selected_candidate_id,
            return_components_artifact_reference,
        }),
        metadata: Some(MeanReversionStageExecutionMetadata {
            stage_output_schema_version: CALIBRATION_MEAN_REVERSION_STAGE_OUTPUT_SCHEMA_VERSION,
            run_global_seed: seed_metadata.run_global_seed,
            stage_seed: seed_metadata.stage_seed,
            stage_seed_source: seed_metadata.stage_seed_source,
            stage_seed_derivation_policy:
                janiform_calibration_artifacts::CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
            solver_settings,
            source_observation_count: diagnostics.calibration_grid.source_observation_count,
            retained_observation_count: diagnostics.calibration_grid.retained_observation_count,
            coverage_ratio: diagnostics.calibration_grid.coverage_ratio,
        }),
        failure: None,
    }
}

fn failure_report(
    run_spec: &CalibrationRunSpec,
    failure: MeanReversionStageExecutionFailure,
) -> MeanReversionStageExecutionReport {
    MeanReversionStageExecutionReport {
        schema_version: CALIBRATION_MEAN_REVERSION_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::MeanReversion,
        status: CalibrationStageExecutionStatus::Failure,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_MEAN_REVERSION_STAGE_HANDOFF_POLICY,
        fitted_stage_payload: None,
        metadata: None,
        failure: Some(failure),
    }
}

fn estimate_mean_reversion_fitted_parameters(
    calendar_registry: &ExchangeCalendarRegistry,
    execution_input: &MeanReversionStageExecutionInput,
    solver_settings: &CalibrationOptimizerSettings,
    progress_sender: Option<&Sender<MeanReversionStageProgressUpdate>>,
) -> Result<
    (MeanReversionFittedParameters, MeanReversionStageDiagnostics),
    MeanReversionStageExecutionFailure,
> {
    if execution_input.grid_candidates.is_empty() {
        return Err(MeanReversionStageExecutionFailure::InvalidRunSpec {
            error_message: "mean-reversion stage requires at least one calibration grid candidate"
                .to_string(),
        });
    }

    let mut grid_evaluations = Vec::with_capacity(execution_input.grid_candidates.len());
    let grid_candidate_count = execution_input.grid_candidates.len();

    emit_mean_reversion_progress(
        progress_sender,
        MeanReversionStageProgressPhase::GridEvaluation,
        0,
        grid_candidate_count,
        Some("start grid evaluation"),
    );

    for (grid_position, grid_candidate) in execution_input.grid_candidates.iter().enumerate() {
        grid_evaluations.push(evaluate_mean_reversion_grid_candidate(
            calendar_registry,
            execution_input,
            solver_settings,
            grid_candidate,
        )?);

        emit_mean_reversion_progress(
            progress_sender,
            MeanReversionStageProgressPhase::GridEvaluation,
            grid_position.saturating_add(1),
            grid_candidate_count,
            Some(grid_candidate.calibration_grid_kind.as_str()),
        );
    }

    emit_mean_reversion_progress(
        progress_sender,
        MeanReversionStageProgressPhase::CandidateSelection,
        0,
        2,
        Some("rank BIC candidates"),
    );

    let Some(selection) =
        evidence::select_mean_reversion_candidate(grid_evaluations.as_mut_slice())
    else {
        return zero_kappa_endogenous_fit_with_grid_evaluations(
            grid_evaluations,
            NO_MEAN_REVERSION_GRID_CANDIDATE_REASON,
        );
    };

    emit_mean_reversion_progress(
        progress_sender,
        MeanReversionStageProgressPhase::CandidateSelection,
        1,
        2,
        Some("assemble selected candidate"),
    );

    let Some(selected_grid) = grid_evaluations.get(selection.grid_index) else {
        return Err(MeanReversionStageExecutionFailure::InvalidRunSpec {
            error_message: "mean-reversion selection referenced a missing evaluated grid"
                .to_string(),
        });
    };

    let Some(selected_candidate) = selected_grid
        .evaluated_candidates
        .get(selection.candidate_index)
    else {
        return Err(MeanReversionStageExecutionFailure::InvalidRunSpec {
            error_message: "mean-reversion selection referenced a missing candidate".to_string(),
        });
    };

    let fitted_parameters = MeanReversionFittedParameters::try_new(selected_candidate.kappa)
        .map_err(
            |source| MeanReversionStageExecutionFailure::InvalidRunSpec {
                error_message: format!("selected mean-reversion candidate is invalid: {source}"),
            },
        )?;

    emit_mean_reversion_progress(
        progress_sender,
        MeanReversionStageProgressPhase::ReportAssembly,
        0,
        2,
        Some("build diagnostics"),
    );

    let diagnostics = MeanReversionStageDiagnostics {
        selected_candidate_family: selected_candidate.candidate_family.to_string(),
        calibration_grid: selected_grid.calibration_grid.clone(),
        anchor_origin_level: selected_candidate.anchor_origin_level,
        phi_reference: selected_candidate.phi_reference,
        kappa: selected_candidate.kappa,
        residual_sum_of_squares: selected_candidate.residual_sum_of_squares,
        quasi_log_likelihood: finite_metric_or_none(selected_candidate.quasi_log_likelihood),
        information_criterion_bic: finite_metric_or_none(
            selected_candidate.information_criterion_bic,
        ),
        model_selection_policy: selection.model_selection_policy.to_string(),
        deviation_sample_count: selected_candidate.deviations.len(),
        regression_pair_count: selected_candidate.residuals.len(),
        least_squares_rank: selected_candidate.least_squares_rank,
        evaluated_candidates: selected_grid
            .evaluated_candidates
            .iter()
            .map(MeanReversionStageCandidateEvaluation::to_diagnostics)
            .collect(),
        evaluated_grids: grid_evaluations
            .iter()
            .map(MeanReversionGridCandidateEvaluation::to_diagnostics)
            .collect(),
        stage_decision_reason: selected_mean_reversion_stage_decision_reason(
            selected_candidate,
            &selection,
        ),
    };

    emit_mean_reversion_progress(
        progress_sender,
        MeanReversionStageProgressPhase::ReportAssembly,
        1,
        2,
        Some("diagnostics built"),
    );

    emit_mean_reversion_progress(
        progress_sender,
        MeanReversionStageProgressPhase::ReportAssembly,
        2,
        2,
        Some("report ready"),
    );

    Ok((fitted_parameters, diagnostics))
}

fn selected_mean_reversion_stage_decision_reason(
    selected_candidate: &MeanReversionStageCandidateEvaluation,
    selection: &evidence::MeanReversionCandidateSelection,
) -> String {
    if selected_candidate.stage_decision_reason == ENDOGENOUS_RUNTIME_PROJECTION_REQUIRED_REASON {
        return selected_candidate.stage_decision_reason.clone();
    }

    format!(
        "{}|{}",
        selected_candidate.stage_decision_reason, selection.stage_decision_reason
    )
}

fn evaluate_mean_reversion_grid_candidate(
    calendar_registry: &ExchangeCalendarRegistry,
    execution_input: &MeanReversionStageExecutionInput,
    solver_settings: &CalibrationOptimizerSettings,
    grid_candidate: &MeanReversionStageGridCandidate,
) -> Result<MeanReversionGridCandidateEvaluation, MeanReversionStageExecutionFailure> {
    let calibration_grid = MeanReversionStageCalibrationGrid {
        calibration_grid_kind: grid_candidate.calibration_grid_kind.clone(),
        calibration_grid_interval_hours: grid_candidate.calibration_grid_interval_hours,
        source_observation_count: grid_candidate.source_observation_count,
        retained_observation_count: grid_candidate.observations.len(),
        coverage_ratio: grid_candidate.coverage_ratio,
    };

    let candidate_family_support = mean_reversion_candidate_family_support(grid_candidate);
    if !candidate_family_support.random_walk_null {
        return Ok(runtime_projection_required_grid_evaluation(
            calibration_grid,
        ));
    }

    let observation_levels: Vec<f64> = grid_candidate
        .observations
        .iter()
        .map(|observation| observation.observed_log_price)
        .collect();

    estimate_endogenous_mean_reversion_grid_candidate(
        calendar_registry,
        execution_input,
        solver_settings,
        calibration_grid,
        grid_candidate,
        observation_levels.as_slice(),
        candidate_family_support,
    )
}

#[derive(Debug, Clone, Copy)]
struct MeanReversionCandidateFamilySupport {
    flat_anchor_mean_reversion: bool,
    random_walk_null: bool,
}

fn mean_reversion_candidate_family_support(
    grid_candidate: &MeanReversionStageGridCandidate,
) -> MeanReversionCandidateFamilySupport {
    MeanReversionCandidateFamilySupport {
        flat_anchor_mean_reversion: mean_reversion_sample_supports_required_source_observations(
            grid_candidate,
            mean_reversion_flat_anchor_required_source_observation_count(),
        ),
        random_walk_null: mean_reversion_sample_supports_required_source_observations(
            grid_candidate,
            mean_reversion_random_walk_required_source_observation_count(),
        ),
    }
}

fn mean_reversion_sample_supports_required_source_observations(
    grid_candidate: &MeanReversionStageGridCandidate,
    required_source_observation_count: usize,
) -> bool {
    if !grid_candidate.coverage_ratio.is_finite() {
        return false;
    }

    let retained_observation_count = grid_candidate.observations.len();

    let required_minimum_coverage_ratio = coverage_ratio(
        required_source_observation_count,
        retained_observation_count,
    );

    retained_observation_count >= required_source_observation_count
        && grid_candidate.source_observation_count >= required_source_observation_count
        && grid_candidate.coverage_ratio >= required_minimum_coverage_ratio
}

#[derive(Debug, Clone)]
struct MeanReversionStageCandidateEvaluation {
    candidate_family: &'static str,
    anchor_origin_level: Option<f64>,
    phi_reference: f64,
    kappa: f64,
    residual_sum_of_squares: f64,
    quasi_log_likelihood: f64,
    information_criterion_bic: f64,
    parameter_count: usize,
    least_squares_rank: usize,
    boundary_anchor_values: Vec<f64>,
    deviations: Vec<f64>,
    residuals: Vec<f64>,
    stage_decision_reason: String,
}

impl MeanReversionStageCandidateEvaluation {
    fn to_diagnostics(&self) -> MeanReversionStageCandidateDiagnostics {
        MeanReversionStageCandidateDiagnostics {
            candidate_family: self.candidate_family.to_string(),
            anchor_origin_level: self.anchor_origin_level,
            phi_reference: self.phi_reference,
            kappa: self.kappa,
            residual_sum_of_squares: self.residual_sum_of_squares,
            quasi_log_likelihood: finite_metric_or_none(self.quasi_log_likelihood),
            information_criterion_bic: finite_metric_or_none(self.information_criterion_bic),
            parameter_count: self.parameter_count,
            least_squares_rank: self.least_squares_rank,
            deviation_sample_count: self.deviations.len(),
            regression_pair_count: self.residuals.len(),
            series_summaries: MeanReversionStageCandidateSeriesDiagnostics {
                boundary_anchor_values: MeanReversionStageSeriesSummary::from_values(
                    self.boundary_anchor_values.as_slice(),
                ),
                deviations: MeanReversionStageSeriesSummary::from_values(
                    self.deviations.as_slice(),
                ),
                residuals: MeanReversionStageSeriesSummary::from_values(self.residuals.as_slice()),
            },
            stage_decision_reason: self.stage_decision_reason.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct MeanReversionGridCandidateEvaluation {
    calibration_grid: MeanReversionStageCalibrationGrid,
    evaluated_candidates: Vec<MeanReversionStageCandidateEvaluation>,
    stage_decision_reason: String,
}

impl MeanReversionGridCandidateEvaluation {
    fn to_diagnostics(&self) -> MeanReversionStageGridDiagnostics {
        MeanReversionStageGridDiagnostics {
            calibration_grid: self.calibration_grid.clone(),
            evaluated_candidates: self
                .evaluated_candidates
                .iter()
                .map(MeanReversionStageCandidateEvaluation::to_diagnostics)
                .collect(),
            stage_decision_reason: self.stage_decision_reason.clone(),
        }
    }
}

fn estimate_endogenous_mean_reversion_grid_candidate(
    _calendar_registry: &ExchangeCalendarRegistry,
    _execution_input: &MeanReversionStageExecutionInput,
    solver_settings: &CalibrationOptimizerSettings,
    calibration_grid: MeanReversionStageCalibrationGrid,
    _grid_candidate: &MeanReversionStageGridCandidate,
    observation_levels: &[f64],
    candidate_family_support: MeanReversionCandidateFamilySupport,
) -> Result<MeanReversionGridCandidateEvaluation, MeanReversionStageExecutionFailure> {
    let mut evaluated_candidates = Vec::new();

    if candidate_family_support.flat_anchor_mean_reversion {
        evaluated_candidates.push(profile_flat_anchor_mean_reversion_candidate(
            observation_levels,
            calibration_grid.calibration_grid_interval_hours,
            solver_settings,
        )?);
    }

    evaluated_candidates.push(evaluate_random_walk_null_candidate(
        observation_levels,
        calibration_grid.calibration_grid_interval_hours,
    ));

    Ok(MeanReversionGridCandidateEvaluation {
        calibration_grid,
        evaluated_candidates,
        stage_decision_reason: "mean_reversion_grid_candidate_evaluated.v1".to_string(),
    })
}

fn profile_flat_anchor_mean_reversion_candidate(
    observation_levels: &[f64],
    calibration_grid_interval_hours: f64,
    solver_settings: &CalibrationOptimizerSettings,
) -> Result<MeanReversionStageCandidateEvaluation, MeanReversionStageExecutionFailure> {
    let phi_boundary_candidate = evaluate_flat_anchor_mean_reversion_candidate(
        observation_levels,
        calibration_grid_interval_hours,
        1.0,
    )?;

    profile_candidate_over_unit_interval(solver_settings, phi_boundary_candidate, |phi_reference| {
        evaluate_flat_anchor_mean_reversion_candidate(
            observation_levels,
            calibration_grid_interval_hours,
            phi_reference,
        )
    })
}

fn evaluate_flat_anchor_mean_reversion_candidate(
    observation_levels: &[f64],
    calibration_grid_interval_hours: f64,
    phi_reference: f64,
) -> Result<MeanReversionStageCandidateEvaluation, MeanReversionStageExecutionFailure> {
    let response = observation_levels
        .iter()
        .zip(observation_levels.iter().skip(1))
        .map(|(current_observation, next_observation)| {
            next_observation - phi_reference * current_observation
        })
        .collect::<Vec<_>>();

    let intercept_column = vec![1.0 - phi_reference; observation_levels.len().saturating_sub(1)];

    let least_squares_solution =
        solve_one_column_least_squares(intercept_column.as_slice(), response.as_slice())?;

    let anchor_origin_level = if phi_reference == 1.0 {
        None
    } else {
        Some(least_squares_solution.first_coefficient)
    };

    let anchor_value = anchor_origin_level.unwrap_or(0.0);
    let boundary_anchor_values = vec![anchor_value; observation_levels.len()];

    let deviations = observation_levels
        .iter()
        .map(|observation| observation - anchor_value)
        .collect::<Vec<_>>();

    let residuals = remove_scaled_column_contribution(
        response.as_slice(),
        intercept_column.as_slice(),
        least_squares_solution.first_coefficient,
    );

    let residual_sum_of_squares = residuals
        .iter()
        .map(|residual| residual * residual)
        .sum::<f64>();

    let quasi_log_likelihood =
        gaussian_quasi_log_likelihood(residuals.len(), residual_sum_of_squares);

    Ok(MeanReversionStageCandidateEvaluation {
        candidate_family: FLAT_ANCHOR_MEAN_REVERSION_CANDIDATE_FAMILY,
        anchor_origin_level,
        phi_reference,
        kappa: kappa_from_phi_reference(phi_reference, calibration_grid_interval_hours),
        residual_sum_of_squares,
        quasi_log_likelihood,
        information_criterion_bic: bayesian_information_criterion(
            residuals.len(),
            2,
            quasi_log_likelihood,
        ),
        parameter_count: 2,
        least_squares_rank: least_squares_solution.rank,
        boundary_anchor_values,
        deviations,
        residuals,
        stage_decision_reason: stage_decision_reason_with_phi_reference_flags(
            FLAT_ANCHOR_MEAN_REVERSION_REASON,
            Some(phi_reference),
        ),
    })
}

fn evaluate_random_walk_null_candidate(
    observation_levels: &[f64],
    _calibration_grid_interval_hours: f64,
) -> MeanReversionStageCandidateEvaluation {
    let residuals = adjacent_observation_level_differences(observation_levels);

    let residual_sum_of_squares = residuals
        .iter()
        .map(|residual| residual * residual)
        .sum::<f64>();

    let quasi_log_likelihood =
        gaussian_quasi_log_likelihood(residuals.len(), residual_sum_of_squares);

    MeanReversionStageCandidateEvaluation {
        candidate_family: RANDOM_WALK_NULL_CANDIDATE_FAMILY,
        anchor_origin_level: None,
        phi_reference: 1.0,
        kappa: 0.0,
        residual_sum_of_squares,
        quasi_log_likelihood,
        information_criterion_bic: bayesian_information_criterion(
            residuals.len(),
            0,
            quasi_log_likelihood,
        ),
        parameter_count: 0,
        least_squares_rank: 0,
        boundary_anchor_values: vec![0.0; observation_levels.len()],
        deviations: observation_levels.to_vec(),
        residuals,
        stage_decision_reason: RANDOM_WALK_NULL_REASON.to_string(),
    }
}

fn candidate_supports_quasi_likelihood_model_selection(
    candidate: &MeanReversionStageCandidateEvaluation,
) -> bool {
    // The Gaussian quasi likelihood profiles the innovation variance from the same transition
    // sample as the structural coefficients, so the candidate needs one residual degree of
    // freedom after estimating both the structural parameters and that variance term.
    candidate.residuals.len() > candidate.parameter_count.saturating_add(1)
}

fn profile_candidate_over_unit_interval<Error>(
    solver_settings: &CalibrationOptimizerSettings,
    mut best_candidate: MeanReversionStageCandidateEvaluation,
    mut evaluate_candidate_at_position: impl FnMut(
        f64,
    ) -> Result<
        MeanReversionStageCandidateEvaluation,
        Error,
    >,
) -> Result<MeanReversionStageCandidateEvaluation, Error> {
    let mut lower_position = 0.0_f64;
    let mut upper_position = 1.0_f64;

    let mut left_position = upper_position - (upper_position - lower_position) / GOLDEN_RATIO;
    let mut right_position = lower_position + (upper_position - lower_position) / GOLDEN_RATIO;

    let mut left_candidate = evaluate_candidate_at_position(left_position)?;
    let mut right_candidate = evaluate_candidate_at_position(right_position)?;

    for _ in 0..solver_settings.max_iterations {
        let width = upper_position - lower_position;
        let tolerance = solver_settings
            .absolute_tolerance
            .max(solver_settings.relative_tolerance * upper_position);

        if width <= tolerance {
            break;
        }

        if left_candidate.quasi_log_likelihood >= right_candidate.quasi_log_likelihood {
            upper_position = right_position;
            right_position = left_position;

            right_candidate = left_candidate;

            left_position = upper_position - (upper_position - lower_position) / GOLDEN_RATIO;
            left_candidate = evaluate_candidate_at_position(left_position)?;
        } else {
            lower_position = left_position;
            left_position = right_position;

            left_candidate = right_candidate;

            right_position = lower_position + (upper_position - lower_position) / GOLDEN_RATIO;
            right_candidate = evaluate_candidate_at_position(right_position)?;
        }
    }

    best_candidate = best_candidate_by_quasi_log_likelihood(best_candidate, left_candidate);
    best_candidate = best_candidate_by_quasi_log_likelihood(best_candidate, right_candidate);

    Ok(best_candidate)
}

fn best_candidate_by_quasi_log_likelihood(
    left_candidate: MeanReversionStageCandidateEvaluation,
    right_candidate: MeanReversionStageCandidateEvaluation,
) -> MeanReversionStageCandidateEvaluation {
    if right_candidate.quasi_log_likelihood > left_candidate.quasi_log_likelihood {
        right_candidate
    } else {
        left_candidate
    }
}

fn adjacent_observation_level_differences(observation_levels: &[f64]) -> Vec<f64> {
    observation_levels
        .iter()
        .zip(observation_levels.iter().skip(1))
        .map(|(current_observation, next_observation)| next_observation - current_observation)
        .collect::<Vec<_>>()
}

fn remove_scaled_column_contribution(
    response: &[f64],
    column: &[f64],
    coefficient: f64,
) -> Vec<f64> {
    response
        .iter()
        .zip(column.iter())
        .map(|(current_response, column_value)| current_response - coefficient * column_value)
        .collect::<Vec<_>>()
}

fn runtime_projection_required_grid_evaluation(
    calibration_grid: MeanReversionStageCalibrationGrid,
) -> MeanReversionGridCandidateEvaluation {
    let fallback_candidate = MeanReversionStageCandidateEvaluation {
        candidate_family: RANDOM_WALK_NULL_CANDIDATE_FAMILY,
        anchor_origin_level: None,
        phi_reference: 1.0,
        kappa: 0.0,
        residual_sum_of_squares: 0.0,
        quasi_log_likelihood: f64::NEG_INFINITY,
        information_criterion_bic: f64::INFINITY,
        parameter_count: 0,
        least_squares_rank: 0,
        boundary_anchor_values: Vec::new(),
        deviations: Vec::new(),
        residuals: Vec::new(),
        stage_decision_reason: ENDOGENOUS_RUNTIME_PROJECTION_REQUIRED_REASON.to_string(),
    };

    MeanReversionGridCandidateEvaluation {
        calibration_grid,
        evaluated_candidates: vec![fallback_candidate],
        stage_decision_reason: ENDOGENOUS_RUNTIME_PROJECTION_REQUIRED_REASON.to_string(),
    }
}

fn zero_kappa_endogenous_fit_with_grid_evaluations(
    grid_evaluations: Vec<MeanReversionGridCandidateEvaluation>,
    stage_decision_reason: &str,
) -> Result<
    (MeanReversionFittedParameters, MeanReversionStageDiagnostics),
    MeanReversionStageExecutionFailure,
> {
    let Some(calibration_grid) = grid_evaluations
        .first()
        .map(|grid_evaluation| grid_evaluation.calibration_grid.clone())
    else {
        return Err(MeanReversionStageExecutionFailure::InvalidRunSpec {
            error_message:
                "mean-reversion zero-kappa fallback requires at least one evaluated grid"
                    .to_string(),
        });
    };

    let fitted_parameters = MeanReversionFittedParameters::zero_kappa();

    Ok((
        fitted_parameters,
        MeanReversionStageDiagnostics {
            selected_candidate_family: RANDOM_WALK_NULL_CANDIDATE_FAMILY.to_string(),
            calibration_grid: calibration_grid.clone(),
            anchor_origin_level: None,
            phi_reference: 1.0,
            kappa: 0.0,
            residual_sum_of_squares: 0.0,
            quasi_log_likelihood: None,
            information_criterion_bic: None,
            model_selection_policy: evidence::MEAN_REVERSION_BIC_MODEL_SELECTION_POLICY.to_string(),
            deviation_sample_count: 0,
            regression_pair_count: 0,
            least_squares_rank: 0,
            evaluated_candidates: Vec::new(),
            evaluated_grids: grid_evaluations
                .iter()
                .map(MeanReversionGridCandidateEvaluation::to_diagnostics)
                .collect(),
            stage_decision_reason: stage_decision_reason.to_string(),
        },
    ))
}

fn solve_one_column_least_squares(
    column: &[f64],
    response: &[f64],
) -> Result<RankAwareLeastSquaresSolution, MeanReversionStageExecutionFailure> {
    rank_aware_one_column_least_squares(column, response)
        .ok_or_else(|| least_squares_failure("one-column"))
}

fn least_squares_failure(fit_label: &str) -> MeanReversionStageExecutionFailure {
    MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
        error_message: format!(
            "mean-reversion {fit_label} least-squares requires non-empty aligned finite inputs"
        ),
    }
}

fn kappa_from_phi_reference(phi_reference: f64, calibration_grid_interval_hours: f64) -> f64 {
    if phi_reference == 1.0 {
        0.0
    } else {
        -phi_reference.ln() / calibration_grid_interval_hours
    }
}

fn gaussian_quasi_log_likelihood(
    regression_pair_count: usize,
    residual_sum_of_squares: f64,
) -> f64 {
    if regression_pair_count == 0 {
        return f64::NEG_INFINITY;
    }
    if residual_sum_of_squares == 0.0 {
        return f64::INFINITY;
    }

    let pair_count = regression_pair_count as f64;
    let sigma_squared_hat = residual_sum_of_squares / pair_count;

    -0.5 * pair_count * (1.0 + (2.0 * std::f64::consts::PI * sigma_squared_hat).ln())
}

fn finite_metric_or_none(value: f64) -> Option<f64> {
    if value.is_finite() { Some(value) } else { None }
}

fn emit_mean_reversion_progress(
    progress_sender: Option<&Sender<MeanReversionStageProgressUpdate>>,
    phase: MeanReversionStageProgressPhase,
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

fn stage_decision_reason_with_phi_reference_flags(
    base_reason: &str,
    phi_reference: Option<f64>,
) -> String {
    let mut reason = base_reason.to_string();

    if phi_reference == Some(1.0) {
        reason.push_str("|phi_reference_boundary_one.v1");
    }

    reason
}
