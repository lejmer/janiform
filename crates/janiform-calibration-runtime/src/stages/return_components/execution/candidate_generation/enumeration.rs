use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::{
    CalibrationOptimizerSettings, SurpriseNewsDeterministicIntensityProfile,
};
use janiform_foundations::cancellation::CancellationToken;
use janiform_model_kernels::market_pressure::MarketPressureParameters;
use janiform_model_kernels::news::{
    KnownNewsArrivalConfiguration, NewsArrivalConfiguration, NewsComponentMarkScaleConfiguration,
    NewsProcessConfiguration, SurpriseNewsArrivalConfiguration,
};
use janiform_model_kernels::return_clock::ReturnClockExposureParameters;
use janiform_work_scheduler::{StaticWorkPoolInput, run_static_work_pool};

use super::super::diagnostics::{
    build_activity_coupling_diagnostic, build_gap_diagnostic, build_jump_diagnostic,
    build_price_range_sanity_diagnostic, build_surprise_arrival_diagnostic,
};
use super::families::ReturnComponentsStageCandidateFamilySets;
use crate::stages::parallelism::resolved_stage_cpu_count;
use crate::stages::return_components::continuous_layer_fit::FittedContinuousReturnCandidate;
use crate::stages::return_components::contracts::{
    ReturnComponentsActivityCouplingCandidateFamily,
    ReturnComponentsKnownNewsArrivalCandidateFamily,
    ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    ReturnComponentsStageActivityCouplingDiagnostic, ReturnComponentsStageCandidateFamilies,
    ReturnComponentsStageCandidateGenerationInput,
    ReturnComponentsStageCandidateGenerationInputError, ReturnComponentsStageCandidateInput,
    ReturnComponentsStageCandidateProvenance, ReturnComponentsStageDiagnosticPolicy,
    ReturnComponentsStageGapDiagnostic, ReturnComponentsStageJumpDiagnostic,
    ReturnComponentsStageObjectiveMetricBuckets, ReturnComponentsStageObjectiveScoringPolicy,
    ReturnComponentsStageObjectiveTermInput, ReturnComponentsStageObservedSignalSummaryStatistics,
    ReturnComponentsStagePriceRangeSanityDiagnostic, ReturnComponentsStagePriceRangeSanityPolicy,
    ReturnComponentsStageProgressPhase, ReturnComponentsStageProgressUpdate,
    ReturnComponentsStageSurpriseArrivalDiagnostic,
    ReturnComponentsSurpriseNewsArrivalCandidateFamily,
};
use crate::stages::return_components::objective_scoring_policy_fit;
use crate::stages::return_components::progress::emit_return_components_progress;

type CandidateBuildResult =
    Result<ReturnComponentsStageCandidateInput, ReturnComponentsStageCandidateGenerationInputError>;
type CandidateBuildSlots = Vec<Option<CandidateBuildResult>>;

pub(super) fn enumerate_candidate_inputs(
    input: CandidateInputsEnumerationInput<'_>,
) -> Result<
    Vec<ReturnComponentsStageCandidateInput>,
    ReturnComponentsStageCandidateGenerationInputError,
> {
    let context = CandidateEnumerationContext::build(input);
    let candidate_selections = build_candidate_family_selections(&context);

    let total_candidate_count = candidate_selections.len();

    emit_candidate_enumeration_start(context.progress_sender, total_candidate_count);

    let worker_count = resolved_stage_cpu_count(context.stage_solver_settings.cpu_count);
    if worker_count > 1 && total_candidate_count > worker_count {
        return enumerate_candidate_inputs_parallel(&context, candidate_selections, worker_count);
    }

    enumerate_candidate_inputs_serial(&context, candidate_selections.as_slice())
}

