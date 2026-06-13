use std::collections::{BTreeMap, BTreeSet};
use std::panic;
use std::sync::mpsc::{self, Sender};

use super::clock::hawkes_adjacent_open_gap;
use super::contracts::{
    HAWKES_MINIMUM_POSITIVE_SEASONALITY_FACTOR, HawkesStageCandidateDiagnostic,
    HawkesStageCandidateInput, HawkesStageExecutionFailure, HawkesStageExecutionInput,
    HawkesStageExecutionMetadata, HawkesStageExecutionReport, HawkesStageFittedPayload,
    HawkesStageHandoffOutput, HawkesStageImpliedMetricsDiagnostic,
    HawkesStageObjectiveTermDiagnostic, HawkesStageObservedSourceRegime, HawkesStageProgressPhase,
    HawkesStageProgressUpdate,
};
use super::likelihood::{
    HawkesLikelihoodTape, build_hawkes_likelihood_tape,
    score_hawkes_candidates_with_signed_likelihood,
};
use super::progress::emit_hawkes_progress;
use super::seasonality_input::validate_hawkes_seasonality_event_vectors;
use super::selection::ranked_hawkes_candidate_indices;
use super::short_gap_ratio_from_threshold;
use crate::adapters::precheck_structured_hawkes_kernel_parameter_domain;
use crate::objective::CalibrationObjectiveTermInput;
use crate::optimizer_adapter::{
    CalibrationOptimizerAdapterCandidateInput, CalibrationOptimizerAdapterConfiguration,
    CalibrationOptimizerAdapterError, CalibrationOptimizerAdapterRunOutput,
    CalibrationOptimizerCandidateParameterValue, CalibrationOptimizerParameterBound,
    run_constrained_optimizer_adapter,
};
use crate::stages::parallelism::run_in_stage_thread_pool;
use crate::stages::seasonality::{
    CalibrationStageExecutionStatus, SeasonalityStageNormalizedPayload,
    maximum_runtime_interval_seasonality_factor_from_payload,
};
use janiform_calibration_artifacts::{
    CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_HAWKES_STAGE_HANDOFF_POLICY, CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_STAGE_SEED_DERIVATION_POLICY, CalibrationOptimizerSettings,
    CalibrationResolvedStageSeed, CalibrationRunSpec, CalibrationStage,
    CalibrationStageHandoffTransport, HawkesStageFittedParameters,
    resolve_calibration_stage_seed_metadata, stage_solver_settings_for_stage,
};
use janiform_foundations::numeric::{
    arithmetic_mean, coefficient_of_variation, round_positive_to_domain, round_to,
};
use janiform_foundations::time::SECONDS_PER_HOUR;
use janiform_market_time::calendar::{ExchangeCalendarRegistry, OPEN_MARKET_STATE_CODE};
use janiform_model_kernels::hawkes::{
    HawkesKernelLoadings, HawkesKernelParameterValidationReport, HawkesKernelParameters,
    HawkesKernelValidationStatus, HawkesStructuredAlphaParameterization,
    HawkesStructuredRhoParameterization, spectral_radius_alpha_over_beta,
    stationary_intensity_multipliers,
};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
#[derive(Debug, Clone, Copy, PartialEq)]
struct HawkesObservedActivityTargets {
    deseasonalized_open_event_rate_per_hour: f64,
    open_factor_exposure_hours: f64,
    deseasonalized_mean_interarrival_seconds: f64,
    deseasonalized_interarrival_coefficient_of_variation: f64,
    deseasonalized_short_gap_threshold_seconds: f64,
    deseasonalized_short_gap_ratio: f64,
    deseasonalized_known_news_rate_per_hour: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct HawkesObservedSourceRegimeKey {
    observation_duration_hours_bits: u64,
    open_session_duration_hours_bits: u64,
    total_tick_count: usize,
    open_session_tick_count: usize,
    closed_session_tick_count: usize,
    open_session_buy_tick_count: usize,
    open_session_sell_tick_count: usize,
    open_session_mean_interarrival_seconds_bits: u64,
    open_session_interarrival_coefficient_of_variation_bits: u64,
    open_session_short_gap_threshold_seconds_bits: u64,
    open_session_short_gap_ratio_bits: u64,
    known_news_open_event_count: usize,
    open_session_known_news_activity_share_bits: u64,
}

impl From<&HawkesStageObservedSourceRegime> for HawkesObservedSourceRegimeKey {
    fn from(source_regime: &HawkesStageObservedSourceRegime) -> Self {
        Self {
            observation_duration_hours_bits: source_regime.observation_duration_hours.to_bits(),
            open_session_duration_hours_bits: source_regime.open_session_duration_hours.to_bits(),
            total_tick_count: source_regime.total_tick_count,
            open_session_tick_count: source_regime.open_session_tick_count,
            closed_session_tick_count: source_regime.closed_session_tick_count,
            open_session_buy_tick_count: source_regime.open_session_buy_tick_count,
            open_session_sell_tick_count: source_regime.open_session_sell_tick_count,
            open_session_mean_interarrival_seconds_bits: source_regime
                .open_session_mean_interarrival_seconds
                .to_bits(),
            open_session_interarrival_coefficient_of_variation_bits: source_regime
                .open_session_interarrival_coefficient_of_variation
                .to_bits(),
            open_session_short_gap_threshold_seconds_bits: source_regime
                .open_session_short_gap_threshold_seconds
                .to_bits(),
            open_session_short_gap_ratio_bits: source_regime.open_session_short_gap_ratio.to_bits(),
            known_news_open_event_count: source_regime.known_news_open_event_count,
            open_session_known_news_activity_share_bits: source_regime
                .open_session_known_news_activity_share
                .to_bits(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct EndogenousStationarityCheck {
    spectral_radius_alpha_over_beta: f64,
    effective_branching_ratio: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct HawkesPreparedCandidate {
    pub(super) adjusted_candidate_input: HawkesStageCandidateInput,
    pub(super) validation_report: HawkesKernelParameterValidationReport,
    pub(super) objective_terms: Vec<CalibrationObjectiveTermInput>,
    pub(super) objective_term_diagnostics: Vec<HawkesStageObjectiveTermDiagnostic>,
    pub(super) implied_metrics: HawkesStageImpliedMetricsDiagnostic,
    pub(super) warnings: Vec<String>,
    pub(super) external_constraint_violation_count: usize,
}

struct HawkesStageSetup {
    stage_solver_settings: CalibrationOptimizerSettings,
    resolved_stage_seed: CalibrationResolvedStageSeed,
    optimizer_adapter_configuration: CalibrationOptimizerAdapterConfiguration,
    max_runtime_interval_seasonality_factor: f64,
    likelihood_tape: Option<HawkesLikelihoodTape>,
}

struct HawkesCandidateGridEvaluation {
    all_prepared_candidates: Vec<HawkesPreparedCandidate>,
    summary_candidate_diagnostics: Vec<HawkesStageCandidateDiagnostic>,
    summary_optimizer_adapter_output: CalibrationOptimizerAdapterRunOutput,
}

struct HawkesEvaluatedCandidateSet {
    prepared_candidates: Vec<HawkesPreparedCandidate>,
    candidate_diagnostics: Vec<HawkesStageCandidateDiagnostic>,
    optimizer_adapter_output: CalibrationOptimizerAdapterRunOutput,
}

struct HawkesCandidateSelection {
    selection_prepared_candidates: Vec<HawkesPreparedCandidate>,
    candidate_diagnostics: Vec<HawkesStageCandidateDiagnostic>,
    optimizer_adapter_output: CalibrationOptimizerAdapterRunOutput,
    selected_candidate_input: HawkesStageCandidateInput,
    kernel_parameters: HawkesKernelParameters,
    valid_candidate_count: usize,
    selected_candidate_raw_objective: f64,
    selected_candidate_penalized_objective: f64,
}

type HawkesStageReportResult<T> = Result<T, Box<HawkesStageExecutionReport>>;

pub fn run_hawkes_calibration_stage(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: HawkesStageExecutionInput,
) -> HawkesStageExecutionReport {
    run_hawkes_calibration_stage_inner(run_spec, calendar_registry, stage_input, None)
}

pub(crate) fn run_hawkes_calibration_stage_with_progress_observer(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: HawkesStageExecutionInput,
    progress_observer: Option<&mut dyn FnMut(HawkesStageProgressUpdate)>,
) -> HawkesStageExecutionReport {
    let Some(progress_observer) = progress_observer else {
        return run_hawkes_calibration_stage_inner(run_spec, calendar_registry, stage_input, None);
    };

    let (progress_sender, progress_receiver) = mpsc::channel();

    std::thread::scope(|scope| {
        let stage_thread = scope.spawn(move || {
            run_hawkes_calibration_stage_inner(
                run_spec,
                calendar_registry,
                stage_input,
                Some(progress_sender),
            )
        });

        for progress_update in progress_receiver {
            progress_observer(progress_update);
        }

        match stage_thread.join() {
            Ok(report) => report,
            Err(payload) => panic::resume_unwind(payload),
        }
    })
}

fn run_hawkes_calibration_stage_inner(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: HawkesStageExecutionInput,
    progress_sender: Option<Sender<HawkesStageProgressUpdate>>,
) -> HawkesStageExecutionReport {
    let progress_sender = progress_sender.as_ref();

    let setup = match resolve_hawkes_stage_setup(
        run_spec,
        calendar_registry,
        &stage_input,
        progress_sender,
    ) {
        Ok(setup) => setup,
        Err(report) => return *report,
    };

    let grid_evaluation =
        match evaluate_hawkes_candidate_grid(run_spec, &stage_input, &setup, progress_sender) {
            Ok(grid_evaluation) => grid_evaluation,
            Err(report) => return *report,
        };

    let selection =
        match select_hawkes_candidate(run_spec, grid_evaluation, &setup, progress_sender) {
            Ok(selection) => selection,
            Err(report) => return *report,
        };

    build_hawkes_stage_success_report(run_spec, &stage_input, setup, selection, progress_sender)
}

fn resolve_hawkes_stage_setup(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: &HawkesStageExecutionInput,
    progress_sender: Option<&Sender<HawkesStageProgressUpdate>>,
) -> HawkesStageReportResult<HawkesStageSetup> {
    let stage_solver_settings =
        validate_hawkes_stage_preflight(run_spec, stage_input, progress_sender)?;

    let resolved_stage_seed =
        resolve_calibration_stage_seed_metadata(run_spec, CalibrationStage::Hawkes);

    let optimizer_adapter_configuration = build_hawkes_optimizer_adapter_configuration(
        run_spec,
        &stage_solver_settings,
        resolved_stage_seed.stage_seed,
    )?;

    let seasonality_payload = stage_input.seasonality_input.payload();

    let max_runtime_interval_seasonality_factor =
        match maximum_runtime_interval_seasonality_factor_from_payload(
            calendar_registry,
            seasonality_payload,
        ) {
            Ok(value) => value,
            Err(error_message) => {
                return Err(Box::new(build_hawkes_stage_failure_report(
                    run_spec,
                    HawkesStageExecutionFailure::CandidateConstructionFailed {
                        candidate_id: "runtime_interval_seasonality_envelope".to_string(),
                        error_message,
                    },
                )));
            }
        };

    emit_hawkes_progress(
        progress_sender,
        HawkesStageProgressPhase::InputPreflight,
        6,
        6,
        Some("stage setup"),
    );

    let likelihood_tape = build_hawkes_likelihood_tape_for_stage(
        run_spec,
        seasonality_payload,
        stage_input.event_side_codes.as_slice(),
        progress_sender,
    )?;

    Ok(HawkesStageSetup {
        stage_solver_settings,
        resolved_stage_seed,
        optimizer_adapter_configuration,
        max_runtime_interval_seasonality_factor,
        likelihood_tape,
    })
}

fn validate_hawkes_stage_preflight(
    run_spec: &CalibrationRunSpec,
    stage_input: &HawkesStageExecutionInput,
    progress_sender: Option<&Sender<HawkesStageProgressUpdate>>,
) -> HawkesStageReportResult<CalibrationOptimizerSettings> {
    emit_hawkes_progress(
        progress_sender,
        HawkesStageProgressPhase::InputPreflight,
        0,
        6,
        Some("run spec"),
    );

    if let Err(source) = run_spec.validate() {
        return Err(Box::new(build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::InvalidRunSpec {
                error_message: format!("{source} (classification={:?})", source.classification()),
            },
        )));
    }

    emit_hawkes_progress(
        progress_sender,
        HawkesStageProgressPhase::InputPreflight,
        1,
        6,
        Some("stage selection"),
    );

    if !run_spec
        .selected_stages
        .iter()
        .copied()
        .any(|selected_stage| selected_stage == CalibrationStage::Hawkes)
    {
        return Err(Box::new(build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::StageIsNotSelectedInRunSpec,
        )));
    }

    emit_hawkes_progress(
        progress_sender,
        HawkesStageProgressPhase::InputPreflight,
        2,
        6,
        Some("solver settings"),
    );

    let Some(stage_solver_settings) =
        stage_solver_settings_for_stage(run_spec, CalibrationStage::Hawkes)
    else {
        return Err(Box::new(build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::MissingStageSolverSettings,
        )));
    };

    emit_hawkes_progress(
        progress_sender,
        HawkesStageProgressPhase::InputPreflight,
        3,
        6,
        Some("candidate inputs"),
    );

    if stage_input.candidate_inputs.is_empty() {
        return Err(Box::new(build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::MissingHawkesCandidateInputs,
        )));
    }

    emit_hawkes_progress(
        progress_sender,
        HawkesStageProgressPhase::InputPreflight,
        4,
        6,
        Some("seasonality handoff"),
    );

    validate_hawkes_seasonality_handoff(run_spec, stage_input)?;
    validate_hawkes_seasonality_payload(run_spec, stage_input.seasonality_input.payload())?;

    emit_hawkes_progress(
        progress_sender,
        HawkesStageProgressPhase::InputPreflight,
        5,
        6,
        Some("runtime inputs"),
    );

    Ok(stage_solver_settings)
}

fn validate_hawkes_seasonality_payload(
    run_spec: &CalibrationRunSpec,
    seasonality_payload: &SeasonalityStageNormalizedPayload,
) -> HawkesStageReportResult<()> {
    validate_hawkes_seasonality_event_vectors(seasonality_payload).map_err(|error_message| {
        Box::new(build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::InvalidSeasonalityPayload { error_message },
        ))
    })
}

fn validate_hawkes_seasonality_handoff(
    run_spec: &CalibrationRunSpec,
    stage_input: &HawkesStageExecutionInput,
) -> HawkesStageReportResult<()> {
    let expected_seasonality_handoff =
        run_spec.stage_handoff(CalibrationStage::Seasonality, CalibrationStage::Hawkes);

    let expected_transport = run_spec.stage_handoff_transport_or_default(
        CalibrationStage::Seasonality,
        CalibrationStage::Hawkes,
    );

    let provided_transport = stage_input.seasonality_input.transport();
    if provided_transport != expected_transport {
        return Err(Box::new(build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::SeasonalityInputTransportMismatch {
                expected_transport,
                provided_transport,
            },
        )));
    }

