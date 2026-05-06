use std::cell::Cell;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_engine_pricing::internal::SpectralThetaWarmStartState;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

use crate::stages::hawkes::stationary_open_event_rate_per_hour;
use crate::stages::joint_metrics::build_continuous_calibration_sample_from_aggregated_intervals;
use crate::stages::joint_refinement::contracts::{
    JointRefinementStageExecutionFailure, JointRefinementStageProgressPhase,
    JointRefinementStageProgressUpdate,
};
use crate::stages::joint_refinement::option_surface_tiering::JointRefinementOptionSurfaceTierKind;
use crate::stages::optimization::{
    BoundedTrustRegionOptimizerSettings, optimize_bounded_trust_region_quadratic,
};
use crate::stages::return_components::{
    NigShapeSearchPoint,
    exact_configuration_and_gradient_from_shape_search_point_for_calibration_sample,
};

use super::super::setup::parameter_vector_to_fitted_parameters;
use super::selection::admissibility_threshold_from_sample_count;
use super::{
    JointRefinementContext, JointRefinementOptionTierPricingArtifact,
    JointRefinementRestartOutcome, JointRefinementResumeStore, OptionPricingProgressInput,
    OptionSurfaceTierPricingArtifactResolutionInput, joint_refinement_parallel_worker_count,
    ordered_parameter_bits, resolve_option_surface_tier_pricing_artifact,
    with_joint_refinement_thread_pool,
};

pub(crate) const JOINT_REFINEMENT_DEFAULT_OPTION_REFINEMENT_CANDIDATE_COUNT: usize = 5;

#[derive(Debug)]
struct OptionRefinementProgressContext<'a> {
    progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    phase: JointRefinementStageProgressPhase,
    total_units: usize,
    current_restart_id: &'a str,
    shared_progress_state: Arc<OptionRefinementSharedProgressState>,
    current_evaluation_count: Cell<usize>,
    total_evaluation_budget: usize,
}

impl OptionRefinementProgressContext<'_> {
    fn record_evaluation_started(&self) {
        self.current_evaluation_count
            .set(self.current_evaluation_count.get().saturating_add(1));
    }

    fn current_evaluation_count(&self) -> usize {
        self.current_evaluation_count.get()
    }

    const fn total_evaluation_budget(&self) -> usize {
        self.total_evaluation_budget
    }

    fn running_evaluation_progress_percent(&self) -> usize {
        self.current_evaluation_count()
            .saturating_mul(100)
            .checked_div(self.total_evaluation_budget.max(1))
            .unwrap_or(0)
            .min(99)
    }

    fn shared_progress_snapshot(&self) -> OptionRefinementSharedProgressSnapshot {
        self.shared_progress_state.snapshot()
    }
}

#[derive(Debug, Clone)]
struct OptionRefinementSharedProgressSnapshot {
    completed_units: usize,
    active_candidate_count: usize,
    best_restart_id: Option<String>,
    best_weighted_objective: Option<f64>,
}

#[derive(Debug, Default)]
struct OptionRefinementSharedProgressState {
    completed_units: AtomicUsize,
    active_candidate_count: AtomicUsize,
    best_completed_candidate: Mutex<Option<(String, f64)>>,
}

impl OptionRefinementSharedProgressState {
    fn snapshot(&self) -> OptionRefinementSharedProgressSnapshot {
        let best_candidate_guard = self
            .best_completed_candidate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let (best_restart_id, best_weighted_objective) = best_candidate_guard
            .as_ref()
            .map(|(restart_id, weighted_objective)| {
                (Some(restart_id.clone()), Some(*weighted_objective))
            })
            .unwrap_or((None, None));

        OptionRefinementSharedProgressSnapshot {
            completed_units: self.completed_units.load(Ordering::Relaxed),
            active_candidate_count: self.active_candidate_count.load(Ordering::Relaxed),
            best_restart_id,
            best_weighted_objective,
        }
    }

    fn record_candidate_started(&self) -> OptionRefinementSharedProgressSnapshot {
        self.active_candidate_count.fetch_add(1, Ordering::Relaxed);
        self.snapshot()
    }

    fn record_candidate_completed(
        &self,
        restart_id: &str,
        weighted_objective: Option<f64>,
    ) -> OptionRefinementSharedProgressSnapshot {
        if let Some(weighted_objective) = weighted_objective {
            let mut best_candidate_guard = self
                .best_completed_candidate
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());

            let should_replace_best = best_candidate_guard
                .as_ref()
                .map(|(_, best_weighted_objective)| weighted_objective < *best_weighted_objective)
                .unwrap_or(true);
            if should_replace_best {
                *best_candidate_guard = Some((restart_id.to_string(), weighted_objective));
            }

            drop(best_candidate_guard);
        }

        self.completed_units.fetch_add(1, Ordering::Relaxed);
        self.active_candidate_count.fetch_sub(1, Ordering::Relaxed);