pub(super) struct CandidateInputsEnumerationInput<'a> {
    pub(super) candidate_generation_input: &'a ReturnComponentsStageCandidateGenerationInput,
    pub(super) candidate_family_sets: &'a ReturnComponentsStageCandidateFamilySets,
    pub(super) stage_solver_settings: &'a CalibrationOptimizerSettings,
    pub(super) return_clock_exposure_parameters: ReturnClockExposureParameters,
    pub(super) summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    pub(super) fitted_surprise_deterministic_intensity_profile:
        &'a SurpriseNewsDeterministicIntensityProfile,
    pub(super) candidate_provenance_template: &'a ReturnComponentsStageCandidateProvenance,
    pub(super) fitted_diagnostic_policy: &'a ReturnComponentsStageDiagnosticPolicy,
    pub(super) fitted_objective_scoring_policy: &'a ReturnComponentsStageObjectiveScoringPolicy,
    pub(super) fitted_price_range_sanity_policy: &'a ReturnComponentsStagePriceRangeSanityPolicy,
    pub(super) progress_sender: Option<&'a Sender<ReturnComponentsStageProgressUpdate>>,
}

struct CandidateEnumerationContext<'a> {
    candidate_generation_input: &'a ReturnComponentsStageCandidateGenerationInput,
    candidate_family_sets: &'a ReturnComponentsStageCandidateFamilySets,
    stage_solver_settings: &'a CalibrationOptimizerSettings,
    return_clock_exposure_parameters: ReturnClockExposureParameters,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    fitted_surprise_deterministic_intensity_profile: &'a SurpriseNewsDeterministicIntensityProfile,
    candidate_provenance_template: &'a ReturnComponentsStageCandidateProvenance,
    fitted_diagnostic_policy: &'a ReturnComponentsStageDiagnosticPolicy,
    fitted_objective_scoring_policy: &'a ReturnComponentsStageObjectiveScoringPolicy,
    fitted_price_range_sanity_policy: &'a ReturnComponentsStagePriceRangeSanityPolicy,
    known_arrival_family: ReturnComponentsKnownNewsArrivalCandidateFamily,
    progress_sender: Option<&'a Sender<ReturnComponentsStageProgressUpdate>>,
}

impl<'a> CandidateEnumerationContext<'a> {
    fn build(input: CandidateInputsEnumerationInput<'a>) -> Self {
        let known_arrival_family = input
            .candidate_family_sets
            .fitted_known_arrival_layer
            .as_candidate_family();

        Self {
            candidate_generation_input: input.candidate_generation_input,
            candidate_family_sets: input.candidate_family_sets,
            stage_solver_settings: input.stage_solver_settings,
            return_clock_exposure_parameters: input.return_clock_exposure_parameters,
            summary_statistics: input.summary_statistics,
            fitted_surprise_deterministic_intensity_profile: input
                .fitted_surprise_deterministic_intensity_profile,
            candidate_provenance_template: input.candidate_provenance_template,
            fitted_diagnostic_policy: input.fitted_diagnostic_policy,
            fitted_objective_scoring_policy: input.fitted_objective_scoring_policy,
            fitted_price_range_sanity_policy: input.fitted_price_range_sanity_policy,
            known_arrival_family,
            progress_sender: input.progress_sender,
        }
    }
}

#[derive(Clone, Copy)]
struct CandidateFamilySelection<'a> {
    candidate_position: usize,
    nig_candidate_index: usize,
    surprise_arrival_family_index: usize,
    return_jump_family_index: usize,
    activity_coupling_family_index: usize,
    market_pressure_family_index: usize,
    continuous_return_candidate: &'a FittedContinuousReturnCandidate,
    surprise_arrival_family: &'a ReturnComponentsSurpriseNewsArrivalCandidateFamily,
    return_jump_family: &'a ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    activity_coupling_family: &'a ReturnComponentsActivityCouplingCandidateFamily,
    market_pressure_parameters: &'a MarketPressureParameters,
}