    if provided_transport == CalibrationStageHandoffTransport::PersistedArtifactReference {
        validate_hawkes_persisted_seasonality_handoff(
            run_spec,
            stage_input,
            expected_seasonality_handoff,
        )?;
    }

    Ok(())
}

fn validate_hawkes_persisted_seasonality_handoff(
    run_spec: &CalibrationRunSpec,
    stage_input: &HawkesStageExecutionInput,
    expected_seasonality_handoff: Option<&janiform_calibration_artifacts::CalibrationStageHandoff>,
) -> HawkesStageReportResult<()> {
    let Some(provided_artifact_reference) = stage_input
        .seasonality_input
        .artifact_reference()
        .map(ToOwned::to_owned)
    else {
        return Err(Box::new(build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::MissingArtifactReferenceInSeasonalityInput,
        )));
    };

    if provided_artifact_reference.trim().is_empty() {
        return Err(Box::new(build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::InvalidArtifactReferenceInSeasonalityInput {
                artifact_reference: provided_artifact_reference,
            },
        )));
    }

    if let Some(expected_stage_handoff) = expected_seasonality_handoff
        && let Some(expected_artifact_reference) = expected_stage_handoff.artifact_reference()
        && expected_artifact_reference != provided_artifact_reference
    {
        return Err(Box::new(build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::SeasonalityArtifactReferenceMismatch {
                expected_artifact_reference: expected_artifact_reference.to_string(),
                provided_artifact_reference,
            },
        )));
    }

    Ok(())
}

fn build_hawkes_optimizer_adapter_configuration(
    run_spec: &CalibrationRunSpec,
    stage_solver_settings: &CalibrationOptimizerSettings,
    stage_seed: u64,
) -> HawkesStageReportResult<CalibrationOptimizerAdapterConfiguration> {
    // Intentional clone because both stage metadata and optimizer configuration
    // persist the solver settings contract.
    match CalibrationOptimizerAdapterConfiguration::try_new(
        "candidate_enumeration.v1",
        stage_seed,
        stage_solver_settings.clone(),
        hawkes_stage_optimizer_bounds(),
    ) {
        Ok(configuration) => Ok(configuration),
        Err(source) => Err(Box::new(build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::CandidateConstructionFailed {
                candidate_id: "optimizer_adapter_configuration".to_string(),
                error_message: source.to_string(),
            },
        ))),
    }
}

fn build_hawkes_likelihood_tape_for_stage(
    run_spec: &CalibrationRunSpec,
    seasonality_payload: &SeasonalityStageNormalizedPayload,
    event_side_codes: &[u8],
    progress_sender: Option<&Sender<HawkesStageProgressUpdate>>,
) -> HawkesStageReportResult<Option<HawkesLikelihoodTape>> {
    build_hawkes_likelihood_tape(seasonality_payload, event_side_codes, progress_sender).map_err(
        |error_message| {
            Box::new(build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::CandidateConstructionFailed {
                    candidate_id: "signed_hawkes_likelihood_event_tape".to_string(),
                    error_message,
                },
            ))
        },
    )
}

fn evaluate_hawkes_candidate_grid(
    run_spec: &CalibrationRunSpec,
    stage_input: &HawkesStageExecutionInput,
    setup: &HawkesStageSetup,
    progress_sender: Option<&Sender<HawkesStageProgressUpdate>>,
) -> HawkesStageReportResult<HawkesCandidateGridEvaluation> {
    let seasonality_payload = stage_input.seasonality_input.payload();

    let mut observed_activity_targets_by_regime = BTreeMap::new();

    let initial_prepared_candidates = prepare_hawkes_candidate_batch(
        run_spec,
        stage_input.candidate_inputs.as_slice(),
        seasonality_payload,
        setup,
        &mut observed_activity_targets_by_regime,
        progress_sender,
        "initial grid",
    )?;

    let initial_candidate_count = initial_prepared_candidates.len();

    let (initial_candidate_diagnostics, initial_optimizer_output) =
        evaluate_hawkes_candidate_batch(
            run_spec,
            &setup.optimizer_adapter_configuration,
            initial_prepared_candidates.as_slice(),
            progress_sender,
            "activity objective",
        )?;

    let refined_candidate_inputs = generate_refined_hawkes_candidate_inputs(
        initial_prepared_candidates.as_slice(),
        initial_candidate_diagnostics.as_slice(),
    );

    emit_hawkes_progress(
        progress_sender,
        HawkesStageProgressPhase::CandidateRefinement,
        initial_candidate_count,
        initial_candidate_count,
        Some("refinement seeds"),
    );

    let mut all_prepared_candidates = initial_prepared_candidates;

    if !refined_candidate_inputs.is_empty() {
        let refined_prepared_candidates = prepare_hawkes_candidate_batch(
            run_spec,
            refined_candidate_inputs.as_slice(),
            seasonality_payload,
            setup,
            &mut observed_activity_targets_by_regime,
            progress_sender,
            "refined grid",
        )?;

        all_prepared_candidates.extend(refined_prepared_candidates);
    }

    let (summary_candidate_diagnostics, summary_optimizer_adapter_output) =
        if all_prepared_candidates.len() == initial_candidate_count {
            (initial_candidate_diagnostics, initial_optimizer_output)
        } else {
            evaluate_hawkes_candidate_batch(
                run_spec,
                &setup.optimizer_adapter_configuration,
                all_prepared_candidates.as_slice(),
                progress_sender,
                "refined activity objective",
            )?
        };

    Ok(HawkesCandidateGridEvaluation {
        all_prepared_candidates,
        summary_candidate_diagnostics,
        summary_optimizer_adapter_output,
    })
}

fn prepare_hawkes_candidate_batch(
    run_spec: &CalibrationRunSpec,
    candidate_inputs: &[HawkesStageCandidateInput],
    seasonality_payload: &SeasonalityStageNormalizedPayload,
    setup: &HawkesStageSetup,
    observed_activity_targets_by_regime: &mut BTreeMap<
        HawkesObservedSourceRegimeKey,
        HawkesObservedActivityTargets,
    >,
    progress_sender: Option<&Sender<HawkesStageProgressUpdate>>,
    current_item_label: &str,
) -> HawkesStageReportResult<Vec<HawkesPreparedCandidate>> {
    emit_hawkes_progress(
        progress_sender,
        HawkesStageProgressPhase::CandidatePreparation,
        0,
        candidate_inputs.len(),
        Some(current_item_label),
    );

    let prepared_candidates = prepare_hawkes_candidates(
        run_spec,
        candidate_inputs,
        seasonality_payload,
        setup.max_runtime_interval_seasonality_factor,
        setup.stage_solver_settings.cpu_count,
        observed_activity_targets_by_regime,
    )?;

    emit_hawkes_progress(
        progress_sender,
        HawkesStageProgressPhase::CandidatePreparation,
        prepared_candidates.len(),
        prepared_candidates.len(),
        Some(current_item_label),
    );

    Ok(prepared_candidates)
}

