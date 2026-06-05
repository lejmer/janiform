//! Continuous NIG basin optimization policy:
//! use bounded Powell over the four validated NIG search coordinates, split the
//! configured stage iteration cap across retained basin seeds, and score local
//! moves on the local likelihood tier before final full-likelihood evaluation.

use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

use super::super::contracts::{
    ReturnComponentsStageProgressPhase, ReturnComponentsStageProgressUpdate,
};
use super::super::progress::emit_return_components_evaluation_progress;
use super::optimization::evaluate_cached_search_point;
use super::types::{
    ContinuousLikelihoodFidelityTier, ContinuousReturnOptimizerContext,
    ContinuousReturnOptimizerDiagnostic, NigShapeSearchBounds, OptimizedNigSearchCandidate,
    SeededNigCandidate,
};
use crate::stages::optimization::{PowellOptimizerSettings, optimize_bounded_powell};

pub(super) fn optimize_seed_candidates(
    optimizer_context: &ContinuousReturnOptimizerContext,
    seed_candidates: &[SeededNigCandidate],
    search_bounds: NigShapeSearchBounds,
    solver_settings: &CalibrationOptimizerSettings,
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
) -> Vec<OptimizedNigSearchCandidate> {
    let completed_seed_candidate_count = std::sync::atomic::AtomicUsize::new(0);
    let total_seed_candidate_count = seed_candidates.len();
    let powell_settings =
        continuous_nig_basin_powell_settings(solver_settings, total_seed_candidate_count);

    seed_candidates
        .par_iter()
        .filter_map(|seed_candidate| {
            let optimized_candidate = optimize_single_seed_candidate(
                optimizer_context,
                seed_candidate,
                search_bounds,
                powell_settings,
            );

            let completed_count = completed_seed_candidate_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                + 1;
            let seed_label = format!("basin_seed_{completed_count:03}");

            emit_return_components_evaluation_progress(
                progress_sender,
                ReturnComponentsStageProgressPhase::ContinuousNigBasinOptimization,
                completed_count,
                total_seed_candidate_count,
                Some(seed_label.as_str()),
                completed_count,
                total_seed_candidate_count,
            );

            optimized_candidate
        })
        .collect()
}

fn optimize_single_seed_candidate(
    optimizer_context: &ContinuousReturnOptimizerContext,
    seed_candidate: &SeededNigCandidate,
    search_bounds: NigShapeSearchBounds,
    powell_settings: PowellOptimizerSettings,
) -> Option<OptimizedNigSearchCandidate> {
    let initial_point = seed_candidate.scored_candidate.search_point.to_vector();

    let mut evaluate_local_objective = |point: &[f64]| -> Option<f64> {
        let search_point = super::types::NigShapeSearchPoint::from_slice(point)?;
        evaluate_cached_search_point(
            optimizer_context,
            search_point,
            ContinuousLikelihoodFidelityTier::Local,
        )
        .map(|evaluation| evaluation.objective.shifted_objective_score)
    };

    let optimizer_result = optimize_bounded_powell(
        initial_point.as_slice(),
        search_bounds.lower_bounds.as_slice(),
        search_bounds.upper_bounds.as_slice(),
        powell_settings,
        &mut evaluate_local_objective,
    );

    let (final_search_point, local_objective_score, local_evaluation_count, stop_reason) =
        match optimizer_result {
            Some(result) => (
                super::types::NigShapeSearchPoint::from_slice(result.best_point.as_slice())
                    .unwrap_or(seed_candidate.scored_candidate.search_point),
                result.best_objective,
                result.evaluation_count,
                result.stop_reason,
            ),
            None => (
                seed_candidate.scored_candidate.search_point,
                seed_candidate.scored_candidate.objective_score,
                0,
                "invalid_step",
            ),
        };

    let final_evaluation = evaluate_cached_search_point(
        optimizer_context,
        final_search_point,
        ContinuousLikelihoodFidelityTier::Full,
    )
    .or_else(|| {
        evaluate_cached_search_point(
            optimizer_context,
            seed_candidate.scored_candidate.search_point,
            ContinuousLikelihoodFidelityTier::Full,
        )
    })?;

    Some(OptimizedNigSearchCandidate {
        configuration: final_evaluation.configuration,
        family: seed_candidate.family,
        seed_search_point: seed_candidate.scored_candidate.search_point,
        final_search_point,
        optimizer_diagnostic: ContinuousReturnOptimizerDiagnostic {
            seed_objective_score: seed_candidate.scored_candidate.objective_score,
            local_objective_score,
            final_objective_score: final_evaluation.objective.shifted_objective_score,
            local_evaluation_count,
            stop_reason,
            proxy_em_diagnostic: Default::default(),
            basin_diagnostics: Vec::new(),
        },
    })
}

fn continuous_nig_basin_powell_settings(
    solver_settings: &CalibrationOptimizerSettings,
    seed_candidate_count: usize,
) -> PowellOptimizerSettings {
    let active_dimension = super::types::NigShapeSearchPoint::DIMENSION;
    let line_search_evaluation_cap = active_dimension.saturating_add(1);
    let per_seed_evaluation_budget = solver_settings
        .max_iterations
        .checked_div(seed_candidate_count.max(1))
        .unwrap_or(1)
        .max(line_search_evaluation_cap);
    let cycle_evaluation_budget = active_dimension
        .saturating_mul(line_search_evaluation_cap)
        .max(1);

    PowellOptimizerSettings {
        absolute_tolerance: solver_settings.absolute_tolerance,
        relative_tolerance: solver_settings.relative_tolerance,
        min_step_norm: solver_settings.absolute_tolerance,
        max_cycle_count: per_seed_evaluation_budget
            .checked_div(cycle_evaluation_budget)
            .unwrap_or(1)
            .max(1),
        line_search_evaluation_cap,
        evaluation_budget: per_seed_evaluation_budget,
    }
}