fn enumerate_candidate_inputs_serial(
    context: &CandidateEnumerationContext<'_>,
    candidate_selections: &[CandidateFamilySelection<'_>],
) -> Result<
    Vec<ReturnComponentsStageCandidateInput>,
    ReturnComponentsStageCandidateGenerationInputError,
> {
    let mut candidate_inputs = Vec::with_capacity(candidate_selections.len());

    for candidate_selection in candidate_selections {
        let candidate_input = build_candidate_input(context, *candidate_selection)?;
        emit_candidate_enumerated(
            context,
            candidate_selection.candidate_position,
            candidate_selections.len(),
            candidate_input.candidate_id.as_str(),
        );
        candidate_inputs.push(candidate_input);
    }

    Ok(candidate_inputs)
}

fn enumerate_candidate_inputs_parallel(
    context: &CandidateEnumerationContext<'_>,
    candidate_selections: Vec<CandidateFamilySelection<'_>>,
    worker_count: usize,
) -> Result<
    Vec<ReturnComponentsStageCandidateInput>,
    ReturnComponentsStageCandidateGenerationInputError,
> {
    let mut candidate_inputs = std::iter::repeat_with(|| None)
        .take(candidate_selections.len())
        .collect::<CandidateBuildSlots>();

    run_static_work_pool(
        None,
        StaticWorkPoolInput::new(
            &mut candidate_inputs,
            CancellationToken::uncancelled(),
            candidate_selections,
            worker_count,
            |candidate_selection: CandidateFamilySelection<'_>, _cancellation_token| {
                (
                    candidate_selection.candidate_position,
                    build_candidate_input(context, candidate_selection),
                )
            },
            |candidate_inputs: &mut CandidateBuildSlots,
             (candidate_position, candidate_input),
             _status| {
                if let Some(candidate_slot) = candidate_inputs.get_mut(candidate_position) {
                    *candidate_slot = Some(candidate_input);
                }
            },
        ),
    );

    let mut built_candidates = Vec::with_capacity(candidate_inputs.len());
    let total_candidate_count = candidate_inputs.len();

    for (candidate_position, candidate_slot) in candidate_inputs.into_iter().enumerate() {
        let Some(candidate_input) = candidate_slot else {
            return Err(
                ReturnComponentsStageCandidateGenerationInputError::MissingCandidateEnumerationWorkerResult {
                    candidate_position,
                },
            );
        };

        built_candidates.push(candidate_input?);
    }

    emit_candidate_inputs_enumerated(context, built_candidates.as_slice(), total_candidate_count);

    Ok(built_candidates)
}

fn build_candidate_family_selections<'a>(
    context: &'a CandidateEnumerationContext<'a>,
) -> Vec<CandidateFamilySelection<'a>> {
    let mut candidate_selections = Vec::new();

    for (nig_candidate_index, continuous_return_candidate) in context
        .candidate_family_sets
        .fitted_continuous_return_layer
        .candidates
        .iter()
        .enumerate()
    {
        for (surprise_arrival_family_index, surprise_arrival_family) in context
            .candidate_family_sets
            .fitted_surprise_arrival_layer
            .iter()
            .enumerate()
        {
            for (return_jump_family_index, return_jump_family) in context
                .candidate_family_sets
                .fitted_return_jump_mark_layer
                .iter()
                .enumerate()
            {
                for (activity_coupling_family_index, activity_coupling_family) in context
                    .candidate_family_sets
                    .fitted_activity_coupling_layer
                    .iter()
                    .enumerate()
                {
                    for (market_pressure_family_index, market_pressure_parameters) in context
                        .candidate_family_sets
                        .fitted_market_pressure_layer
                        .iter()
                        .enumerate()
                    {
                        if !news_mark_scale_components_are_compatible(
                            return_jump_family,
                            activity_coupling_family,
                        ) {
                            continue;
                        }

                        candidate_selections.push(CandidateFamilySelection {
                            candidate_position: candidate_selections.len(),
                            nig_candidate_index,
                            surprise_arrival_family_index,
                            return_jump_family_index,
                            activity_coupling_family_index,
                            market_pressure_family_index,
                            continuous_return_candidate,
                            surprise_arrival_family,
                            return_jump_family,
                            activity_coupling_family,
                            market_pressure_parameters,
                        });
                    }
                }
            }
        }
    }

    candidate_selections
}

fn news_mark_scale_components_are_compatible(
    return_jump_family: &ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    activity_coupling_family: &ReturnComponentsActivityCouplingCandidateFamily,
) -> bool {
    return_jump_component_is_compatible(
        return_jump_family.known_return_jump_drift,
        return_jump_family.known_return_jump_volatility,
        activity_coupling_family.known_activity_coupling_scale,
    ) && return_jump_component_is_compatible(
        return_jump_family.surprise_return_jump_drift,
        return_jump_family.surprise_return_jump_volatility,
        activity_coupling_family.surprise_activity_coupling_scale,
    )
}