fn evaluate_hawkes_candidate_batch(
    run_spec: &CalibrationRunSpec,
    optimizer_adapter_configuration: &CalibrationOptimizerAdapterConfiguration,
    prepared_candidates: &[HawkesPreparedCandidate],
    progress_sender: Option<&Sender<HawkesStageProgressUpdate>>,
    current_item_label: &str,
) -> HawkesStageReportResult<(
    Vec<HawkesStageCandidateDiagnostic>,
    CalibrationOptimizerAdapterRunOutput,
)> {
    emit_hawkes_progress(
        progress_sender,
        HawkesStageProgressPhase::CandidateEvaluation,
        0,
        prepared_candidates.len(),
        Some(current_item_label),
    );

    let evaluation = evaluate_prepared_hawkes_candidates(
        run_spec,
        optimizer_adapter_configuration,
        prepared_candidates,
    )?;

    emit_hawkes_progress(
        progress_sender,
        HawkesStageProgressPhase::CandidateEvaluation,
        prepared_candidates.len(),
        prepared_candidates.len(),
        Some(current_item_label),
    );

    Ok(evaluation)
}

fn select_hawkes_candidate(
    run_spec: &CalibrationRunSpec,
    grid_evaluation: HawkesCandidateGridEvaluation,
    setup: &HawkesStageSetup,
    progress_sender: Option<&Sender<HawkesStageProgressUpdate>>,
) -> HawkesStageReportResult<HawkesCandidateSelection> {
    let evaluated_candidate_set =
        evaluate_hawkes_selection_candidate_set(run_spec, grid_evaluation, setup, progress_sender)?;

    build_hawkes_candidate_selection(run_spec, evaluated_candidate_set, progress_sender)
}

fn evaluate_hawkes_selection_candidate_set(
    run_spec: &CalibrationRunSpec,
    grid_evaluation: HawkesCandidateGridEvaluation,
    setup: &HawkesStageSetup,
    progress_sender: Option<&Sender<HawkesStageProgressUpdate>>,
) -> HawkesStageReportResult<HawkesEvaluatedCandidateSet> {
    let HawkesCandidateGridEvaluation {
        all_prepared_candidates,
        summary_candidate_diagnostics,
        summary_optimizer_adapter_output,
    } = grid_evaluation;

    let Some(likelihood_tape) = setup.likelihood_tape.as_ref() else {
        return Ok(HawkesEvaluatedCandidateSet {
            prepared_candidates: all_prepared_candidates,
            candidate_diagnostics: summary_candidate_diagnostics,
            optimizer_adapter_output: summary_optimizer_adapter_output,
        });
    };

    let likelihood_prepared_candidates = score_hawkes_candidates_with_signed_likelihood(
        all_prepared_candidates.as_slice(),
        summary_candidate_diagnostics.as_slice(),
        likelihood_tape,
        setup.stage_solver_settings.max_iterations,
        setup.stage_solver_settings.cpu_count,
        progress_sender,
    )
    .map_err(|error_message| {
        Box::new(build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::CandidateConstructionFailed {
                candidate_id: "signed_hawkes_likelihood_shortlist".to_string(),
                error_message,
            },
        ))
    })?;

    if likelihood_prepared_candidates.is_empty() {
        return Ok(HawkesEvaluatedCandidateSet {
            prepared_candidates: all_prepared_candidates,
            candidate_diagnostics: summary_candidate_diagnostics,
            optimizer_adapter_output: summary_optimizer_adapter_output,
        });
    }

    let (candidate_diagnostics, optimizer_adapter_output) = evaluate_hawkes_candidate_batch(
        run_spec,
        &setup.optimizer_adapter_configuration,
        likelihood_prepared_candidates.as_slice(),
        progress_sender,
        "signed likelihood objective",
    )?;

    Ok(HawkesEvaluatedCandidateSet {
        prepared_candidates: likelihood_prepared_candidates,
        candidate_diagnostics,
        optimizer_adapter_output,
    })
}

fn build_hawkes_candidate_selection(
    run_spec: &CalibrationRunSpec,
    evaluated_candidate_set: HawkesEvaluatedCandidateSet,
    progress_sender: Option<&Sender<HawkesStageProgressUpdate>>,
) -> HawkesStageReportResult<HawkesCandidateSelection> {
    emit_hawkes_progress(
        progress_sender,
        HawkesStageProgressPhase::CandidateSelection,
        0,
        1,
        Some("optimizer selection"),
    );

    let HawkesEvaluatedCandidateSet {
        prepared_candidates,
        candidate_diagnostics,
        optimizer_adapter_output,
    } = evaluated_candidate_set;

    let Some(selected_candidate_id) = optimizer_adapter_output.selected_candidate_id.as_ref()
    else {
        return Err(Box::new(build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::ConstraintViolationsForAllCandidates {
                candidate_diagnostics,
            },
        )));
    };

    let Some(selected_candidate_position) = prepared_candidates.iter().position(|candidate| {
        candidate.adjusted_candidate_input.candidate_id == *selected_candidate_id
    }) else {
        return Err(Box::new(build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::CandidateConstructionFailed {
                candidate_id: "optimizer_adapter_selection".to_string(),
                error_message: "selected candidate id was not found in evaluated hawkes candidates"
                    .to_string(),
            },
        )));
    };

    let selected_candidate_input = selected_hawkes_candidate_input(
        run_spec,
        prepared_candidates.as_slice(),
        selected_candidate_position,
    )?;

    let selected_candidate_diagnostic = selected_hawkes_candidate_diagnostic(
        run_spec,
        candidate_diagnostics.as_slice(),
        selected_candidate_position,
    )?;

    let kernel_parameters = selected_hawkes_kernel_parameters(
        run_spec,
        &selected_candidate_input,
        selected_candidate_diagnostic,
    )?;

    let selected_candidate_raw_objective = selected_candidate_diagnostic.raw_objective_value;

    let selected_candidate_penalized_objective =
        selected_candidate_diagnostic.penalized_objective_value;

    let valid_candidate_count = optimizer_adapter_output
        .candidate_diagnostics
        .iter()
        .filter(|candidate_diagnostic| candidate_diagnostic.total_constraint_violation_count == 0)
        .count();

    emit_hawkes_progress(
        progress_sender,
        HawkesStageProgressPhase::CandidateSelection,
        1,
        1,
        Some(selected_candidate_input.candidate_id.as_str()),
    );

    Ok(HawkesCandidateSelection {
        selection_prepared_candidates: prepared_candidates,
        candidate_diagnostics,
        optimizer_adapter_output,
        selected_candidate_input,
        kernel_parameters,
        valid_candidate_count,
        selected_candidate_raw_objective,
        selected_candidate_penalized_objective,
    })
}

fn selected_hawkes_candidate_input(
    run_spec: &CalibrationRunSpec,
    prepared_candidates: &[HawkesPreparedCandidate],
    selected_candidate_position: usize,
) -> HawkesStageReportResult<HawkesStageCandidateInput> {
    let Some(selected_candidate) = prepared_candidates.get(selected_candidate_position) else {
        return Err(Box::new(build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::CandidateConstructionFailed {
                candidate_id: "optimizer_adapter_selection".to_string(),
                error_message:
                    "selected candidate position was outside evaluated hawkes candidates"
                        .to_string(),
            },
        )));
    };

    // Intentional clone because the selected parameter contract is persisted
    // separately from the evaluated candidate grid.
    Ok(selected_candidate.adjusted_candidate_input.clone())
}

fn selected_hawkes_candidate_diagnostic<'a>(
    run_spec: &CalibrationRunSpec,
    candidate_diagnostics: &'a [HawkesStageCandidateDiagnostic],
    selected_candidate_position: usize,
) -> HawkesStageReportResult<&'a HawkesStageCandidateDiagnostic> {
    let Some(selected_candidate_diagnostic) =
        candidate_diagnostics.get(selected_candidate_position)
    else {
        return Err(Box::new(build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::CandidateConstructionFailed {
                candidate_id: "optimizer_adapter_selection".to_string(),
                error_message: "selected candidate position was outside hawkes diagnostics"
                    .to_string(),
            },
        )));
    };

    Ok(selected_candidate_diagnostic)
}

fn selected_hawkes_kernel_parameters(
    run_spec: &CalibrationRunSpec,
    selected_candidate_input: &HawkesStageCandidateInput,
    selected_candidate_diagnostic: &HawkesStageCandidateDiagnostic,
) -> HawkesStageReportResult<HawkesKernelParameters> {
    let Some(derived_alpha) = selected_candidate_diagnostic
        .validation_report
        .derived_alpha
    else {
        return Err(Box::new(build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::CandidateConstructionFailed {
                candidate_id: selected_candidate_input.candidate_id.clone(),
                error_message:
                    "selected candidate passed validation but did not provide derived alpha"
                        .to_string(),
            },
        )));
    };

    let Some(derived_rho) = selected_candidate_diagnostic.validation_report.derived_rho else {
        return Err(Box::new(build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::CandidateConstructionFailed {
                candidate_id: selected_candidate_input.candidate_id.clone(),
                error_message:
                    "selected candidate passed validation but did not provide derived rho"
                        .to_string(),
            },
        )));
    };

    HawkesKernelParameters::try_new_with_stability_tolerance(
        selected_candidate_input.lambda_bar,
        selected_candidate_input.beta,
        derived_alpha,
        derived_rho,
        selected_candidate_input.stability_numeric_tolerance,
    )
    .map_err(|source| {
        Box::new(build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::CandidateConstructionFailed {
                candidate_id: selected_candidate_input.candidate_id.clone(),
                error_message: source.to_string(),
            },
        ))
    })
}

fn build_hawkes_stage_success_report(
    run_spec: &CalibrationRunSpec,
    stage_input: &HawkesStageExecutionInput,
    setup: HawkesStageSetup,
    selection: HawkesCandidateSelection,
    progress_sender: Option<&Sender<HawkesStageProgressUpdate>>,
) -> HawkesStageExecutionReport {
    emit_hawkes_progress(
        progress_sender,
        HawkesStageProgressPhase::ReportAssembly,
        0,
        1,
        Some("stage payload"),
    );

    let HawkesStageSetup {
        stage_solver_settings,
        resolved_stage_seed,
        optimizer_adapter_configuration: _,
        max_runtime_interval_seasonality_factor: _,
        likelihood_tape: _,
    } = setup;

    let HawkesCandidateSelection {
        selection_prepared_candidates,
        candidate_diagnostics,
        optimizer_adapter_output,
        selected_candidate_input,
        kernel_parameters,
        valid_candidate_count,
        selected_candidate_raw_objective,
        selected_candidate_penalized_objective,
    } = selection;

    let fitted_stage_payload = HawkesStageFittedPayload {
        schema_version: CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION,
        selected_candidate_id: selected_candidate_input.candidate_id.clone(),
        fitted_parameters: HawkesStageFittedParameters {
            kernel_parameters,
            structured_alpha: selected_candidate_input.structured_alpha,
            structured_rho: selected_candidate_input.structured_rho,
            self_dominance_policy: selected_candidate_input.self_dominance_policy,
            stability_numeric_tolerance: selected_candidate_input.stability_numeric_tolerance,
        },
        candidate_diagnostics,
        seasonality_input_transport: stage_input.seasonality_input.transport(),
        seasonality_dataset_id: stage_input
            .seasonality_input
            .dataset_id()
            .map(ToOwned::to_owned),
        seasonality_artifact_reference: stage_input
            .seasonality_input
            .artifact_reference()
            .map(ToOwned::to_owned),
    };

    emit_hawkes_progress(
        progress_sender,
        HawkesStageProgressPhase::ReportAssembly,
        1,
        1,
        Some("stage report"),
    );

    HawkesStageExecutionReport {
        schema_version: CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::Hawkes,
        status: CalibrationStageExecutionStatus::Success,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_HAWKES_STAGE_HANDOFF_POLICY,
        handoffs: build_hawkes_stage_handoff_outputs(run_spec),
        fitted_stage_payload: Some(fitted_stage_payload),
        metadata: Some(HawkesStageExecutionMetadata {
            stage_output_schema_version: CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION,
            run_global_seed: resolved_stage_seed.run_global_seed,
            stage_seed: resolved_stage_seed.stage_seed,
            stage_seed_source: resolved_stage_seed.stage_seed_source,
            stage_seed_derivation_policy: CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
            solver_settings: stage_solver_settings,
            optimizer_adapter_metadata: optimizer_adapter_output.metadata,
            input_candidate_count: selection_prepared_candidates.len(),
            valid_candidate_count,
            selected_candidate_raw_objective,
            selected_candidate_penalized_objective,
        }),
        failure: None,
    }
}