        self.snapshot()
    }

    fn record_resumed_candidate(
        &self,
        restart_id: &str,
        weighted_objective: f64,
    ) -> OptionRefinementSharedProgressSnapshot {
        let mut best_candidate_guard = self
            .best_completed_candidate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let should_replace_best = best_candidate_guard
            .as_ref()
            .map(|(_, best_weighted_objective)| weighted_objective < *best_weighted_objective)
            .unwrap_or(true);
        if should_replace_best {
            *best_candidate_guard = Some((restart_id.to_string(), weighted_objective));
        }

        drop(best_candidate_guard);

        self.completed_units.fetch_add(1, Ordering::Relaxed);
        self.snapshot()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct OptionRefinedCandidate {
    pub(crate) source_rank: usize,
    pub(crate) search_tier_score: f64,
    pub(crate) selection_tier_score: f64,
    pub(crate) search_tier_pricing_artifact: Arc<JointRefinementOptionTierPricingArtifact>,
    pub(crate) selection_tier_pricing_artifact: Arc<JointRefinementOptionTierPricingArtifact>,
    pub(crate) restart_outcome: JointRefinementRestartOutcome,
}

#[derive(Debug, Clone)]
pub(crate) struct SearchTierCandidate {
    pub(crate) source_rank: usize,
    pub(crate) source_dynamics_parameter_bits: Vec<u64>,
    pub(crate) search_tier_score: f64,
    pub(crate) search_tier_pricing_artifact: Arc<JointRefinementOptionTierPricingArtifact>,
    pub(crate) restart_outcome: JointRefinementRestartOutcome,
}

#[derive(Debug, Clone)]
pub(crate) struct JointRefinementOptionRefinementResult {
    pub(crate) selected_restart_outcome: JointRefinementRestartOutcome,
    pub(crate) selected_search_tier_pricing_artifact: Arc<JointRefinementOptionTierPricingArtifact>,
    pub(crate) selected_selection_tier_pricing_artifact:
        Arc<JointRefinementOptionTierPricingArtifact>,
}

pub(crate) fn desired_joint_refinement_option_refinement_candidate_count(
    candidate_count_override: Option<usize>,
) -> usize {
    candidate_count_override
        .filter(|candidate_count| *candidate_count > 0)
        .unwrap_or(JOINT_REFINEMENT_DEFAULT_OPTION_REFINEMENT_CANDIDATE_COUNT)
}

pub(crate) fn run_option_refinement(
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    dynamics_candidates: &[JointRefinementRestartOutcome],
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    mut resume_store: Option<&mut JointRefinementResumeStore>,
) -> Result<JointRefinementOptionRefinementResult, JointRefinementStageExecutionFailure> {
    if context.option_pricing_context.is_none() {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: "joint option refinement requires an option pricing context"
                    .to_string(),
            },
        );
    }

    if dynamics_candidates.is_empty() {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint option refinement requires at least one coarse-consistent dynamics candidate"
                        .to_string(),
            },
        );
    }

    let search_candidates = refine_search_tier_candidates(
        context,
        solver_settings,
        dynamics_candidates,
        progress_sender,
        resume_store.as_deref_mut(),
    );
    if search_candidates.is_empty() {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint option refinement failed for every search-tier dynamics candidate"
                        .to_string(),
            },
        );
    }

    let selection_frontier = select_option_refinement_selection_frontier(search_candidates);

    let selection_candidates = evaluate_selection_tier_candidates(
        context,
        selection_frontier,
        progress_sender,
        resume_store,
    );

    let Some(best_candidate) = best_selection_tier_candidate(selection_candidates) else {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint option refinement failed for every selection-tier dynamics candidate"
                        .to_string(),
            },
        );
    };

    Ok(JointRefinementOptionRefinementResult {
        selected_restart_outcome: best_candidate.restart_outcome,
        selected_search_tier_pricing_artifact: best_candidate.search_tier_pricing_artifact,
        selected_selection_tier_pricing_artifact: best_candidate.selection_tier_pricing_artifact,
    })
}

fn select_option_refinement_selection_frontier(
    search_candidates: Vec<SearchTierCandidate>,
) -> Vec<SearchTierCandidate> {
    let Some(best_search_tier_score) = search_candidates
        .iter()
        .map(|candidate| candidate.search_tier_score)
        .min_by(f64::total_cmp)
    else {
        return search_candidates;
    };

    let effective_sample_count = search_candidates
        .iter()
        .map(option_refinement_effective_search_sample_count)
        .max()
        .unwrap_or(1);

    let admissible_score_ceiling =
        option_refinement_selection_score_ceiling(best_search_tier_score, effective_sample_count);

    search_candidates
        .into_iter()
        .filter(|candidate| candidate.search_tier_score <= admissible_score_ceiling)
        .collect()
}

fn option_refinement_effective_search_sample_count(candidate: &SearchTierCandidate) -> usize {
    candidate
        .search_tier_pricing_artifact
        .maturity_scores
        .iter()
        .map(|maturity_score| maturity_score.maturity_weight)
        .sum::<usize>()
        .max(1)
}

fn option_refinement_selection_score_ceiling(
    best_search_tier_score: f64,
    effective_sample_count: usize,
) -> f64 {
    best_search_tier_score
        * admissibility_threshold_from_sample_count(effective_sample_count.max(1))
}

fn resolved_option_refinement_candidate_parallelism(
    available_stage_thread_count: usize,
    total_pricing_cpu_count: usize,
    candidate_count: usize,
) -> usize {
    candidate_count
        .min(available_stage_thread_count.max(1))
        .min(total_pricing_cpu_count.max(1))
        .max(1)
}

fn resolved_option_refinement_candidate_pricing_cpu_count(
    total_pricing_cpu_count: usize,
    candidate_parallelism: usize,
) -> usize {
    total_pricing_cpu_count
        .max(1)
        .checked_div(candidate_parallelism.max(1))
        .unwrap_or(0)
        .max(1)
}

fn option_refinement_candidate_parallelism(
    context: &JointRefinementContext,
    candidate_count: usize,
) -> usize {
    let total_pricing_cpu_count = context
        .option_pricing_context
        .as_ref()
        .map(|option_pricing_context| option_pricing_context.cpu_count)
        .unwrap_or(1);

    resolved_option_refinement_candidate_parallelism(
        joint_refinement_parallel_worker_count(context),
        total_pricing_cpu_count,
        candidate_count,
    )
}

fn option_refinement_candidate_pricing_cpu_count(
    context: &JointRefinementContext,
    candidate_parallelism: usize,
) -> usize {
    let total_pricing_cpu_count = context
        .option_pricing_context
        .as_ref()
        .map(|option_pricing_context| option_pricing_context.cpu_count)
        .unwrap_or(1);

    resolved_option_refinement_candidate_pricing_cpu_count(
        total_pricing_cpu_count,
        candidate_parallelism,
    )
}

fn context_with_option_pricing_cpu_count(
    context: &JointRefinementContext,
    pricing_cpu_count: usize,
) -> JointRefinementContext {
    let mut updated_context = context.clone();
    if let Some(option_pricing_context) = updated_context.option_pricing_context.as_mut() {
        option_pricing_context.cpu_count = pricing_cpu_count.max(1);
    }

    updated_context
}