fn return_jump_component_is_compatible(
    return_jump_drift: f64,
    return_jump_volatility: f64,
    activity_coupling_scale: f64,
) -> bool {
    (return_jump_drift == 0.0 && return_jump_volatility == 0.0) || activity_coupling_scale > 0.0
}

fn emit_candidate_enumeration_start(
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
    total_candidate_count: usize,
) {
    emit_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::CandidateEnumeration,
        0,
        total_candidate_count,
        Some("enumerate generated candidates"),
    );
}

fn build_candidate_input(
    context: &CandidateEnumerationContext<'_>,
    candidate_selection: CandidateFamilySelection<'_>,
) -> Result<ReturnComponentsStageCandidateInput, ReturnComponentsStageCandidateGenerationInputError>
{
    let surprise_arrival_diagnostic =
        build_candidate_surprise_arrival_diagnostic(context, candidate_selection);
    let jump_diagnostic = build_candidate_jump_diagnostic(context, candidate_selection);
    let gap_diagnostic = build_candidate_gap_diagnostic(context, candidate_selection);
    let activity_coupling_diagnostic =
        build_candidate_activity_coupling_diagnostic(context, candidate_selection);

    let candidate_families = build_candidate_families(context, candidate_selection);

    let price_range_sanity_diagnostic =
        build_candidate_price_range_sanity_diagnostic(context, candidate_selection);

    let objective_components = build_candidate_objective_components(
        context,
        candidate_selection.continuous_return_candidate,
        &surprise_arrival_diagnostic,
        &jump_diagnostic,
        &gap_diagnostic,
        &activity_coupling_diagnostic,
        &price_range_sanity_diagnostic,
    );

    let news_process_configuration =
        build_news_process_configuration_from_candidate_families(context, candidate_selection)?;

    let candidate_id = generated_candidate_id(candidate_selection);
    // Candidate inputs own fit diagnostics because stage reports persist them independently.
    let continuous_distribution_fit_breakdown = candidate_selection
        .continuous_return_candidate
        .evaluation
        .fit_breakdown
        .clone();
    // Candidate inputs own provenance and surface contracts for artifact serialization.
    let candidate_provenance = context.candidate_provenance_template.clone();
    let price_discretization_surface = context
        .candidate_generation_input
        .price_discretization_surface
        .clone();
    let timestamp_batching_surface = context
        .candidate_generation_input
        .timestamp_batching_surface
        .clone();

    let candidate_input = ReturnComponentsStageCandidateInput {
        candidate_id,
        nig_shared_plus_shift_configuration: candidate_selection
            .continuous_return_candidate
            .nig_configuration,
        news_process_configuration,
        objective_metric_buckets: objective_components.metric_buckets,
        return_distribution_diagnostic: candidate_selection
            .continuous_return_candidate
            .evaluation
            .diagnostic,
        continuous_distribution_fit_breakdown,
        surprise_arrival_diagnostic,
        jump_diagnostic,
        gap_diagnostic,
        activity_coupling_diagnostic,
        price_range_sanity_diagnostic,
        objective_terms: objective_components.terms,
        candidate_families,
        candidate_provenance,
        dominant_scale_driver: objective_components.dominant_scale_driver,
        // Each generated candidate owns the fitted pressure contract.
        market_pressure_parameters: candidate_selection.market_pressure_parameters.clone(),
        return_clock_exposure_parameters: context.return_clock_exposure_parameters,
        price_discretization_surface,
        timestamp_batching_surface,
    };

    Ok(candidate_input)
}

fn build_candidate_families(
    context: &CandidateEnumerationContext<'_>,
    candidate_selection: CandidateFamilySelection<'_>,
) -> ReturnComponentsStageCandidateFamilies {
    // Each candidate owns its serializable family controls.
    let known_news_arrival_controls = context.known_arrival_family.clone();
    let surprise_news_arrival_controls = candidate_selection.surprise_arrival_family.clone();

    ReturnComponentsStageCandidateFamilies {
        known_news_arrival_controls,
        surprise_news_arrival_controls,
        return_jump_mark_scale_controls: *candidate_selection.return_jump_family,
        news_to_hawkes_activity_coupling_controls: *candidate_selection.activity_coupling_family,
    }
}