fn build_candidate_objective_terms(
    candidate_input: &HawkesStageCandidateInput,
    observed_activity_targets: HawkesObservedActivityTargets,
    lambda_bar_scaling_ratio_applied: f64,
    max_runtime_interval_seasonality_factor: f64,
    derived_alpha: Option<HawkesKernelLoadings>,
    derived_rho: Option<HawkesKernelLoadings>,
) -> (
    Vec<CalibrationObjectiveTermInput>,
    Vec<HawkesStageObjectiveTermDiagnostic>,
    HawkesStageImpliedMetricsDiagnostic,
    Vec<String>,
) {
    let Some(derived_alpha) = derived_alpha else {
        return invalid_candidate_objective_bundle(
            candidate_input,
            "candidate did not derive alpha",
        );
    };

    let Some(derived_rho) = derived_rho else {
        return invalid_candidate_objective_bundle(candidate_input, "candidate did not derive rho");
    };

    let Some(implied_metrics) = derive_candidate_implied_metrics(
        candidate_input,
        observed_activity_targets,
        lambda_bar_scaling_ratio_applied,
        max_runtime_interval_seasonality_factor,
        derived_alpha,
        derived_rho,
    ) else {
        return invalid_candidate_objective_bundle(
            candidate_input,
            "candidate does not admit stationary Hawkes intensity multipliers",
        );
    };

    let mut warnings = Vec::new();

    if implied_metrics.event_volume_ratio_vs_total_observed > 1.5 {
        warnings.push(format!(
            "model implied event count over the observation window is {:.3}x the observed total tick count",
            implied_metrics.event_volume_ratio_vs_total_observed
        ));
    }
    if implied_metrics.model_implied_open_event_rate_per_hour
        > implied_metrics.deseasonalized_open_event_rate_per_hour_target * 1.5
    {
        warnings.push(format!(
            "model implied deseasonalized open-session rate {:.3} exceeds the deseasonalized target {:.3} by more than 50%",
            implied_metrics.model_implied_open_event_rate_per_hour,
            implied_metrics.deseasonalized_open_event_rate_per_hour_target
        ));
    }

    let mut objective_terms = Vec::new();
    let mut term_diagnostics = Vec::new();

    push_objective_term(
        "open_session_event_rate",
        3.0,
        observed_activity_targets.deseasonalized_open_event_rate_per_hour,
        implied_metrics.model_implied_open_event_rate_per_hour,
        observed_activity_targets
            .deseasonalized_open_event_rate_per_hour
            .max(1.0),
        &mut objective_terms,
        &mut term_diagnostics,
    );

    push_objective_term(
        "directional_imbalance_magnitude",
        1.5,
        observed_directional_imbalance_magnitude(&candidate_input.source_regime),
        (implied_metrics.model_implied_open_sell_share - 0.5).abs() * 2.0,
        observed_directional_imbalance_magnitude(&candidate_input.source_regime).max(0.05),
        &mut objective_terms,
        &mut term_diagnostics,
    );

    push_objective_term(
        "interarrival_coefficient_of_variation",
        1.25,
        observed_activity_targets.deseasonalized_interarrival_coefficient_of_variation,
        implied_metrics.model_implied_interarrival_coefficient_of_variation,
        observed_activity_targets
            .deseasonalized_interarrival_coefficient_of_variation
            .max(0.25),
        &mut objective_terms,
        &mut term_diagnostics,
    );

    push_objective_term(
        "short_gap_ratio",
        1.0,
        observed_activity_targets.deseasonalized_short_gap_ratio,
        implied_metrics.model_implied_short_gap_ratio,
        observed_activity_targets
            .deseasonalized_short_gap_ratio
            .max(0.05),
        &mut objective_terms,
        &mut term_diagnostics,
    );

    push_objective_term(
        "observation_window_event_count",
        2.0,
        candidate_input.source_regime.open_session_tick_count as f64,
        implied_metrics.model_implied_expected_event_count_over_observation_window,
        (candidate_input.source_regime.open_session_tick_count as f64).max(1.0),
        &mut objective_terms,
        &mut term_diagnostics,
    );

    if candidate_input.source_regime.known_news_open_event_count > 0 {
        push_objective_term(
            "known_news_activity_share",
            0.75,
            candidate_input
                .source_regime
                .open_session_known_news_activity_share,
            implied_metrics.model_implied_known_news_activity_share,
            candidate_input
                .source_regime
                .open_session_known_news_activity_share
                .max(0.02),
            &mut objective_terms,
            &mut term_diagnostics,
        );
    }

    (objective_terms, term_diagnostics, implied_metrics, warnings)
}

fn prepare_hawkes_candidates(
    run_spec: &CalibrationRunSpec,
    candidate_inputs: &[HawkesStageCandidateInput],
    seasonality_payload: &SeasonalityStageNormalizedPayload,
    max_runtime_interval_seasonality_factor: f64,
    cpu_count: usize,
    observed_activity_targets_by_regime: &mut BTreeMap<
        HawkesObservedSourceRegimeKey,
        HawkesObservedActivityTargets,
    >,
) -> HawkesStageReportResult<Vec<HawkesPreparedCandidate>> {
    let validated_candidate_inputs = validate_hawkes_candidate_inputs(run_spec, candidate_inputs)?;

    populate_observed_activity_targets_cache(
        validated_candidate_inputs.as_slice(),
        seasonality_payload,
        observed_activity_targets_by_regime,
    );

    Ok(prepare_hawkes_candidates_from_validated_inputs(
        validated_candidate_inputs.as_slice(),
        observed_activity_targets_by_regime,
        max_runtime_interval_seasonality_factor,
        cpu_count,
    ))
}

fn validate_hawkes_candidate_inputs<'a>(
    run_spec: &CalibrationRunSpec,
    candidate_inputs: &'a [HawkesStageCandidateInput],
) -> HawkesStageReportResult<Vec<&'a HawkesStageCandidateInput>> {
    let mut seen_candidate_ids = BTreeSet::new();
    let mut validated_candidate_inputs = Vec::with_capacity(candidate_inputs.len());

    for candidate_input in candidate_inputs {
        if !seen_candidate_ids.insert(candidate_input.candidate_id.clone()) {
            return Err(Box::new(build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::DuplicateCandidateId {
                    candidate_id: candidate_input.candidate_id.clone(),
                },
            )));
        }

        if let Err(source) = candidate_input.validate() {
            return Err(Box::new(build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::InvalidCandidateInput {
                    candidate_id: candidate_input.candidate_id.clone(),
                    error_message: source.to_string(),
                },
            )));
        }

        validated_candidate_inputs.push(candidate_input);
    }

    Ok(validated_candidate_inputs)
}

fn populate_observed_activity_targets_cache(
    validated_candidate_inputs: &[&HawkesStageCandidateInput],
    seasonality_payload: &SeasonalityStageNormalizedPayload,
    observed_activity_targets_by_regime: &mut BTreeMap<
        HawkesObservedSourceRegimeKey,
        HawkesObservedActivityTargets,
    >,
) {
    for candidate_input in validated_candidate_inputs {
        let regime_key = HawkesObservedSourceRegimeKey::from(&candidate_input.source_regime);

        observed_activity_targets_by_regime
            .entry(regime_key)
            .or_insert_with(|| {
                derive_observed_activity_targets(
                    &candidate_input.source_regime,
                    seasonality_payload,
                )
            });
    }
}

fn prepare_hawkes_candidates_from_validated_inputs(
    validated_candidate_inputs: &[&HawkesStageCandidateInput],
    observed_activity_targets_by_regime: &BTreeMap<
        HawkesObservedSourceRegimeKey,
        HawkesObservedActivityTargets,
    >,
    max_runtime_interval_seasonality_factor: f64,
    cpu_count: usize,
) -> Vec<HawkesPreparedCandidate> {
    run_with_hawkes_stage_thread_pool(cpu_count, || {
        validated_candidate_inputs
            .par_iter()
            .map(|candidate_input| {
                let candidate_input = *candidate_input;
                let regime_key =
                    HawkesObservedSourceRegimeKey::from(&candidate_input.source_regime);
                let observed_activity_targets = *observed_activity_targets_by_regime
                    .get(&regime_key)
                    .unwrap_or_else(|| {
                        unreachable!(
                            "validated Hawkes candidate must have cached observed activity targets"
                        )
                    });

                prepare_hawkes_candidate(
                    candidate_input,
                    observed_activity_targets,
                    max_runtime_interval_seasonality_factor,
                )
            })
            .collect()
    })
}

fn prepare_hawkes_candidate(
    candidate_input: &HawkesStageCandidateInput,
    observed_activity_targets: HawkesObservedActivityTargets,
    max_runtime_interval_seasonality_factor: f64,
) -> HawkesPreparedCandidate {
    // Intentional copy to keep the immutable candidate grid reusable across CPU schedules.
    let candidate_for_adjustment = candidate_input.clone();

    let (adjusted_candidate_input, lambda_bar_scaling_ratio_applied) =
        adjust_candidate_for_deseasonalized_activity(
            candidate_for_adjustment,
            observed_activity_targets,
        );

    let validation_report = precheck_structured_hawkes_kernel_parameter_domain(
        adjusted_candidate_input.lambda_bar,
        adjusted_candidate_input.beta,
        adjusted_candidate_input.structured_alpha,
        adjusted_candidate_input.structured_rho,
        adjusted_candidate_input.self_dominance_policy,
        adjusted_candidate_input.stability_numeric_tolerance,
    );

    let mut external_constraint_violation_count = validation_report.summary.failed_check_count;

    let (objective_terms, objective_term_diagnostics, implied_metrics, mut warnings) =
        build_candidate_objective_terms(
            &adjusted_candidate_input,
            observed_activity_targets,
            lambda_bar_scaling_ratio_applied,
            max_runtime_interval_seasonality_factor,
            validation_report.derived_alpha,
            validation_report.derived_rho,
        );

    if let Some(stationarity_check) = validation_report.derived_alpha.and_then(|derived_alpha| {
        check_endogenous_stationarity(derived_alpha, adjusted_candidate_input.beta)
    }) {
        external_constraint_violation_count = external_constraint_violation_count.saturating_add(1);

        warnings.push(format!(
            "endogenous Hawkes branching ratio {:.6} reaches or exceeds 1.0",
            stationarity_check.effective_branching_ratio,
        ));
    }

    HawkesPreparedCandidate {
        adjusted_candidate_input,
        validation_report,
        objective_terms,
        objective_term_diagnostics,
        implied_metrics,
        warnings,
        external_constraint_violation_count,
    }
}

