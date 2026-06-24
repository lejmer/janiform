#[cfg(test)]
use std::sync::Arc;
use std::sync::Mutex;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;

use janiform_foundations::cancellation::CancellationToken;
use janiform_work_scheduler::{StaticWorkPoolInput, run_static_work_pool};

use super::super::super::contracts::JointRefinementStageProgressUpdate;
use super::super::super::execution::{
    JointRefinementContext, build_halton_candidate_id, halton_parameter_vector,
    joint_refinement_has_parallel_worker_capacity, joint_refinement_parallel_worker_count,
    precheck_seed_summary_workload,
};
use super::super::super::objective::{
    JointRefinementCandidateAdmissionDiagnostics, JointRefinementCandidateAdmissionScreening,
    evaluate_joint_fitted_candidate_admission_diagnostics,
};
#[cfg(test)]
use super::candidate_screening::candidate_admission_screening_from_scalar;
use super::candidate_screening::{ScreenedCandidate, ScreenedCandidateOutput};
use super::decode::parameter_vector_to_fitted_parameters;
use super::progress::emit_completed_candidate_screening_unit;
pub(super) fn screen_candidate_seeds<F>(
    candidate_seeds: &[(String, Vec<f64>)],
    seed_score: &F,
    joint_context: Option<&JointRefinementContext>,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    completed_screening_units: &Mutex<usize>,
    total_screening_units: usize,
) -> Vec<ScreenedCandidate>
where
    F: Fn(&str, &[f64]) -> Option<JointRefinementCandidateAdmissionScreening> + Sync + ?Sized,
{
    let thread_pool = joint_context.and_then(|joint_context| {
        joint_context
            .stage_thread_pool
            .as_ref()
            .map(|thread_pool| thread_pool.as_ref())
    });

    let cancellation_token = joint_context
        .map(|joint_context| joint_context.cancellation_token.shared_handle())
        .unwrap_or_else(CancellationToken::uncancelled);

    screen_candidate_seeds_with_thread_pool(
        candidate_seeds,
        seed_score,
        thread_pool,
        cancellation_token,
        progress_sender,
        completed_screening_units,
        total_screening_units,
    )
}

fn screen_candidate_seeds_with_thread_pool<F>(
    candidate_seeds: &[(String, Vec<f64>)],
    seed_score: &F,
    thread_pool: Option<&rayon::ThreadPool>,
    cancellation_token: CancellationToken,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    completed_screening_units: &Mutex<usize>,
    total_screening_units: usize,
) -> Vec<ScreenedCandidate>
where
    F: Fn(&str, &[f64]) -> Option<JointRefinementCandidateAdmissionScreening> + Sync + ?Sized,
{
    let screen_candidate =
        |(candidate_position, (candidate_id, seed)): (usize, &(String, Vec<f64>))| {
            let score = seed_score(candidate_id.as_str(), seed.as_slice());

            emit_completed_candidate_screening_unit(
                progress_sender,
                completed_screening_units,
                total_screening_units,
                candidate_id.as_str(),
            );

            ScreenedCandidateOutput {
                candidate_position,
                candidate: ScreenedCandidate::try_new(
                    candidate_id.as_str(),
                    seed.as_slice(),
                    score,
                    candidate_position,
                ),
            }
        };

    if let Some(thread_pool) = thread_pool
        && thread_pool.current_num_threads() > 1
        && candidate_seeds.len() > 1
    {
        let candidate_jobs = candidate_seeds.iter().enumerate().collect::<Vec<_>>();

        let mut screened_candidates = std::iter::repeat_with(|| None)
            .take(candidate_jobs.len())
            .collect::<Vec<Option<ScreenedCandidate>>>();

        run_static_work_pool(
            Some(thread_pool),
            StaticWorkPoolInput::new(
                &mut screened_candidates,
                cancellation_token.shared_handle(),
                candidate_jobs,
                thread_pool.current_num_threads(),
                |candidate: (usize, &(String, Vec<f64>)), cancellation_token: CancellationToken| {
                    if cancellation_token.is_cancelled() {
                        return ScreenedCandidateOutput {
                            candidate_position: candidate.0,
                            candidate: None,
                        };
                    }

                    screen_candidate(candidate)
                },
                |screened_candidates: &mut Vec<Option<ScreenedCandidate>>,
                 output: ScreenedCandidateOutput,
                 _status| {
                    if let Some(candidate_slot) =
                        screened_candidates.get_mut(output.candidate_position)
                    {
                        *candidate_slot = output.candidate;
                    }
                },
            ),
        );

        return screened_candidates.into_iter().flatten().collect();
    }

    candidate_seeds
        .iter()
        .enumerate()
        .filter_map(|candidate| {
            if cancellation_token.is_cancelled() {
                return None;
            }

            screen_candidate(candidate).candidate
        })
        .collect()
}