fn refine_search_tier_candidates(
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    dynamics_candidates: &[JointRefinementRestartOutcome],
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    mut resume_store: Option<&mut JointRefinementResumeStore>,
) -> Vec<SearchTierCandidate> {
    let total_units = dynamics_candidates.len();

    let shared_progress_state = Arc::new(OptionRefinementSharedProgressState::default());

    let mut candidate_slots = vec![None; total_units];
    let mut unresolved_candidates = Vec::new();

    emit_option_refinement_progress(OptionRefinementProgressEmissionInput::new(
        progress_sender,
        JointRefinementStageProgressPhase::OptionSearchTierRefinement,
        0,
        total_units,
    ));

    for (source_rank, dynamics_candidate) in dynamics_candidates.iter().enumerate() {
        if let Some(resume_store) = resume_store.as_deref_mut()
            && let Some(mut resumed_candidate) =
                resume_store.search_tier_candidate(dynamics_candidate.restart_id.as_str())
        {
            resumed_candidate.source_rank = source_rank;

            let progress_snapshot = shared_progress_state.record_resumed_candidate(
                dynamics_candidate.restart_id.as_str(),
                resumed_candidate.search_tier_score,
            );

            candidate_slots[source_rank] = Some(resumed_candidate.clone());

            emit_option_refinement_progress(OptionRefinementProgressEmissionInput {
                current_restart_id: Some(dynamics_candidate.restart_id.as_str()),
                current_item_label: Some("resumed"),
                current_weighted_objective: Some(resumed_candidate.search_tier_score),
                best_restart_id: progress_snapshot.best_restart_id.as_deref(),
                best_weighted_objective: progress_snapshot.best_weighted_objective,
                active_restart_count: Some(progress_snapshot.active_candidate_count),
                ..OptionRefinementProgressEmissionInput::new(
                    progress_sender,
                    JointRefinementStageProgressPhase::OptionSearchTierRefinement,
                    progress_snapshot.completed_units,
                    total_units,
                )
            });

            continue;
        }

        unresolved_candidates.push((source_rank, dynamics_candidate));
    }

    let candidate_parallelism =
        option_refinement_candidate_parallelism(context, unresolved_candidates.len());

    let pricing_cpu_count =
        option_refinement_candidate_pricing_cpu_count(context, candidate_parallelism);

    let unresolved_results = with_joint_refinement_thread_pool(context, || {
        if unresolved_candidates.len() > 1 && candidate_parallelism > 1 {
            unresolved_candidates
                .par_iter()
                .map(|(source_rank, dynamics_candidate)| {
                    refine_parallel_search_tier_candidate(
                        ParallelSearchTierCandidateRefinementInput {
                            context,
                            solver_settings,
                            source_rank: *source_rank,
                            dynamics_candidate,
                            progress_sender,
                            total_units,
                            shared_progress_state: Arc::clone(&shared_progress_state),
                            pricing_cpu_count,
                        },
                    )
                })
                .collect::<Vec<_>>()
        } else {
            unresolved_candidates
                .iter()
                .map(|(source_rank, dynamics_candidate)| {
                    refine_parallel_search_tier_candidate(
                        ParallelSearchTierCandidateRefinementInput {
                            context,
                            solver_settings,
                            source_rank: *source_rank,
                            dynamics_candidate,
                            progress_sender,
                            total_units,
                            shared_progress_state: Arc::clone(&shared_progress_state),
                            pricing_cpu_count,
                        },
                    )
                })
                .collect::<Vec<_>>()
        }
    });

    for (source_rank, candidate) in unresolved_results {
        if let Some(candidate) = candidate {
            if let Some(resume_store) = resume_store.as_deref_mut() {
                resume_store.record_search_tier_candidate(&candidate);
            }

            candidate_slots[source_rank] = Some(candidate);
        }
    }

    candidate_slots.into_iter().flatten().collect()
}

fn refine_single_search_tier_candidate(
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    dynamics_candidate: &JointRefinementRestartOutcome,
    progress_context: Option<&OptionRefinementProgressContext<'_>>,
    pricing_cpu_count: usize,
) -> Result<SearchTierCandidate, String> {
    let refinement_context =
        pricing_block_refinement_context(context, dynamics_candidate, pricing_cpu_count);

    let source_dynamics_parameter_bits =
        ordered_parameter_bits(dynamics_candidate.parameter_vector.as_slice());

    let mut theta_warm_start_state = SpectralThetaWarmStartState::new();

    let initial_candidate = scored_pricing_vector(
        &refinement_context,
        dynamics_candidate,
        dynamics_candidate.parameter_vector.as_slice(),
        source_dynamics_parameter_bits.as_slice(),
        JointRefinementOptionSurfaceTierKind::Search,
        Some(&mut theta_warm_start_state),
        progress_context,
    )?;

    let optimized_candidate = optimize_pricing_block(
        &refinement_context,
        solver_settings,
        dynamics_candidate,
        source_dynamics_parameter_bits.as_slice(),
        &mut theta_warm_start_state,
        progress_context,
    )
    .ok();

    Ok(match optimized_candidate {
        Some(candidate) if candidate.search_tier_score < initial_candidate.search_tier_score => {
            candidate
        }
        _ => initial_candidate,
    })
}

struct ParallelSearchTierCandidateRefinementInput<'a> {
    context: &'a JointRefinementContext,
    solver_settings: &'a CalibrationOptimizerSettings,
    source_rank: usize,
    dynamics_candidate: &'a JointRefinementRestartOutcome,
    progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    total_units: usize,
    shared_progress_state: Arc<OptionRefinementSharedProgressState>,
    pricing_cpu_count: usize,
}