fn run_with_hawkes_stage_thread_pool<T, F>(cpu_count: usize, operation: F) -> T
where
    T: Send,
    F: FnOnce() -> T + Send,
{
    run_in_stage_thread_pool(cpu_count, operation)
}

fn evaluate_prepared_hawkes_candidates(
    run_spec: &CalibrationRunSpec,
    optimizer_adapter_configuration: &CalibrationOptimizerAdapterConfiguration,
    prepared_candidates: &[HawkesPreparedCandidate],
) -> HawkesStageReportResult<(
    Vec<HawkesStageCandidateDiagnostic>,
    CalibrationOptimizerAdapterRunOutput,
)> {
    let adapter_candidate_inputs: Vec<CalibrationOptimizerAdapterCandidateInput> =
        prepared_candidates
            .iter()
            .map(|candidate| CalibrationOptimizerAdapterCandidateInput {
                candidate_id: candidate.adjusted_candidate_input.candidate_id.clone(),
                objective_terms: candidate.objective_terms.clone(),
                parameter_values: vec![
                    CalibrationOptimizerCandidateParameterValue {
                        parameter_name: "lambda_bar".to_string(),
                        value: candidate.adjusted_candidate_input.lambda_bar,
                    },
                    CalibrationOptimizerCandidateParameterValue {
                        parameter_name: "beta".to_string(),
                        value: candidate.adjusted_candidate_input.beta,
                    },
                    CalibrationOptimizerCandidateParameterValue {
                        parameter_name: "stability_numeric_tolerance".to_string(),
                        value: candidate
                            .adjusted_candidate_input
                            .stability_numeric_tolerance,
                    },
                ],
                external_constraint_violation_count: candidate.external_constraint_violation_count,
            })
            .collect();

    let optimizer_adapter_output = match run_constrained_optimizer_adapter(
        optimizer_adapter_configuration,
        adapter_candidate_inputs.as_slice(),
    ) {
        Ok(output) => output,
        Err(CalibrationOptimizerAdapterError::InvalidCandidateInput {
            candidate_id,
            source,
        }) => {
            return Err(Box::new(build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::InvalidCandidateInput {
                    candidate_id,
                    error_message: source.to_string(),
                },
            )));
        }
        Err(CalibrationOptimizerAdapterError::InvalidObjectiveComposition {
            candidate_id,
            source,
        }) => {
            return Err(Box::new(build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::InvalidCandidateInput {
                    candidate_id,
                    error_message: source.to_string(),
                },
            )));
        }
        Err(CalibrationOptimizerAdapterError::DuplicateCandidateId { candidate_id }) => {
            return Err(Box::new(build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::DuplicateCandidateId { candidate_id },
            )));
        }
        Err(source) => {
            return Err(Box::new(build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::CandidateConstructionFailed {
                    candidate_id: "optimizer_adapter".to_string(),
                    error_message: source.to_string(),
                },
            )));
        }
    };

    let candidate_diagnostics = prepared_candidates
        .iter()
        .zip(optimizer_adapter_output.candidate_diagnostics.iter())
        .map(
            |(candidate, optimizer_diagnostic)| HawkesStageCandidateDiagnostic {
                candidate_id: candidate.adjusted_candidate_input.candidate_id.clone(),
                raw_objective_value: optimizer_diagnostic.raw_objective_value,
                constraint_penalty: optimizer_diagnostic.constraint_penalty,
                penalized_objective_value: optimizer_diagnostic.penalized_objective_value,
                constraint_violation_count: optimizer_diagnostic.total_constraint_violation_count,
                objective_terms: candidate.objective_term_diagnostics.clone(),
                source_regime: candidate.adjusted_candidate_input.source_regime.clone(),
                directional_label_diagnostics: candidate
                    .adjusted_candidate_input
                    .directional_label_diagnostics
                    .clone(),
                // Intentional clone into the persisted stage diagnostics payload.
                implied_metrics: candidate.implied_metrics.clone(),
                warnings: candidate.warnings.clone(),
                // Intentional clone into the persisted stage diagnostics payload.
                validation_report: candidate.validation_report.clone(),
            },
        )
        .collect();

    Ok((candidate_diagnostics, optimizer_adapter_output))
}

fn generate_refined_hawkes_candidate_inputs(
    prepared_candidates: &[HawkesPreparedCandidate],
    candidate_diagnostics: &[HawkesStageCandidateDiagnostic],
) -> Vec<HawkesStageCandidateInput> {
    const MAX_REFINEMENT_SEEDS: usize = 3;

    const LAMBDA_SCALES: [f64; 3] = [0.7, 1.0, 1.4];
    const BETA_SCALES: [f64; 3] = [0.7, 1.0, 1.4];
    const RHO_SCALES: [f64; 3] = [0.6, 1.0, 1.6];

    let ranked_seed_indices =
        ranked_refinement_seed_indices(prepared_candidates, candidate_diagnostics);

    let mut refinement_candidates = Vec::new();
    let mut seen_ids = BTreeSet::new();

    for (seed_rank, candidate_index) in ranked_seed_indices
        .into_iter()
        .take(MAX_REFINEMENT_SEEDS)
        .enumerate()
    {
        let seed_candidate = &prepared_candidates[candidate_index].adjusted_candidate_input;
        let alpha_scales = refinement_alpha_scales(&candidate_diagnostics[candidate_index]);

        for lambda_scale in LAMBDA_SCALES {
            for beta_scale in BETA_SCALES {
                for alpha_scale in &alpha_scales {
                    for rho_scale in RHO_SCALES {
                        if lambda_scale == 1.0
                            && beta_scale == 1.0
                            && *alpha_scale == 1.0
                            && rho_scale == 1.0
                        {
                            continue;
                        }

                        let candidate_id = format!(
                            "{}_refined_{seed_rank}_{:03}",
                            seed_candidate.candidate_id,
                            refinement_candidates.len()
                        );
                        if !seen_ids.insert(candidate_id.clone()) {
                            continue;
                        }

                        refinement_candidates.push(HawkesStageCandidateInput {
                            candidate_id,
                            lambda_bar: round_positive_to_domain(
                                seed_candidate.lambda_bar * lambda_scale,
                                12,
                            ),
                            beta: round_positive_to_domain(seed_candidate.beta * beta_scale, 12),
                            structured_alpha: scale_structured_alpha(
                                seed_candidate.structured_alpha,
                                *alpha_scale,
                            ),
                            structured_rho: scale_structured_rho(
                                seed_candidate.structured_rho,
                                rho_scale,
                            ),
                            self_dominance_policy: seed_candidate.self_dominance_policy,
                            stability_numeric_tolerance: seed_candidate.stability_numeric_tolerance,
                            source_regime: seed_candidate.source_regime.clone(),
                            directional_label_diagnostics: seed_candidate
                                .directional_label_diagnostics
                                .clone(),
                        });
                    }
                }
            }
        }
    }

    refinement_candidates
}

fn ranked_refinement_seed_indices(
    prepared_candidates: &[HawkesPreparedCandidate],
    candidate_diagnostics: &[HawkesStageCandidateDiagnostic],
) -> Vec<usize> {
    let eligible_seed_indices: Vec<usize> = prepared_candidates
        .iter()
        .enumerate()
        .filter_map(|(candidate_index, candidate)| {
            (candidate.validation_report.status == HawkesKernelValidationStatus::Pass)
                .then_some(candidate_index)
        })
        .collect();
    if eligible_seed_indices.is_empty() {
        return Vec::new();
    }

    ranked_hawkes_candidate_indices(candidate_diagnostics, eligible_seed_indices.as_slice())
}

fn refinement_alpha_scales(candidate_diagnostic: &HawkesStageCandidateDiagnostic) -> Vec<f64> {
    const BASE_ALPHA_SCALES: [f64; 7] = [0.05, 0.1, 0.2, 0.4, 0.8, 1.0, 1.2];
    const TARGET_STABILITY_MARGIN: f64 = 0.95;
    const MINIMUM_ALPHA_SCALE: f64 = 0.01;

    let mut alpha_scales = BASE_ALPHA_SCALES.to_vec();

    let effective_branching_ratio = candidate_diagnostic
        .implied_metrics
        .max_runtime_effective_branching_ratio;

    if effective_branching_ratio.is_finite() && effective_branching_ratio > 0.0 {
        let targeted_scale = round_to(
            (TARGET_STABILITY_MARGIN / effective_branching_ratio).clamp(MINIMUM_ALPHA_SCALE, 1.0),
            12,
        );

        alpha_scales.push(targeted_scale);
        alpha_scales.push(round_to(
            (targeted_scale * 0.5).clamp(MINIMUM_ALPHA_SCALE, 1.0),
            12,
        ));
    }

    alpha_scales.sort_by(|left, right| left.total_cmp(right));
    alpha_scales.dedup_by(|left, right| left.total_cmp(right).is_eq());

    alpha_scales
}

fn scale_structured_alpha(
    structured_alpha: HawkesStructuredAlphaParameterization,
    scale: f64,
) -> HawkesStructuredAlphaParameterization {
    HawkesStructuredAlphaParameterization {
        a_self: round_positive_to_domain(structured_alpha.a_self * scale, 12),
        a_cross: round_to((structured_alpha.a_cross * scale).max(0.0), 12),
        delta_self: round_to((structured_alpha.delta_self * scale).max(0.0), 12),
        delta_cross: round_to((structured_alpha.delta_cross * scale).max(0.0), 12),
    }
}

fn scale_structured_rho(
    structured_rho: HawkesStructuredRhoParameterization,
    scale: f64,
) -> HawkesStructuredRhoParameterization {
    HawkesStructuredRhoParameterization {
        r_self: round_positive_to_domain(structured_rho.r_self * scale, 12),
        r_cross: round_to((structured_rho.r_cross * scale).max(0.0), 12),
        delta_self_r: round_to((structured_rho.delta_self_r * scale).max(0.0), 12),
        delta_cross_r: round_to((structured_rho.delta_cross_r * scale).max(0.0), 12),
    }
}