#[cfg(test)]
pub(in crate::stages::joint_refinement) fn candidate_screening_max_parallelism_for_test(
    cpu_count: usize,
    candidate_count: usize,
) -> usize {
    let thread_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(crate::stages::parallelism::resolved_stage_cpu_count(
            cpu_count,
        ))
        .build()
        .expect("test thread pool should build");

    let active_count = Arc::new(AtomicUsize::new(0));
    let max_active_count = Arc::new(AtomicUsize::new(0));

    let candidate_seeds = (0..candidate_count)
        .map(|candidate_index| {
            (
                format!("candidate_{candidate_index}"),
                vec![candidate_index as f64],
            )
        })
        .collect::<Vec<_>>();

    let completed_screening_units = Mutex::new(0usize);

    let score_candidate = |_: &str, _: &[f64]| {
        let current_active_count = active_count.fetch_add(1, Ordering::SeqCst) + 1;
        max_active_count.fetch_max(current_active_count, Ordering::SeqCst);
        std::thread::sleep(std::time::Duration::from_millis(25));
        active_count.fetch_sub(1, Ordering::SeqCst);
        candidate_admission_screening_from_scalar(0.0)
    };

    let _ = screen_candidate_seeds_with_thread_pool(
        candidate_seeds.as_slice(),
        &score_candidate,
        Some(&thread_pool),
        CancellationToken::uncancelled(),
        None,
        &completed_screening_units,
        candidate_count,
    );

    max_active_count.load(Ordering::SeqCst)
}

pub(super) fn screen_halton_candidate_seeds<F>(
    input: HaltonCandidateScreeningInput<'_, F>,
) -> Vec<ScreenedCandidate>
where
    F: Fn(&str, &[f64]) -> Option<JointRefinementCandidateAdmissionScreening> + Sync + ?Sized,
{
    let HaltonCandidateScreeningInput {
        baseline_parameter_vector,
        lower_bounds,
        upper_bounds,
        target_candidate_count,
        source_order_offset,
        halton_seed_score,
        joint_context,
        progress_sender,
        completed_screening_units,
        total_screening_units,
    } = input;

    let screen_halton_position = |halton_position: usize| {
        let halton_seed = local_trust_region_halton_parameter_vector(
            halton_position,
            baseline_parameter_vector,
            lower_bounds,
            upper_bounds,
        );

        let candidate_id = build_halton_candidate_id(halton_position);
        let score = halton_seed_score(candidate_id.as_str(), halton_seed.as_slice());

        emit_completed_candidate_screening_unit(
            progress_sender,
            completed_screening_units,
            total_screening_units,
            candidate_id.as_str(),
        );

        ScreenedCandidateOutput {
            candidate_position: halton_position.saturating_sub(1),
            candidate: score.and_then(|score| {
                ScreenedCandidate::try_new(
                    candidate_id.as_str(),
                    halton_seed.as_slice(),
                    Some(score),
                    source_order_offset.saturating_add(halton_position),
                )
            }),
        }
    };

    if let Some(joint_context) = joint_context
        && joint_refinement_has_parallel_worker_capacity(joint_context)
        && target_candidate_count > 1
    {
        let halton_jobs = (1..=target_candidate_count).collect::<Vec<_>>();

        let mut screened_candidates = std::iter::repeat_with(|| None)
            .take(target_candidate_count)
            .collect::<Vec<Option<ScreenedCandidate>>>();

        run_static_work_pool(
            joint_context.stage_thread_pool.as_deref(),
            StaticWorkPoolInput::new(
                &mut screened_candidates,
                joint_context.cancellation_token.shared_handle(),
                halton_jobs,
                joint_refinement_parallel_worker_count(joint_context),
                |halton_position, _cancellation_token| screen_halton_position(halton_position),
                |screened_candidates: &mut Vec<Option<ScreenedCandidate>>,
                 output: ScreenedCandidateOutput,
                 _status| {
                    if let Some(candidate_slot) =
                        screened_candidates.get_mut(output.candidate_position)
                    {
                        *candidate_slot = output.candidate;
                    }
                },
            ),
        );

        return screened_candidates.into_iter().flatten().collect();
    }

    (1..=target_candidate_count)
        .filter_map(|halton_position| screen_halton_position(halton_position).candidate)
        .collect()
}