fn refine_parallel_search_tier_candidate(
    input: ParallelSearchTierCandidateRefinementInput<'_>,
) -> (usize, Option<SearchTierCandidate>) {
    let ParallelSearchTierCandidateRefinementInput {
        context,
        solver_settings,
        source_rank,
        dynamics_candidate,
        progress_sender,
        total_units,
        shared_progress_state,
        pricing_cpu_count,
    } = input;

    let started_snapshot = shared_progress_state.record_candidate_started();

    emit_option_refinement_progress(OptionRefinementProgressEmissionInput {
        current_restart_id: Some(dynamics_candidate.restart_id.as_str()),
        best_restart_id: started_snapshot.best_restart_id.as_deref(),
        best_weighted_objective: started_snapshot.best_weighted_objective,
        active_restart_count: Some(started_snapshot.active_candidate_count),
        ..OptionRefinementProgressEmissionInput::new(
            progress_sender,
            JointRefinementStageProgressPhase::OptionSearchTierRefinement,
            started_snapshot.completed_units,
            total_units,
        )
    });

    let progress_context = OptionRefinementProgressContext {
        progress_sender,
        phase: JointRefinementStageProgressPhase::OptionSearchTierRefinement,
        total_units,
        current_restart_id: dynamics_candidate.restart_id.as_str(),
        shared_progress_state: Arc::clone(&shared_progress_state),
        current_evaluation_count: Cell::new(0),
        total_evaluation_budget: option_refinement_candidate_total_evaluation_budget(
            context.active_parameter_layout.nig_block.len(),
        ),
    };

    let refined_candidate = refine_single_search_tier_candidate(
        context,
        solver_settings,
        dynamics_candidate,
        Some(&progress_context),
        pricing_cpu_count,
    )
    .ok()
    .map(|candidate| SearchTierCandidate {
        source_rank,
        source_dynamics_parameter_bits: candidate.source_dynamics_parameter_bits,
        search_tier_score: candidate.search_tier_score,
        search_tier_pricing_artifact: candidate.search_tier_pricing_artifact,
        restart_outcome: candidate.restart_outcome,
    });

    let finished_snapshot = shared_progress_state.record_candidate_completed(
        dynamics_candidate.restart_id.as_str(),
        refined_candidate
            .as_ref()
            .map(|candidate| candidate.search_tier_score),
    );

    emit_option_refinement_progress(OptionRefinementProgressEmissionInput {
        current_restart_id: Some(dynamics_candidate.restart_id.as_str()),
        current_weighted_objective: refined_candidate
            .as_ref()
            .map(|candidate| candidate.search_tier_score),
        best_restart_id: finished_snapshot.best_restart_id.as_deref(),
        best_weighted_objective: finished_snapshot.best_weighted_objective,
        active_restart_count: Some(finished_snapshot.active_candidate_count),
        current_evaluation_count: Some(progress_context.current_evaluation_count()),
        total_evaluation_budget: Some(progress_context.total_evaluation_budget()),
        evaluation_progress_percent: Some(100),
        ..OptionRefinementProgressEmissionInput::new(
            progress_sender,
            JointRefinementStageProgressPhase::OptionSearchTierRefinement,
            finished_snapshot.completed_units,
            total_units,
        )
    });

    (source_rank, refined_candidate)
}

fn optimize_pricing_block(
    refinement_context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    dynamics_candidate: &JointRefinementRestartOutcome,
    source_dynamics_parameter_bits: &[u64],
    theta_warm_start_state: &mut SpectralThetaWarmStartState,
    progress_context: Option<&OptionRefinementProgressContext<'_>>,
) -> Result<SearchTierCandidate, String> {
    let block_indices = refinement_context
        .active_parameter_layout
        .nig_block
        .as_slice();
    if block_indices.is_empty() {
        return scored_pricing_vector(
            refinement_context,
            dynamics_candidate,
            dynamics_candidate.parameter_vector.as_slice(),
            source_dynamics_parameter_bits,
            JointRefinementOptionSurfaceTierKind::Search,
            Some(theta_warm_start_state),
            progress_context,
        );
    }

    let initial_point = block_indices
        .iter()
        .map(|parameter_index| dynamics_candidate.parameter_vector[*parameter_index])
        .collect::<Vec<_>>();

    let lower_bounds = block_indices
        .iter()
        .map(|parameter_index| refinement_context.lower_bounds[*parameter_index])
        .collect::<Vec<_>>();

    let upper_bounds = block_indices
        .iter()
        .map(|parameter_index| refinement_context.upper_bounds[*parameter_index])
        .collect::<Vec<_>>();

    let search_coordinate_scaling =
        derive_pricing_block_search_coordinate_scaling(refinement_context, dynamics_candidate);

    let scaled_initial_point = scale_pricing_block_coordinates(
        initial_point.as_slice(),
        search_coordinate_scaling.as_slice(),
    );

    let scaled_lower_bounds = scale_pricing_block_coordinates(
        lower_bounds.as_slice(),
        search_coordinate_scaling.as_slice(),
    );

    let scaled_upper_bounds = scale_pricing_block_coordinates(
        upper_bounds.as_slice(),
        search_coordinate_scaling.as_slice(),
    );

    let scaled_trust_region_settings = scale_trust_region_settings_for_pricing_block(
        option_pricing_block_trust_region_settings(solver_settings, block_indices.len()),
        search_coordinate_scaling.as_slice(),
    );

    let mut evaluate_block = |scaled_block_vector: &[f64]| -> Option<f64> {
        let block_vector = unscale_pricing_block_coordinates(
            scaled_block_vector,
            search_coordinate_scaling.as_slice(),
        );

        let pricing_vector = substitute_pricing_block(
            dynamics_candidate.parameter_vector.as_slice(),
            block_indices,
            block_vector.as_slice(),
        );

        scored_pricing_vector(
            refinement_context,
            dynamics_candidate,
            pricing_vector.as_slice(),
            source_dynamics_parameter_bits,
            JointRefinementOptionSurfaceTierKind::Search,
            Some(theta_warm_start_state),
            progress_context,
        )
        .ok()
        .map(|candidate| candidate.search_tier_score)
    };

    let optimizer_result = optimize_bounded_trust_region_quadratic(
        scaled_initial_point.as_slice(),
        scaled_lower_bounds.as_slice(),
        scaled_upper_bounds.as_slice(),
        scaled_trust_region_settings,
        &mut evaluate_block,
    )
    .ok_or_else(|| "pricing block optimizer found no valid option-pricing step".to_string())?;

    let optimized_block_vector = unscale_pricing_block_coordinates(
        optimizer_result.best_point.as_slice(),
        search_coordinate_scaling.as_slice(),
    );

    let pricing_vector = substitute_pricing_block(
        dynamics_candidate.parameter_vector.as_slice(),
        block_indices,
        optimized_block_vector.as_slice(),
    );

    scored_pricing_vector(
        refinement_context,
        dynamics_candidate,
        pricing_vector.as_slice(),
        source_dynamics_parameter_bits,
        JointRefinementOptionSurfaceTierKind::Search,
        Some(theta_warm_start_state),
        progress_context,
    )
}