fn invalid_candidate_objective_bundle(
    candidate_input: &HawkesStageCandidateInput,
    warning_message: &str,
) -> (
    Vec<CalibrationObjectiveTermInput>,
    Vec<HawkesStageObjectiveTermDiagnostic>,
    HawkesStageImpliedMetricsDiagnostic,
    Vec<String>,
) {
    (
        vec![CalibrationObjectiveTermInput {
            term_id: "invalid_parameterization_penalty".to_string(),
            weight: 1.0,
            observed_score: 1_000_000.0,
        }],
        vec![HawkesStageObjectiveTermDiagnostic {
            term_id: "invalid_parameterization_penalty".to_string(),
            weight: 1.0,
            observed_value: 0.0,
            model_implied_value: 1_000_000.0,
            scale: 1.0,
            absolute_error: 1_000_000.0,
            normalized_error: 1_000_000.0,
            weighted_contribution: 1_000_000.0,
        }],
        HawkesStageImpliedMetricsDiagnostic {
            baseline_buy_intensity_per_hour: candidate_input.lambda_bar,
            baseline_sell_intensity_per_hour: candidate_input.lambda_bar,
            observed_open_event_rate_per_hour: observed_open_session_event_rate_per_hour(
                &candidate_input.source_regime,
            ),
            deseasonalized_open_event_rate_per_hour_target: 0.0,
            lambda_bar_scaling_ratio_applied: 1.0,
            max_runtime_interval_seasonality_factor: 1.0,
            max_runtime_effective_branching_ratio: 0.0,
            runtime_effective_branching_margin: 1.0,
            model_implied_open_event_rate_per_hour: 0.0,
            model_implied_open_buy_share: 0.5,
            model_implied_open_sell_share: 0.5,
            model_implied_interarrival_coefficient_of_variation: 0.0,
            model_implied_short_gap_ratio: 0.0,
            model_implied_known_news_activity_share: 0.0,
            model_implied_expected_event_count_over_observation_window: 0.0,
            endogenous_branching_ratio: 0.0,
            self_excitation_ratio: 0.0,
            cross_excitation_ratio: 0.0,
            exogenous_self_loading_ratio: 0.0,
            exogenous_cross_loading_ratio: 0.0,
            stability_margin: 0.0,
            event_volume_ratio_vs_total_observed: 0.0,
        },
        vec![warning_message.to_string()],
    )
}

fn derive_observed_activity_targets(
    source_regime: &HawkesStageObservedSourceRegime,
    seasonality_payload: &SeasonalityStageNormalizedPayload,
) -> HawkesObservedActivityTargets {
    let open_event_points: Vec<(i64, f64)> = seasonality_payload
        .timestamp_utc_nanos
        .iter()
        .copied()
        .zip(
            seasonality_payload
                .open_event_business_time_delta_exposure_hours_by_event
                .iter()
                .copied(),
        )
        .zip(seasonality_payload.market_state_codes.iter().copied())
        .filter_map(
            |((timestamp_utc_nanos, delta_exposure_hours), market_state_code)| {
                if market_state_code != OPEN_MARKET_STATE_CODE {
                    return None;
                }

                Some((timestamp_utc_nanos, delta_exposure_hours))
            },
        )
        .collect();

    let mut transformed_open_gaps_seconds: Vec<f64> =
        Vec::with_capacity(open_event_points.len().saturating_sub(1));

    let mut seasonality_weighted_gap_seconds_sum = 0.0;

    for ((previous_timestamp_utc_nanos, _), (current_timestamp_utc_nanos, delta_exposure_hours)) in
        open_event_points
            .iter()
            .copied()
            .zip(open_event_points.iter().copied().skip(1))
    {
        if hawkes_adjacent_open_gap(previous_timestamp_utc_nanos, current_timestamp_utc_nanos)
            .is_none()
        {
            continue;
        }

        let transformed_gap_seconds = delta_exposure_hours * SECONDS_PER_HOUR;

        seasonality_weighted_gap_seconds_sum += transformed_gap_seconds;

        transformed_open_gaps_seconds.push(transformed_gap_seconds);
    }

    let integrated_open_gap_exposure_hours =
        seasonality_weighted_gap_seconds_sum / SECONDS_PER_HOUR;

    let open_factor_exposure_hours = if integrated_open_gap_exposure_hours.is_finite()
        && integrated_open_gap_exposure_hours > 0.0
    {
        integrated_open_gap_exposure_hours
    } else {
        source_regime.open_session_duration_hours
    }
    .max(HAWKES_MINIMUM_POSITIVE_SEASONALITY_FACTOR);

    let deseasonalized_open_event_rate_per_hour =
        source_regime.open_session_tick_count as f64 / open_factor_exposure_hours;

    let deseasonalized_mean_interarrival_seconds =
        arithmetic_mean(transformed_open_gaps_seconds.as_slice())
            .unwrap_or(source_regime.open_session_mean_interarrival_seconds);

    let deseasonalized_interarrival_coefficient_of_variation =
        coefficient_of_variation(transformed_open_gaps_seconds.as_slice());

    let observed_short_gap_threshold_to_mean_ratio = source_regime
        .open_session_short_gap_threshold_seconds
        / source_regime.open_session_mean_interarrival_seconds;

    let deseasonalized_short_gap_threshold_seconds =
        deseasonalized_mean_interarrival_seconds * observed_short_gap_threshold_to_mean_ratio;

    let deseasonalized_short_gap_ratio = short_gap_ratio_from_threshold(
        transformed_open_gaps_seconds.as_slice(),
        deseasonalized_short_gap_threshold_seconds,
    );

    let deseasonalized_known_news_rate_per_hour = source_regime.known_news_open_event_count as f64
        / open_factor_exposure_hours.max(HAWKES_MINIMUM_POSITIVE_SEASONALITY_FACTOR);

    HawkesObservedActivityTargets {
        deseasonalized_open_event_rate_per_hour,
        open_factor_exposure_hours,
        deseasonalized_mean_interarrival_seconds,
        deseasonalized_interarrival_coefficient_of_variation,
        deseasonalized_short_gap_threshold_seconds,
        deseasonalized_short_gap_ratio,
        deseasonalized_known_news_rate_per_hour,
    }
}

fn adjust_candidate_for_deseasonalized_activity(
    candidate_input: HawkesStageCandidateInput,
    observed_activity_targets: HawkesObservedActivityTargets,
) -> (HawkesStageCandidateInput, f64) {
    let Ok(derived_alpha) =
        HawkesKernelLoadings::try_from_structured_alpha(candidate_input.structured_alpha)
    else {
        return (candidate_input, 1.0);
    };

    let Some(stationary_multipliers) =
        stationary_intensity_multipliers(derived_alpha, candidate_input.beta)
    else {
        return (candidate_input, 1.0);
    };

    let stationary_multiplier = stationary_multipliers.buy_intensity_multiplier.max(0.0)
        + stationary_multipliers.sell_intensity_multiplier.max(0.0);

    let candidate_open_rate_per_hour = candidate_input.lambda_bar * stationary_multiplier;

    let target_open_rate_per_hour =
        observed_activity_targets.deseasonalized_open_event_rate_per_hour;

    if !candidate_open_rate_per_hour.is_finite()
        || candidate_open_rate_per_hour <= 0.0
        || !target_open_rate_per_hour.is_finite()
        || target_open_rate_per_hour <= 0.0
    {
        return (candidate_input, 1.0);
    }

    let lambda_bar_scale = target_open_rate_per_hour / candidate_open_rate_per_hour;
    if !lambda_bar_scale.is_finite() || lambda_bar_scale <= 0.0 {
        return (candidate_input, 1.0);
    }

    let original_lambda_bar = candidate_input.lambda_bar;

    let mut adjusted_candidate_input = candidate_input;

    adjusted_candidate_input.lambda_bar =
        round_positive_to_domain(original_lambda_bar * lambda_bar_scale, 12);

    let applied_scale = adjusted_candidate_input.lambda_bar / original_lambda_bar;

    let lambda_bar_scaling_ratio_applied = if applied_scale.is_finite() && applied_scale > 0.0 {
        applied_scale
    } else {
        lambda_bar_scale
    };

    (adjusted_candidate_input, lambda_bar_scaling_ratio_applied)
}

fn check_endogenous_stationarity(
    derived_alpha: HawkesKernelLoadings,
    beta: f64,
) -> Option<EndogenousStationarityCheck> {
    let spectral_radius_alpha_over_beta = spectral_radius_alpha_over_beta(derived_alpha, beta);

    let effective_branching_ratio = spectral_radius_alpha_over_beta;
    if effective_branching_ratio >= 1.0 {
        return Some(EndogenousStationarityCheck {
            spectral_radius_alpha_over_beta,
            effective_branching_ratio,
        });
    }

    None
}