fn build_candidate_surprise_arrival_diagnostic(
    context: &CandidateEnumerationContext<'_>,
    candidate_selection: CandidateFamilySelection<'_>,
) -> ReturnComponentsStageSurpriseArrivalDiagnostic {
    build_surprise_arrival_diagnostic(
        context.fitted_diagnostic_policy,
        context.summary_statistics,
        candidate_selection.surprise_arrival_family,
        context.fitted_surprise_deterministic_intensity_profile,
    )
}

fn build_candidate_jump_diagnostic(
    context: &CandidateEnumerationContext<'_>,
    candidate_selection: CandidateFamilySelection<'_>,
) -> ReturnComponentsStageJumpDiagnostic {
    build_jump_diagnostic(
        context.fitted_diagnostic_policy,
        context.summary_statistics,
        candidate_selection.return_jump_family,
        context.candidate_generation_input.shared_mark_law_template,
    )
}

fn build_candidate_gap_diagnostic(
    context: &CandidateEnumerationContext<'_>,
    candidate_selection: CandidateFamilySelection<'_>,
) -> ReturnComponentsStageGapDiagnostic {
    build_gap_diagnostic(
        context.fitted_diagnostic_policy,
        context.summary_statistics,
        candidate_selection.return_jump_family,
        context.candidate_generation_input.shared_mark_law_template,
    )
}

fn build_candidate_activity_coupling_diagnostic(
    context: &CandidateEnumerationContext<'_>,
    candidate_selection: CandidateFamilySelection<'_>,
) -> ReturnComponentsStageActivityCouplingDiagnostic {
    build_activity_coupling_diagnostic(
        context.fitted_diagnostic_policy,
        context.summary_statistics,
        candidate_selection.activity_coupling_family,
    )
}

fn build_candidate_price_range_sanity_diagnostic(
    context: &CandidateEnumerationContext<'_>,
    candidate_selection: CandidateFamilySelection<'_>,
) -> ReturnComponentsStagePriceRangeSanityDiagnostic {
    build_price_range_sanity_diagnostic(
        context.fitted_diagnostic_policy,
        context.summary_statistics,
        candidate_selection
            .continuous_return_candidate
            .evaluation
            .diagnostic,
        candidate_selection.surprise_arrival_family,
        candidate_selection.return_jump_family,
        context.candidate_generation_input.shared_mark_law_template,
        context.fitted_price_range_sanity_policy,
    )
}

fn build_candidate_objective_components(
    context: &CandidateEnumerationContext<'_>,
    continuous_return_candidate: &FittedContinuousReturnCandidate,
    surprise_arrival_diagnostic: &ReturnComponentsStageSurpriseArrivalDiagnostic,
    jump_diagnostic: &ReturnComponentsStageJumpDiagnostic,
    gap_diagnostic: &ReturnComponentsStageGapDiagnostic,
    activity_coupling_diagnostic: &ReturnComponentsStageActivityCouplingDiagnostic,
    price_range_sanity_diagnostic: &ReturnComponentsStagePriceRangeSanityDiagnostic,
) -> CandidateObjectiveComponents {
    let metric_buckets = objective_scoring_policy_fit::score_candidate_metric_buckets(
        context.fitted_objective_scoring_policy,
        continuous_return_candidate.evaluation.objective_score,
        surprise_arrival_diagnostic,
        jump_diagnostic,
        gap_diagnostic,
        activity_coupling_diagnostic,
        price_range_sanity_diagnostic,
    );

    let dominant_scale_driver =
        objective_scoring_policy_fit::dominant_scale_driver_from_metric_buckets(
            context.fitted_objective_scoring_policy,
            metric_buckets,
        );

    let terms = objective_scoring_policy_fit::build_candidate_objective_terms(
        context.candidate_generation_input.objective_metric_weights,
        metric_buckets,
    );

    CandidateObjectiveComponents {
        metric_buckets,
        terms,
        dominant_scale_driver,
    }
}

struct CandidateObjectiveComponents {
    metric_buckets: ReturnComponentsStageObjectiveMetricBuckets,
    terms: Vec<ReturnComponentsStageObjectiveTermInput>,
    dominant_scale_driver: String,
}