fn derive_pricing_block_search_coordinate_scaling(
    refinement_context: &JointRefinementContext,
    dynamics_candidate: &JointRefinementRestartOutcome,
) -> Vec<f64> {
    let nig_block = refinement_context
        .active_parameter_layout
        .nig_block
        .as_slice();
    if nig_block.len() != 4 {
        return vec![1.0; nig_block.len()];
    }

    let Some(modeled_open_event_rate_per_hour) = stationary_open_event_rate_per_hour(
        &dynamics_candidate
            .final_evaluation
            .fitted_parameters
            .hawkes_fitted_parameters,
    ) else {
        return vec![1.0; nig_block.len()];
    };

    let calibration_sample = build_continuous_calibration_sample_from_aggregated_intervals(
        refinement_context
            .observed_data
            .observed_15m_intervals
            .as_slice(),
        modeled_open_event_rate_per_hour,
        refinement_context
            .observed_data
            .observed_open_event_rate_per_hour,
        refinement_context.observed_data.fallback_sell_share_proxy,
    );

    let search_point = NigShapeSearchPoint {
        log_alpha: dynamics_candidate.parameter_vector[nig_block[0]],
        buy_skew_coordinate: dynamics_candidate.parameter_vector[nig_block[1]],
        sell_relative_coordinate: dynamics_candidate.parameter_vector[nig_block[2]],
        log_delta_offset: dynamics_candidate.parameter_vector[nig_block[3]],
    };

    let Some(mapping) =
        exact_configuration_and_gradient_from_shape_search_point_for_calibration_sample(
            &calibration_sample,
            search_point,
        )
    else {
        return vec![1.0; nig_block.len()];
    };

    let gradient = mapping.gradient;
    [
        gradient.alpha_gradient[0]
            .hypot(gradient.beta_0_gradient[0])
            .hypot(gradient.delta_gradient[0])
            .hypot(gradient.delta_beta_gradient[0]),
        gradient.alpha_gradient[1]
            .hypot(gradient.beta_0_gradient[1])
            .hypot(gradient.delta_gradient[1])
            .hypot(gradient.delta_beta_gradient[1]),
        gradient.alpha_gradient[2]
            .hypot(gradient.beta_0_gradient[2])
            .hypot(gradient.delta_gradient[2])
            .hypot(gradient.delta_beta_gradient[2]),
        gradient.alpha_gradient[3]
            .hypot(gradient.beta_0_gradient[3])
            .hypot(gradient.delta_gradient[3])
            .hypot(gradient.delta_beta_gradient[3]),
    ]
    .into_iter()
    .map(|scale: f64| {
        if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        }
    })
    .collect()
}

fn scale_pricing_block_coordinates(point: &[f64], scaling: &[f64]) -> Vec<f64> {
    point
        .iter()
        .zip(scaling.iter())
        .map(|(coordinate, scale)| coordinate * scale)
        .collect()
}

fn unscale_pricing_block_coordinates(scaled_point: &[f64], scaling: &[f64]) -> Vec<f64> {
    scaled_point
        .iter()
        .zip(scaling.iter())
        .map(|(coordinate, scale)| coordinate / scale)
        .collect()
}

fn scale_trust_region_settings_for_pricing_block(
    settings: BoundedTrustRegionOptimizerSettings,
    scaling: &[f64],
) -> BoundedTrustRegionOptimizerSettings {
    let minimum_positive_scale = scaling
        .iter()
        .copied()
        .filter(|scale| scale.is_finite() && *scale > 0.0)
        .min_by(f64::total_cmp)
        .unwrap_or(1.0);

    BoundedTrustRegionOptimizerSettings {
        absolute_tolerance: settings.absolute_tolerance * minimum_positive_scale,
        minimum_trust_region_radius: settings.minimum_trust_region_radius * minimum_positive_scale,
        ..settings
    }
}

fn evaluate_selection_tier_candidates(
    context: &JointRefinementContext,
    search_candidates: Vec<SearchTierCandidate>,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    mut resume_store: Option<&mut JointRefinementResumeStore>,
) -> Vec<OptionRefinedCandidate> {
    let total_units = search_candidates.len();

    let shared_progress_state = Arc::new(OptionRefinementSharedProgressState::default());

    let mut candidate_slots = vec![None; total_units];
    let mut unresolved_candidates = Vec::new();

    emit_option_refinement_progress(OptionRefinementProgressEmissionInput::new(
        progress_sender,
        JointRefinementStageProgressPhase::OptionSelectionTierEvaluation,
        0,
        total_units,
    ));

    for (candidate_position, search_candidate) in search_candidates.into_iter().enumerate() {
        let restart_id = search_candidate.restart_outcome.restart_id.clone();
        if let Some(resume_store) = resume_store.as_deref_mut()
            && let Some(mut resumed_candidate) =
                resume_store.selection_tier_candidate(restart_id.as_str())
        {
            resumed_candidate.source_rank = search_candidate.source_rank;

            let progress_snapshot = shared_progress_state.record_resumed_candidate(
                restart_id.as_str(),
                resumed_candidate.selection_tier_score,
            );

            candidate_slots[candidate_position] = Some(resumed_candidate.clone());

            emit_option_refinement_progress(OptionRefinementProgressEmissionInput {
                current_restart_id: Some(restart_id.as_str()),
                current_item_label: Some("resumed"),
                current_weighted_objective: Some(resumed_candidate.selection_tier_score),
                best_restart_id: progress_snapshot.best_restart_id.as_deref(),
                best_weighted_objective: progress_snapshot.best_weighted_objective,
                active_restart_count: Some(progress_snapshot.active_candidate_count),
                ..OptionRefinementProgressEmissionInput::new(
                    progress_sender,
                    JointRefinementStageProgressPhase::OptionSelectionTierEvaluation,
                    progress_snapshot.completed_units,
                    total_units,
                )
            });

            continue;
        }

        unresolved_candidates.push((candidate_position, search_candidate));
    }

    let candidate_parallelism =
        option_refinement_candidate_parallelism(context, unresolved_candidates.len());

    let pricing_cpu_count =
        option_refinement_candidate_pricing_cpu_count(context, candidate_parallelism);

    let unresolved_results = with_joint_refinement_thread_pool(context, || {
        if unresolved_candidates.len() > 1 && candidate_parallelism > 1 {
            unresolved_candidates
                .par_iter()
                .map(|(candidate_position, search_candidate)| {
                    evaluate_parallel_selection_tier_candidate(
                        context,
                        *candidate_position,
                        search_candidate,
                        progress_sender,
                        total_units,
                        Arc::clone(&shared_progress_state),
                        pricing_cpu_count,
                    )
                })
                .collect::<Vec<_>>()
        } else {
            unresolved_candidates
                .iter()
                .map(|(candidate_position, search_candidate)| {
                    evaluate_parallel_selection_tier_candidate(
                        context,
                        *candidate_position,
                        search_candidate,
                        progress_sender,
                        total_units,
                        Arc::clone(&shared_progress_state),
                        pricing_cpu_count,
                    )
                })
                .collect::<Vec<_>>()
        }
    });

    for (candidate_position, candidate) in unresolved_results {
        if let Some(candidate) = candidate {
            if let Some(resume_store) = resume_store.as_deref_mut() {
                resume_store.record_selection_tier_candidate(&candidate);
            }
            candidate_slots[candidate_position] = Some(candidate);
        }
    }

    candidate_slots.into_iter().flatten().collect()
}