fn derive_candidate_implied_metrics(
    candidate_input: &HawkesStageCandidateInput,
    observed_activity_targets: HawkesObservedActivityTargets,
    lambda_bar_scaling_ratio_applied: f64,
    max_runtime_interval_seasonality_factor: f64,
    derived_alpha: HawkesKernelLoadings,
    derived_rho: HawkesKernelLoadings,
) -> Option<HawkesStageImpliedMetricsDiagnostic> {
    let stationary_multipliers =
        stationary_intensity_multipliers(derived_alpha, candidate_input.beta)?;

    let stationary_buy_intensity =
        candidate_input.lambda_bar * stationary_multipliers.buy_intensity_multiplier.max(0.0);

    let stationary_sell_intensity =
        candidate_input.lambda_bar * stationary_multipliers.sell_intensity_multiplier.max(0.0);

    let stationary_total_intensity = stationary_buy_intensity + stationary_sell_intensity;

    let model_implied_open_sell_share = if stationary_total_intensity > 0.0 {
        stationary_sell_intensity / stationary_total_intensity
    } else {
        0.5
    };

    let observed_open_event_rate_per_hour =
        observed_open_session_event_rate_per_hour(&candidate_input.source_regime);

    let endogenous_branching_ratio =
        spectral_radius_alpha_over_beta(derived_alpha, candidate_input.beta);

    let max_runtime_effective_branching_ratio = endogenous_branching_ratio;

    let self_excitation_ratio =
        (derived_alpha.plus_plus + derived_alpha.minus_minus) / (2.0 * candidate_input.beta);

    let cross_excitation_ratio =
        (derived_alpha.plus_minus + derived_alpha.minus_plus) / (2.0 * candidate_input.beta);

    let exogenous_self_loading_ratio =
        (derived_rho.plus_plus + derived_rho.minus_minus) / (2.0 * candidate_input.beta);

    let exogenous_cross_loading_ratio =
        (derived_rho.plus_minus + derived_rho.minus_plus) / (2.0 * candidate_input.beta);

    let model_implied_interarrival_coefficient_of_variation = (1.0
        + 2.0 * endogenous_branching_ratio / (1.0 - endogenous_branching_ratio).max(1e-9))
    .sqrt();

    let poisson_short_gap_ratio = 1.0
        - (-(stationary_total_intensity / 3600.0)
            * observed_activity_targets.deseasonalized_short_gap_threshold_seconds)
            .exp();

    let clustering_decay = (-(candidate_input.beta / 60.0)
        * observed_activity_targets.deseasonalized_short_gap_threshold_seconds)
        .exp();

    let model_implied_short_gap_ratio = (poisson_short_gap_ratio
        + (1.0 - poisson_short_gap_ratio) * endogenous_branching_ratio * clustering_decay)
        .clamp(0.0, 0.999_999);

    let news_rate_per_hour = observed_activity_targets.deseasonalized_known_news_rate_per_hour;

    let rho_average_row_sum = (derived_rho.plus_plus
        + derived_rho.plus_minus
        + derived_rho.minus_plus
        + derived_rho.minus_minus)
        / 2.0;

    let exogenous_extra_rate = if news_rate_per_hour > 0.0 {
        news_rate_per_hour * rho_average_row_sum
            / (candidate_input.beta * (1.0 - endogenous_branching_ratio).max(1e-9))
    } else {
        0.0
    };

    let model_implied_known_news_activity_share = if stationary_total_intensity > 0.0 {
        (exogenous_extra_rate / stationary_total_intensity).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let expected_event_count_over_observation_window = stationary_total_intensity
        * observed_activity_targets
            .open_factor_exposure_hours
            .max(HAWKES_MINIMUM_POSITIVE_SEASONALITY_FACTOR);

    Some(HawkesStageImpliedMetricsDiagnostic {
        baseline_buy_intensity_per_hour: candidate_input.lambda_bar,
        baseline_sell_intensity_per_hour: candidate_input.lambda_bar,
        observed_open_event_rate_per_hour,
        deseasonalized_open_event_rate_per_hour_target: observed_activity_targets
            .deseasonalized_open_event_rate_per_hour,
        lambda_bar_scaling_ratio_applied,
        max_runtime_interval_seasonality_factor,
        max_runtime_effective_branching_ratio,
        runtime_effective_branching_margin: (1.0 - max_runtime_effective_branching_ratio).max(0.0),
        model_implied_open_event_rate_per_hour: stationary_total_intensity,
        model_implied_open_buy_share: 1.0 - model_implied_open_sell_share,
        model_implied_open_sell_share,
        model_implied_interarrival_coefficient_of_variation,
        model_implied_short_gap_ratio,
        model_implied_known_news_activity_share,
        model_implied_expected_event_count_over_observation_window:
            expected_event_count_over_observation_window,
        endogenous_branching_ratio,
        self_excitation_ratio,
        cross_excitation_ratio,
        exogenous_self_loading_ratio,
        exogenous_cross_loading_ratio,
        stability_margin: (1.0 - endogenous_branching_ratio).max(0.0),
        event_volume_ratio_vs_total_observed: expected_event_count_over_observation_window
            / (candidate_input.source_regime.open_session_tick_count as f64).max(1.0),
    })
}

fn push_objective_term(
    term_id: &str,
    weight: f64,
    observed_value: f64,
    model_implied_value: f64,
    scale: f64,
    objective_terms: &mut Vec<CalibrationObjectiveTermInput>,
    term_diagnostics: &mut Vec<HawkesStageObjectiveTermDiagnostic>,
) {
    let absolute_error = (model_implied_value - observed_value).abs();
    let normalized_error = absolute_error / scale.max(1e-9);
    let weighted_contribution = weight * normalized_error;

    objective_terms.push(CalibrationObjectiveTermInput {
        term_id: term_id.to_string(),
        weight,
        observed_score: normalized_error,
    });

    term_diagnostics.push(HawkesStageObjectiveTermDiagnostic {
        term_id: term_id.to_string(),
        weight,
        observed_value,
        model_implied_value,
        scale,
        absolute_error,
        normalized_error,
        weighted_contribution,
    });
}

fn observed_open_session_event_rate_per_hour(
    source_regime: &HawkesStageObservedSourceRegime,
) -> f64 {
    source_regime.open_session_tick_count as f64
        / source_regime.open_session_duration_hours.max(1e-9)
}

fn observed_directional_imbalance_magnitude(
    source_regime: &HawkesStageObservedSourceRegime,
) -> f64 {
    ((source_regime.open_session_sell_tick_count as f64
        / source_regime.open_session_tick_count as f64)
        - 0.5)
        .abs()
        * 2.0
}

pub(crate) fn stationary_open_event_rate_per_hour(
    fitted_parameters: &HawkesStageFittedParameters,
) -> Option<f64> {
    let derived_alpha =
        HawkesKernelLoadings::try_from_structured_alpha(fitted_parameters.structured_alpha).ok()?;

    let stationary_multipliers =
        stationary_intensity_multipliers(derived_alpha, fitted_parameters.kernel_parameters.beta)?;

    Some(
        fitted_parameters.kernel_parameters.lambda_bar
            * (stationary_multipliers.buy_intensity_multiplier.max(0.0)
                + stationary_multipliers.sell_intensity_multiplier.max(0.0)),
    )
}

fn hawkes_stage_optimizer_bounds() -> Vec<CalibrationOptimizerParameterBound> {
    vec![
        CalibrationOptimizerParameterBound {
            parameter_name: "lambda_bar".to_string(),
            lower_inclusive: 0.0,
            upper_inclusive: f64::MAX,
        },
        CalibrationOptimizerParameterBound {
            parameter_name: "beta".to_string(),
            lower_inclusive: 0.0,
            upper_inclusive: f64::MAX,
        },
        CalibrationOptimizerParameterBound {
            parameter_name: "stability_numeric_tolerance".to_string(),
            lower_inclusive: 0.0,
            upper_inclusive: f64::MAX,
        },
    ]
}

fn build_hawkes_stage_handoff_outputs(
    run_spec: &CalibrationRunSpec,
) -> Vec<HawkesStageHandoffOutput> {
    run_spec
        .stage_handoffs
        .iter()
        .filter(|stage_handoff| stage_handoff.from_stage == CalibrationStage::Hawkes)
        .map(|stage_handoff| match stage_handoff.transport() {
            CalibrationStageHandoffTransport::InMemoryOutput => HawkesStageHandoffOutput {
                from_stage: stage_handoff.from_stage,
                to_stage: stage_handoff.to_stage,
                transport: stage_handoff.transport(),
                in_memory_payload_available: true,
                artifact_reference: None,
            },
            CalibrationStageHandoffTransport::PersistedArtifactReference => {
                HawkesStageHandoffOutput {
                    from_stage: stage_handoff.from_stage,
                    to_stage: stage_handoff.to_stage,
                    transport: stage_handoff.transport(),
                    in_memory_payload_available: false,
                    artifact_reference: stage_handoff.artifact_reference().map(str::to_string),
                }
            }
        })
        .collect()
}

fn build_hawkes_stage_failure_report(
    run_spec: &CalibrationRunSpec,
    failure: HawkesStageExecutionFailure,
) -> HawkesStageExecutionReport {
    HawkesStageExecutionReport {
        schema_version: CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::Hawkes,
        status: CalibrationStageExecutionStatus::Failure,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_HAWKES_STAGE_HANDOFF_POLICY,
        handoffs: Vec::new(),
        fitted_stage_payload: None,
        metadata: None,
        failure: Some(failure),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        HawkesObservedActivityTargets, HawkesPreparedCandidate,
        adjust_candidate_for_deseasonalized_activity, derive_candidate_implied_metrics,
        populate_observed_activity_targets_cache, prepare_hawkes_candidates_from_validated_inputs,
        ranked_refinement_seed_indices, refinement_alpha_scales,
    };
    use crate::adapters::precheck_structured_hawkes_kernel_parameter_domain;
    use crate::stages::hawkes::{
        HawkesStageCandidateDiagnostic, HawkesStageCandidateInput,
        HawkesStageDirectionalLabelDiagnostics, HawkesStageImpliedMetricsDiagnostic,
        HawkesStageObservedSourceRegime,
    };
    use crate::stages::seasonality::SeasonalityStageNormalizedPayload;
    use janiform_foundations::time::SECONDS_PER_HOUR;
    use janiform_market_time::business_time_clock::{
        FOURIER_BUSINESS_TIME_CLOCK_BASIS, FOURIER_BUSINESS_TIME_CLOCK_COORDINATE_SYSTEM,
        FOURIER_BUSINESS_TIME_CLOCK_MODEL_SCHEMA_VERSION,
        FOURIER_BUSINESS_TIME_CLOCK_NORMALIZATION_POLICY,
        FOURIER_BUSINESS_TIME_CLOCK_SELECTION_POLICY, FourierBusinessTimeClockModel,
        zero_order_harmonic_series_by_axis,
    };
    use janiform_model_kernels::hawkes::{
        HawkesKernelLoadings, HawkesStructuredAlphaParameterization,
        HawkesStructuredRhoParameterization, HawkesStructuredRhoSelfDominancePolicy,
    };

    #[test]
    fn ranked_refinement_seed_indices_fall_back_to_least_violating_candidates() {
        let candidate_diagnostics = vec![
            test_candidate_diagnostic("candidate_a", 2, 1.0, 1.0),
            test_candidate_diagnostic("candidate_b", 1, 2.0, 2.0),
            test_candidate_diagnostic("candidate_c", 1, 0.5, 0.5),
        ];

        let prepared_candidates = vec![
            test_prepared_candidate("candidate_a"),
            test_prepared_candidate("candidate_b"),
            test_prepared_candidate("candidate_c"),
        ];

        let ranked_seed_indices =
            ranked_refinement_seed_indices(&prepared_candidates, &candidate_diagnostics);

        assert_eq!(ranked_seed_indices, vec![2, 1]);
    }

    #[test]
    fn refinement_alpha_scales_include_targeted_stability_reduction() {
        let candidate_diagnostic =
            test_candidate_diagnostic("candidate_supercritical", 1, 1.0, 1.0);

        let alpha_scales = refinement_alpha_scales(&candidate_diagnostic);

        assert!(alpha_scales.contains(&0.095));
        assert!(alpha_scales.contains(&0.19));
        assert!(alpha_scales.iter().any(|scale| *scale < 0.2));
    }

    #[test]
    fn repeated_source_regimes_prepare_in_original_candidate_order() {
        let candidate_a = test_candidate_input("candidate_a");
        let candidate_b = test_candidate_input("candidate_b");

        let validated_candidate_inputs = vec![&candidate_a, &candidate_b];

        let mut observed_activity_targets_by_regime = BTreeMap::new();

        populate_observed_activity_targets_cache(
            validated_candidate_inputs.as_slice(),
            &test_seasonality_payload(),
            &mut observed_activity_targets_by_regime,
        );

        assert_eq!(observed_activity_targets_by_regime.len(), 1);

        let prepared_candidates = prepare_hawkes_candidates_from_validated_inputs(
            validated_candidate_inputs.as_slice(),
            &observed_activity_targets_by_regime,
            1.0,
            1,
        );

        assert_eq!(prepared_candidates.len(), 2);
        assert_eq!(
            prepared_candidates[0].adjusted_candidate_input.candidate_id,
            "candidate_a"
        );
        assert_eq!(
            prepared_candidates[1].adjusted_candidate_input.candidate_id,
            "candidate_b"
        );
    }

    #[test]
    fn candidate_adjustment_matches_deseasonalized_open_activity_target() {
        let candidate_input = HawkesStageCandidateInput {
            lambda_bar: 1.0,
            structured_alpha: HawkesStructuredAlphaParameterization {
                a_self: 5.0,
                a_cross: 0.0,
                delta_self: 0.0,
                delta_cross: 0.0,
            },
            ..test_candidate_input("candidate")
        };

        let observed_activity_targets = HawkesObservedActivityTargets {
            deseasonalized_open_event_rate_per_hour: 8.0,
            open_factor_exposure_hours: 1.0,
            deseasonalized_mean_interarrival_seconds: 1.0,
            deseasonalized_interarrival_coefficient_of_variation: 1.0,
            deseasonalized_short_gap_threshold_seconds: 1.0,
            deseasonalized_short_gap_ratio: 0.0,
            deseasonalized_known_news_rate_per_hour: 0.0,
        };

        let (adjusted_candidate, applied_ratio) = adjust_candidate_for_deseasonalized_activity(
            candidate_input,
            observed_activity_targets,
        );

        assert_eq!(adjusted_candidate.lambda_bar, 2.0);
        assert_eq!(applied_ratio, 2.0);
    }

    #[test]
    fn implied_event_count_uses_deseasonalized_open_exposure() {
        let candidate_input = test_candidate_input("candidate");

        let observed_activity_targets = HawkesObservedActivityTargets {
            deseasonalized_open_event_rate_per_hour: 2.0,
            open_factor_exposure_hours: 20.0,
            deseasonalized_mean_interarrival_seconds: 1.0,
            deseasonalized_interarrival_coefficient_of_variation: 1.0,
            deseasonalized_short_gap_threshold_seconds: 1.0,
            deseasonalized_short_gap_ratio: 0.0,
            deseasonalized_known_news_rate_per_hour: 0.0,
        };

        let implied_metrics = derive_candidate_implied_metrics(
            &candidate_input,
            observed_activity_targets,
            1.0,
            1.0,
            HawkesKernelLoadings {
                plus_plus: 0.0,
                plus_minus: 0.0,
                minus_plus: 0.0,
                minus_minus: 0.0,
            },
            HawkesKernelLoadings {
                plus_plus: 0.0,
                plus_minus: 0.0,
                minus_plus: 0.0,
                minus_minus: 0.0,
            },
        )
        .expect("zero excitation candidate admits stationary metrics");

        assert_eq!(
            implied_metrics.model_implied_expected_event_count_over_observation_window,
            40.0
        );
    }

    #[test]
    fn prepared_candidates_are_identical_across_cpu_counts() {
        let candidate_a = test_candidate_input("candidate_a");
        let candidate_b = test_candidate_input("candidate_b");

        let candidate_c = HawkesStageCandidateInput {
            candidate_id: "candidate_c".to_string(),
            source_regime: HawkesStageObservedSourceRegime {
                open_session_mean_interarrival_seconds: 2.0,
                ..test_source_regime()
            },
            ..test_candidate_input("candidate_c")
        };

        let validated_candidate_inputs = vec![&candidate_a, &candidate_b, &candidate_c];

        let mut observed_activity_targets_by_regime = BTreeMap::new();

        populate_observed_activity_targets_cache(
            validated_candidate_inputs.as_slice(),
            &test_seasonality_payload(),
            &mut observed_activity_targets_by_regime,
        );

        let single_thread_candidates = prepare_hawkes_candidates_from_validated_inputs(
            validated_candidate_inputs.as_slice(),
            &observed_activity_targets_by_regime,
            1.0,
            1,
        );

        let multi_thread_candidates = prepare_hawkes_candidates_from_validated_inputs(
            validated_candidate_inputs.as_slice(),
            &observed_activity_targets_by_regime,
            1.0,
            4,
        );

        assert_eq!(single_thread_candidates, multi_thread_candidates);
    }

    fn test_candidate_diagnostic(
        candidate_id: &str,
        constraint_violation_count: usize,
        raw_objective_value: f64,
        penalized_objective_value: f64,
    ) -> HawkesStageCandidateDiagnostic {
        HawkesStageCandidateDiagnostic {
            candidate_id: candidate_id.to_string(),
            raw_objective_value,
            constraint_penalty: 10.0,
            penalized_objective_value,
            constraint_violation_count,
            objective_terms: Vec::new(),
            source_regime: test_source_regime(),
            directional_label_diagnostics: test_directional_label_diagnostics(),
            implied_metrics: HawkesStageImpliedMetricsDiagnostic {
                baseline_buy_intensity_per_hour: 1.0,
                baseline_sell_intensity_per_hour: 1.0,
                observed_open_event_rate_per_hour: 1.0,
                deseasonalized_open_event_rate_per_hour_target: 1.0,
                lambda_bar_scaling_ratio_applied: 1.0,
                max_runtime_interval_seasonality_factor: 10.0,
                max_runtime_effective_branching_ratio: 5.0,
                runtime_effective_branching_margin: 0.0,
                model_implied_open_event_rate_per_hour: 1.0,
                model_implied_open_buy_share: 0.5,
                model_implied_open_sell_share: 0.5,
                model_implied_interarrival_coefficient_of_variation: 1.0,
                model_implied_short_gap_ratio: 0.5,
                model_implied_known_news_activity_share: 0.0,
                model_implied_expected_event_count_over_observation_window: 1.0,
                endogenous_branching_ratio: 0.5,
                self_excitation_ratio: 0.5,
                cross_excitation_ratio: 0.0,
                exogenous_self_loading_ratio: 0.0,
                exogenous_cross_loading_ratio: 0.0,
                stability_margin: 0.5,
                event_volume_ratio_vs_total_observed: 1.0,
            },
            warnings: Vec::new(),
            validation_report: precheck_structured_hawkes_kernel_parameter_domain(
                1.0,
                10.0,
                HawkesStructuredAlphaParameterization {
                    a_self: 0.1,
                    a_cross: 0.0,
                    delta_self: 0.0,
                    delta_cross: 0.0,
                },
                HawkesStructuredRhoParameterization {
                    r_self: 1.0e-12,
                    r_cross: 0.0,
                    delta_self_r: 0.0,
                    delta_cross_r: 0.0,
                },
                Some(HawkesStructuredRhoSelfDominancePolicy {
                    minimum_self_to_cross_ratio: 1.0,
                }),
                0.0,
            ),
        }
    }

    fn test_prepared_candidate(candidate_id: &str) -> HawkesPreparedCandidate {
        HawkesPreparedCandidate {
            adjusted_candidate_input: test_candidate_input(candidate_id),
            validation_report: precheck_structured_hawkes_kernel_parameter_domain(
                1.0,
                10.0,
                HawkesStructuredAlphaParameterization {
                    a_self: 0.1,
                    a_cross: 0.0,
                    delta_self: 0.0,
                    delta_cross: 0.0,
                },
                HawkesStructuredRhoParameterization {
                    r_self: 1.0e-12,
                    r_cross: 0.0,
                    delta_self_r: 0.0,
                    delta_cross_r: 0.0,
                },
                Some(HawkesStructuredRhoSelfDominancePolicy {
                    minimum_self_to_cross_ratio: 1.0,
                }),
                0.0,
            ),
            objective_terms: Vec::new(),
            objective_term_diagnostics: Vec::new(),
            implied_metrics: test_candidate_diagnostic(candidate_id, 0, 0.0, 0.0).implied_metrics,
            warnings: Vec::new(),
            external_constraint_violation_count: 0,
        }
    }

    fn test_candidate_input(candidate_id: &str) -> HawkesStageCandidateInput {
        HawkesStageCandidateInput {
            candidate_id: candidate_id.to_string(),
            lambda_bar: 1.0,
            beta: 10.0,
            structured_alpha: HawkesStructuredAlphaParameterization {
                a_self: 0.1,
                a_cross: 0.0,
                delta_self: 0.0,
                delta_cross: 0.0,
            },
            structured_rho: HawkesStructuredRhoParameterization {
                r_self: 1.0e-12,
                r_cross: 0.0,
                delta_self_r: 0.0,
                delta_cross_r: 0.0,
            },
            self_dominance_policy: Some(HawkesStructuredRhoSelfDominancePolicy {
                minimum_self_to_cross_ratio: 1.0,
            }),
            stability_numeric_tolerance: 0.0,
            source_regime: test_source_regime(),
            directional_label_diagnostics: test_directional_label_diagnostics(),
        }
    }

    fn test_source_regime() -> HawkesStageObservedSourceRegime {
        HawkesStageObservedSourceRegime {
            observation_duration_hours: 1.0,
            open_session_duration_hours: 1.0,
            total_tick_count: 10,
            open_session_tick_count: 10,
            closed_session_tick_count: 0,
            open_session_buy_tick_count: 5,
            open_session_sell_tick_count: 5,
            open_session_mean_interarrival_seconds: 1.0,
            open_session_interarrival_coefficient_of_variation: 1.0,
            open_session_short_gap_threshold_seconds: 0.5,
            open_session_short_gap_ratio: 0.5,
            known_news_open_event_count: 0,
            open_session_known_news_activity_share: 0.0,
        }
    }

    fn test_directional_label_diagnostics() -> HawkesStageDirectionalLabelDiagnostics {
        HawkesStageDirectionalLabelDiagnostics {
            policy_id: "policy".to_string(),
            first_inferred_trade_sign: "buy".to_string(),
            provided_label_count: 0,
            inferred_label_count: 0,
            flat_move_carry_count: 0,
            price_up_inference_count: 0,
            price_down_inference_count: 0,
        }
    }

    fn flat_business_time_clock_model() -> FourierBusinessTimeClockModel {
        FourierBusinessTimeClockModel {
            schema_version: FOURIER_BUSINESS_TIME_CLOCK_MODEL_SCHEMA_VERSION.to_string(),
            coordinate_system: FOURIER_BUSINESS_TIME_CLOCK_COORDINATE_SYSTEM.to_string(),
            coefficient_basis: FOURIER_BUSINESS_TIME_CLOCK_BASIS.to_string(),
            normalization_policy: FOURIER_BUSINESS_TIME_CLOCK_NORMALIZATION_POLICY.to_string(),
            selection_policy: FOURIER_BUSINESS_TIME_CLOCK_SELECTION_POLICY.to_string(),
            intercept: 0.0,
            harmonic_series: zero_order_harmonic_series_by_axis(),
            selected_objective_value: 0.0,
            selected_negative_log_likelihood: 0.0,
            evaluated_candidate_count: 1,
            fit_open_minute_count: 1,
            fit_open_event_count: 1,
            fit_open_exposure_seconds: 60.0,
        }
    }

    fn test_seasonality_payload() -> SeasonalityStageNormalizedPayload {
        SeasonalityStageNormalizedPayload {
            schema_version: "seasonality.normalized.test.v1",
            dataset_id: "dataset".to_string(),
            exchange_code: "XNYS".to_string(),
            trade_events_data_reference: "memory://trade-events".to_string(),
            timestamp_event_input_schema_version: "timestamp-input.v1".to_string(),
            timestamp_event_preprocessed_schema_version: "timestamp-preprocessed.v1".to_string(),
            clock_model_schema_version: FOURIER_BUSINESS_TIME_CLOCK_MODEL_SCHEMA_VERSION
                .to_string(),
            source_record_positions: vec![0, 1, 2],
            timestamp_utc_nanos: vec![0, 1_000_000_000, 3_000_000_000],
            exchange_local_minute_of_week: vec![570, 571, 573],
            market_state_codes: vec![1, 1, 1],
            business_time_rate_by_event: vec![1.0, 2.0, 1.0],
            open_event_business_time_delta_exposure_hours_by_event: vec![
                0.0,
                1.5 / SECONDS_PER_HOUR,
                3.0 / SECONDS_PER_HOUR,
            ],
            clock_model: flat_business_time_clock_model(),
        }
    }
}