fn local_trust_region_halton_parameter_vector(
    halton_position: usize,
    baseline_parameter_vector: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Vec<f64> {
    let full_bounds_halton =
        halton_parameter_vector(halton_position as u64, lower_bounds, upper_bounds);

    let radius_fraction =
        normalized_local_trust_region_radius_fraction(baseline_parameter_vector.len());

    baseline_parameter_vector
        .iter()
        .zip(lower_bounds.iter().zip(upper_bounds.iter()))
        .zip(full_bounds_halton.iter())
        .map(
            |((baseline_value, (lower_bound, upper_bound)), halton_value)| {
                local_trust_region_coordinate(
                    *baseline_value,
                    *lower_bound,
                    *upper_bound,
                    *halton_value,
                    radius_fraction,
                )
            },
        )
        .collect()
}

fn normalized_local_trust_region_radius_fraction(parameter_count: usize) -> f64 {
    // The radius keeps a full-coordinate displacement at unit length in normalized space.
    (parameter_count.max(1) as f64).sqrt().recip()
}

fn local_trust_region_coordinate(
    baseline_value: f64,
    lower_bound: f64,
    upper_bound: f64,
    full_bounds_halton_value: f64,
    radius_fraction: f64,
) -> f64 {
    let full_width = upper_bound - lower_bound;
    if !baseline_value.is_finite()
        || !lower_bound.is_finite()
        || !upper_bound.is_finite()
        || !full_width.is_finite()
        || full_width <= 0.0
    {
        return baseline_value;
    }

    let unit_position = ((full_bounds_halton_value - lower_bound) / full_width).clamp(0.0, 1.0);
    let signed_position = 2.0 * unit_position - 1.0;

    let directional_width = if signed_position < 0.0 {
        baseline_value - lower_bound
    } else {
        upper_bound - baseline_value
    };

    (baseline_value + signed_position * radius_fraction * directional_width)
        .clamp(lower_bound, upper_bound)
}

pub(super) struct HaltonCandidateScreeningInput<'a, F: ?Sized> {
    pub(super) baseline_parameter_vector: &'a [f64],
    pub(super) lower_bounds: &'a [f64],
    pub(super) upper_bounds: &'a [f64],
    pub(super) target_candidate_count: usize,
    pub(super) source_order_offset: usize,
    pub(super) halton_seed_score: &'a F,
    pub(super) joint_context: Option<&'a JointRefinementContext>,
    pub(super) progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    pub(super) completed_screening_units: &'a Mutex<usize>,
    pub(super) total_screening_units: usize,
}

pub(super) fn screened_exploratory_candidate_seed_score(
    halton_seed: &[f64],
    joint_context: &JointRefinementContext,
) -> Option<JointRefinementCandidateAdmissionScreening> {
    exploratory_candidate_seed_diagnostics(halton_seed, joint_context)
        .ok()
        .map(|diagnostics| diagnostics.screening)
}

fn exploratory_candidate_seed_diagnostics(
    candidate_seed: &[f64],
    joint_context: &JointRefinementContext,
) -> Result<JointRefinementCandidateAdmissionDiagnostics, &'static str> {
    let Some(fitted_parameters) =
        parameter_vector_to_fitted_parameters(candidate_seed, joint_context)
    else {
        return Err("candidate_seed_parameter_decoding_failed");
    };

    precheck_seed_summary_workload(&fitted_parameters, joint_context)
        .map_err(|_| "candidate_seed_summary_workload_infeasible")?;

    evaluate_joint_fitted_candidate_admission_diagnostics(&fitted_parameters, joint_context)
}