fn scored_pricing_vector(
    refinement_context: &JointRefinementContext,
    dynamics_candidate: &JointRefinementRestartOutcome,
    pricing_vector: &[f64],
    source_dynamics_parameter_bits: &[u64],
    tier_kind: JointRefinementOptionSurfaceTierKind,
    theta_warm_start_state: Option<&mut SpectralThetaWarmStartState>,
    progress_context: Option<&OptionRefinementProgressContext<'_>>,
) -> Result<SearchTierCandidate, String> {
    if let Some(progress_context) = progress_context {
        progress_context.record_evaluation_started();
    }

    let shared_progress_snapshot =
        progress_context.map(|context| context.shared_progress_snapshot());

    let fitted_parameters =
        parameter_vector_to_fitted_parameters(pricing_vector, refinement_context)
            .ok_or_else(|| "pricing block parameter conversion failed".to_string())?;

    let pricing_parameter_bits = pricing_block_parameter_bits(pricing_vector, refinement_context);

    let pricing_artifact = resolve_option_surface_tier_pricing_artifact(
        OptionSurfaceTierPricingArtifactResolutionInput {
            context: refinement_context,
            fitted_parameters: &fitted_parameters,
            tier_kind,
            source_dynamics_parameter_bits,
            pricing_parameter_bits: pricing_parameter_bits.as_slice(),
            include_node_price_sensitivities: false,
            theta_warm_start_state,
            progress: OptionPricingProgressInput {
                progress_sender: progress_context.and_then(|context| context.progress_sender),
                progress_phase: progress_context.map(|context| context.phase),
                completed_units: shared_progress_snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.completed_units),
                total_units: progress_context.map(|context| context.total_units),
                current_restart_id: progress_context.map(|context| context.current_restart_id),
                best_restart_id: shared_progress_snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.best_restart_id.as_deref()),
                best_weighted_objective: shared_progress_snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.best_weighted_objective),
                active_restart_count: shared_progress_snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.active_candidate_count),
                current_evaluation_count: progress_context
                    .map(|context| context.current_evaluation_count()),
                total_evaluation_budget: progress_context
                    .map(|context| context.total_evaluation_budget()),
                evaluation_progress_percent: progress_context
                    .map(|context| context.running_evaluation_progress_percent()),
            },
        },
    )?;

    let search_tier_score = pricing_artifact
        .score
        .root_mean_square_spread_normalized_residual;

    Ok(SearchTierCandidate {
        source_rank: 0,
        source_dynamics_parameter_bits: source_dynamics_parameter_bits.to_vec(),
        search_tier_score,
        search_tier_pricing_artifact: pricing_artifact,
        restart_outcome: restart_outcome_with_pricing_block(
            dynamics_candidate,
            pricing_vector,
            fitted_parameters,
        ),
    })
}