fn build_news_process_configuration_from_candidate_families(
    context: &CandidateEnumerationContext<'_>,
    candidate_selection: CandidateFamilySelection<'_>,
) -> Result<NewsProcessConfiguration, ReturnComponentsStageCandidateGenerationInputError> {
    let known_arrival_family = &context.known_arrival_family;
    let surprise_arrival_family = candidate_selection.surprise_arrival_family;
    let return_jump_family = candidate_selection.return_jump_family;
    let activity_coupling_family = candidate_selection.activity_coupling_family;
    // News process configurations own known-event timestamps for independent candidate payloads.
    let scheduled_event_timestamps_utc_nanos = known_arrival_family
        .retained_known_event_timestamps_utc_nanos
        .clone();

    NewsProcessConfiguration::try_new(
        NewsArrivalConfiguration {
            known: KnownNewsArrivalConfiguration {
                scheduled_event_timestamps_utc_nanos,
            },
            surprise: SurpriseNewsArrivalConfiguration {
                open_baseline_intensity: surprise_arrival_family.open_baseline_intensity,
                closed_baseline_intensity: surprise_arrival_family.closed_baseline_intensity,
            },
        },
        context.candidate_generation_input.shared_mark_law_template,
        known_component_mark_scale(activity_coupling_family, return_jump_family),
        surprise_component_mark_scale(activity_coupling_family, return_jump_family),
    )
    .map_err(|source| {
        ReturnComponentsStageCandidateGenerationInputError::InvalidSharedMarkLawTemplate {
            error_message: source.to_string(),
        }
    })
}

fn known_component_mark_scale(
    activity_coupling_family: &ReturnComponentsActivityCouplingCandidateFamily,
    return_jump_family: &ReturnComponentsReturnJumpMarkScaleCandidateFamily,
) -> NewsComponentMarkScaleConfiguration {
    NewsComponentMarkScaleConfiguration {
        activity_coupling_scale: activity_coupling_family.known_activity_coupling_scale,
        return_jump_drift: return_jump_family.known_return_jump_drift,
        return_jump_volatility: return_jump_family.known_return_jump_volatility,
    }
}

fn surprise_component_mark_scale(
    activity_coupling_family: &ReturnComponentsActivityCouplingCandidateFamily,
    return_jump_family: &ReturnComponentsReturnJumpMarkScaleCandidateFamily,
) -> NewsComponentMarkScaleConfiguration {
    NewsComponentMarkScaleConfiguration {
        activity_coupling_scale: activity_coupling_family.surprise_activity_coupling_scale,
        return_jump_drift: return_jump_family.surprise_return_jump_drift,
        return_jump_volatility: return_jump_family.surprise_return_jump_volatility,
    }
}

fn generated_candidate_id(candidate_selection: CandidateFamilySelection<'_>) -> String {
    format!(
        "generated_nig_{:03}_known_001_surprise_{:03}_mark_{:03}_coupling_{:03}_pressure_{:03}",
        candidate_selection.nig_candidate_index + 1,
        candidate_selection.surprise_arrival_family_index + 1,
        candidate_selection.return_jump_family_index + 1,
        candidate_selection.activity_coupling_family_index + 1,
        candidate_selection.market_pressure_family_index + 1,
    )
}

fn emit_candidate_enumerated(
    context: &CandidateEnumerationContext<'_>,
    candidate_position: usize,
    total_candidate_count: usize,
    candidate_id: &str,
) {
    emit_return_components_progress(
        context.progress_sender,
        ReturnComponentsStageProgressPhase::CandidateEnumeration,
        candidate_position + 1,
        total_candidate_count,
        Some(candidate_id),
    );
}

fn emit_candidate_inputs_enumerated(
    context: &CandidateEnumerationContext<'_>,
    candidate_inputs: &[ReturnComponentsStageCandidateInput],
    total_candidate_count: usize,
) {
    for (candidate_position, candidate_input) in candidate_inputs.iter().enumerate() {
        emit_candidate_enumerated(
            context,
            candidate_position,
            total_candidate_count,
            candidate_input.candidate_id.as_str(),
        );
    }
}