fn evaluate_parallel_selection_tier_candidate(
    context: &JointRefinementContext,
    candidate_position: usize,
    search_candidate: &SearchTierCandidate,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    total_units: usize,
    shared_progress_state: Arc<OptionRefinementSharedProgressState>,
    pricing_cpu_count: usize,
) -> (usize, Option<OptionRefinedCandidate>) {
    let restart_id = search_candidate.restart_outcome.restart_id.as_str();

    let started_snapshot = shared_progress_state.record_candidate_started();

    emit_option_refinement_progress(OptionRefinementProgressEmissionInput {
        current_restart_id: Some(restart_id),
        best_restart_id: started_snapshot.best_restart_id.as_deref(),
        best_weighted_objective: started_snapshot.best_weighted_objective,
        active_restart_count: Some(started_snapshot.active_candidate_count),
        ..OptionRefinementProgressEmissionInput::new(
            progress_sender,
            JointRefinementStageProgressPhase::OptionSelectionTierEvaluation,
            started_snapshot.completed_units,
            total_units,
        )
    });

    let selection_context = context_with_option_pricing_cpu_count(context, pricing_cpu_count);

    let pricing_parameter_bits = pricing_block_parameter_bits(
        search_candidate.restart_outcome.parameter_vector.as_slice(),
        &selection_context,
    );

    let best_snapshot = shared_progress_state.snapshot();

    let selection_artifact = resolve_option_surface_tier_pricing_artifact(
        OptionSurfaceTierPricingArtifactResolutionInput {
            context: &selection_context,
            fitted_parameters: &search_candidate
                .restart_outcome
                .final_evaluation
                .fitted_parameters,
            tier_kind: JointRefinementOptionSurfaceTierKind::Selection,
            source_dynamics_parameter_bits: search_candidate
                .source_dynamics_parameter_bits
                .as_slice(),
            pricing_parameter_bits: pricing_parameter_bits.as_slice(),
            include_node_price_sensitivities: false,
            theta_warm_start_state: None,
            progress: OptionPricingProgressInput {
                progress_sender,
                progress_phase: Some(
                    JointRefinementStageProgressPhase::OptionSelectionTierEvaluation,
                ),
                completed_units: Some(best_snapshot.completed_units),
                total_units: Some(total_units),
                current_restart_id: Some(restart_id),
                best_restart_id: best_snapshot.best_restart_id.as_deref(),
                best_weighted_objective: best_snapshot.best_weighted_objective,
                active_restart_count: Some(best_snapshot.active_candidate_count),
                current_evaluation_count: None,
                total_evaluation_budget: None,
                evaluation_progress_percent: None,
            },
        },
    )
    .ok();

    let refined_candidate = selection_artifact.map(|selection_tier_pricing_artifact| {
        let selection_tier_score = selection_tier_pricing_artifact
            .score
            .root_mean_square_spread_normalized_residual;
        OptionRefinedCandidate {
            source_rank: search_candidate.source_rank,
            search_tier_score: search_candidate.search_tier_score,
            selection_tier_score,
            search_tier_pricing_artifact: search_candidate.search_tier_pricing_artifact.clone(),
            selection_tier_pricing_artifact,
            restart_outcome: search_candidate.restart_outcome.clone(),
        }
    });

    let finished_snapshot = shared_progress_state.record_candidate_completed(
        restart_id,
        refined_candidate
            .as_ref()
            .map(|candidate| candidate.selection_tier_score),
    );

    emit_option_refinement_progress(OptionRefinementProgressEmissionInput {
        current_restart_id: Some(restart_id),
        current_weighted_objective: refined_candidate
            .as_ref()
            .map(|candidate| candidate.selection_tier_score),
        best_restart_id: finished_snapshot.best_restart_id.as_deref(),
        best_weighted_objective: finished_snapshot.best_weighted_objective,
        active_restart_count: Some(finished_snapshot.active_candidate_count),
        current_evaluation_count: Some(1),
        total_evaluation_budget: Some(1),
        evaluation_progress_percent: Some(100),
        ..OptionRefinementProgressEmissionInput::new(
            progress_sender,
            JointRefinementStageProgressPhase::OptionSelectionTierEvaluation,
            finished_snapshot.completed_units,
            total_units,
        )
    });

    (candidate_position, refined_candidate)
}

fn best_selection_tier_candidate(
    mut candidates: Vec<OptionRefinedCandidate>,
) -> Option<OptionRefinedCandidate> {
    candidates.sort_by(|left, right| {
        left.selection_tier_score
            .total_cmp(&right.selection_tier_score)
            .then_with(|| left.search_tier_score.total_cmp(&right.search_tier_score))
            .then_with(|| left.source_rank.cmp(&right.source_rank))
            .then_with(|| {
                left.restart_outcome
                    .restart_id
                    .cmp(&right.restart_outcome.restart_id)
            })
    });

    candidates.into_iter().next()
}

fn pricing_block_refinement_context(
    context: &JointRefinementContext,
    dynamics_candidate: &JointRefinementRestartOutcome,
    pricing_cpu_count: usize,
) -> JointRefinementContext {
    let mut refinement_context = context_with_option_pricing_cpu_count(context, pricing_cpu_count);

    refinement_context.baseline_hawkes_fitted_parameters = dynamics_candidate
        .final_evaluation
        .fitted_parameters
        .hawkes_fitted_parameters
        .clone();

    refinement_context.baseline_return_components_fitted_parameters = dynamics_candidate
        .final_evaluation
        .fitted_parameters
        .return_components_fitted_parameters
        .clone();

    refinement_context.baseline_mean_reversion_fitted_parameters = dynamics_candidate
        .final_evaluation
        .fitted_parameters
        .return_components_fitted_parameters
        .mean_reversion_fitted_parameters
        .clone();

    refinement_context.option_pricing_refinement_enabled = true;

    refinement_context
}

fn restart_outcome_with_pricing_block(
    dynamics_candidate: &JointRefinementRestartOutcome,
    pricing_vector: &[f64],
    fitted_parameters: janiform_calibration_artifacts::JointRefinementStageFittedParameters,
) -> JointRefinementRestartOutcome {
    let mut local_evaluation = dynamics_candidate.local_evaluation.clone();
    local_evaluation.fitted_parameters = fitted_parameters.clone();

    let mut final_evaluation = dynamics_candidate.final_evaluation.clone();
    final_evaluation.fitted_parameters = fitted_parameters;

    JointRefinementRestartOutcome {
        restart_id: dynamics_candidate.restart_id.clone(),
        parameter_vector: pricing_vector.to_vec(),
        local_evaluation,
        final_evaluation,
        local_evaluation_count: dynamics_candidate.local_evaluation_count,
        stop_reason: "option_pricing_block_refined".to_string(),
    }
}

pub(crate) fn option_pricing_block_evaluation_budget(active_nig_block_dimension: usize) -> usize {
    active_nig_block_dimension
        .saturating_mul(6)
        .saturating_add(1)
}

fn option_refinement_candidate_total_evaluation_budget(active_nig_block_dimension: usize) -> usize {
    if active_nig_block_dimension == 0 {
        return 1;
    }

    option_pricing_block_evaluation_budget(active_nig_block_dimension).saturating_add(2)
}

fn option_pricing_block_trust_region_settings(
    solver_settings: &CalibrationOptimizerSettings,
    active_nig_block_dimension: usize,
) -> BoundedTrustRegionOptimizerSettings {
    BoundedTrustRegionOptimizerSettings {
        absolute_tolerance: solver_settings.absolute_tolerance,
        relative_tolerance: solver_settings.relative_tolerance,
        minimum_trust_region_radius: solver_settings.absolute_tolerance,
        initial_trust_region_radius_fraction: 0.25,
        interpolation_point_target: active_nig_block_dimension
            .saturating_mul(2)
            .saturating_add(1),
        evaluation_budget: option_pricing_block_evaluation_budget(active_nig_block_dimension),
    }
}

fn pricing_block_parameter_bits(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
) -> Vec<u64> {
    context
        .active_parameter_layout
        .nig_block
        .iter()
        .map(|parameter_index| parameter_vector[*parameter_index].to_bits())
        .collect()
}

fn substitute_pricing_block(
    current_vector: &[f64],
    block_indices: &[usize],
    block_vector: &[f64],
) -> Vec<f64> {
    let mut updated = current_vector.to_vec();

    for (block_position, parameter_index) in block_indices.iter().enumerate() {
        updated[*parameter_index] = block_vector[block_position];
    }

    updated
}

struct OptionRefinementProgressEmissionInput<'a> {
    progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    phase: JointRefinementStageProgressPhase,
    completed_units: usize,
    total_units: usize,
    current_restart_id: Option<&'a str>,
    current_item_label: Option<&'a str>,
    current_weighted_objective: Option<f64>,
    best_restart_id: Option<&'a str>,
    best_weighted_objective: Option<f64>,
    active_restart_count: Option<usize>,
    current_evaluation_count: Option<usize>,
    total_evaluation_budget: Option<usize>,
    evaluation_progress_percent: Option<usize>,
}

impl<'a> OptionRefinementProgressEmissionInput<'a> {
    fn new(
        progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
        phase: JointRefinementStageProgressPhase,
        completed_units: usize,
        total_units: usize,
    ) -> Self {
        Self {
            progress_sender,
            phase,
            completed_units,
            total_units,
            current_restart_id: None,
            current_item_label: None,
            current_weighted_objective: None,
            best_restart_id: None,
            best_weighted_objective: None,
            active_restart_count: None,
            current_evaluation_count: None,
            total_evaluation_budget: None,
            evaluation_progress_percent: None,
        }
    }
}

fn emit_option_refinement_progress(input: OptionRefinementProgressEmissionInput<'_>) {
    let OptionRefinementProgressEmissionInput {
        progress_sender,
        phase,
        completed_units,
        total_units,
        current_restart_id,
        current_item_label,
        current_weighted_objective,
        best_restart_id,
        best_weighted_objective,
        active_restart_count,
        current_evaluation_count,
        total_evaluation_budget,
        evaluation_progress_percent,
    } = input;

    if let Some(progress_sender) = progress_sender {
        let _ = progress_sender.send(JointRefinementStageProgressUpdate {
            current_restart_id: current_restart_id.map(str::to_string),
            current_item_label: current_item_label.map(str::to_string),
            active_restart_count,
            current_evaluation_count,
            total_evaluation_budget,
            evaluation_progress_percent,
            current_weighted_objective,
            best_restart_id: best_restart_id.map(str::to_string),
            best_weighted_objective,
            ..JointRefinementStageProgressUpdate::new(phase, completed_units, total_units)
        });
    }
}

#[cfg(test)]
mod tests {
    use janiform_calibration_artifacts::CalibrationOptimizerSettings;

    use super::{
        JOINT_REFINEMENT_DEFAULT_OPTION_REFINEMENT_CANDIDATE_COUNT,
        desired_joint_refinement_option_refinement_candidate_count,
        option_pricing_block_trust_region_settings,
        option_refinement_candidate_total_evaluation_budget,
        option_refinement_selection_score_ceiling,
        resolved_option_refinement_candidate_parallelism,
        resolved_option_refinement_candidate_pricing_cpu_count,
    };

    #[test]
    fn option_refinement_candidate_count_defaults_to_five() {
        assert_eq!(
            desired_joint_refinement_option_refinement_candidate_count(None),
            JOINT_REFINEMENT_DEFAULT_OPTION_REFINEMENT_CANDIDATE_COUNT,
        );
    }

    #[test]
    fn option_refinement_candidate_count_honors_positive_override() {
        assert_eq!(
            desired_joint_refinement_option_refinement_candidate_count(Some(7)),
            7,
        );
    }

    #[test]
    fn pricing_block_budget_uses_low_dimension_trust_region_model() {
        let solver_settings =
            CalibrationOptimizerSettings::try_new("joint-test", 256, 4, 1.0e-6, 2.0e-6, 10.0)
                .expect("solver settings should validate");

        let trust_region_settings = option_pricing_block_trust_region_settings(&solver_settings, 4);

        assert_eq!(trust_region_settings.interpolation_point_target, 9);
        assert_eq!(trust_region_settings.evaluation_budget, 25);
        assert_eq!(
            trust_region_settings.initial_trust_region_radius_fraction,
            0.25
        );
        assert_eq!(trust_region_settings.absolute_tolerance, 1.0e-6);
        assert_eq!(trust_region_settings.relative_tolerance, 2.0e-6);
    }

    #[test]
    fn option_refinement_candidate_budget_counts_initial_and_final_pricing_runs() {
        assert_eq!(option_refinement_candidate_total_evaluation_budget(0), 1);
        assert_eq!(option_refinement_candidate_total_evaluation_budget(4), 27);
    }

    #[test]
    fn option_refinement_parallelism_is_bounded_by_candidates_and_available_capacity() {
        assert_eq!(
            resolved_option_refinement_candidate_parallelism(24, 24, 5),
            5
        );
        assert_eq!(
            resolved_option_refinement_candidate_parallelism(2, 24, 5),
            2
        );
        assert_eq!(
            resolved_option_refinement_candidate_parallelism(24, 3, 5),
            3
        );
    }

    #[test]
    fn option_refinement_candidate_pricing_cpu_count_partitions_total_cpu_budget() {
        assert_eq!(
            resolved_option_refinement_candidate_pricing_cpu_count(24, 5),
            4
        );
        assert_eq!(
            resolved_option_refinement_candidate_pricing_cpu_count(3, 5),
            1
        );
    }

    #[test]
    fn option_refinement_selection_score_ceiling_uses_sample_count_derived_admissibility_band() {
        let best_search_tier_score = 2.0;
        let effective_sample_count = 16;

        assert_eq!(
            option_refinement_selection_score_ceiling(
                best_search_tier_score,
                effective_sample_count,
            ),
            2.5
        );
    }
}
