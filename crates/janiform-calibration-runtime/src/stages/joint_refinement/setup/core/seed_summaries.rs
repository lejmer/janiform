use std::sync::Mutex;
use std::sync::mpsc::Sender;
use std::time::Instant;

use chrono::{DateTime, Utc};
use janiform_foundations::cancellation::CancellationToken;
use janiform_work_scheduler::{StaticWorkPoolInput, run_static_work_pool};

use super::super::super::contracts::{
    JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate,
};
use super::super::super::execution::{
    JointRefinementContext, JointRefinementSeedSummary, JointRefinementSeedSummaryCacheKey,
    joint_refinement_has_parallel_worker_capacity, joint_refinement_parallel_worker_count,
};
use super::super::super::worker_runtime::JointRefinementSeedSummaryAcquisitionError;
use janiform_calibration_artifacts::{
    CalibrationOptimizerSettings, JointRefinementStageFittedParameters,
};
pub(in crate::stages::joint_refinement) fn joint_refinement_final_simulation_seed_count(
    solver_settings: &CalibrationOptimizerSettings,
) -> usize {
    match solver_settings.max_iterations {
        0..=32 => 1,
        33..=96 => 2,
        _ => 3,
    }
}

pub(in crate::stages::joint_refinement) fn cached_seed_summaries_for_candidate_seeds(
    input: CandidateSeedSummariesCacheInput<'_>,
) -> Result<Vec<(u64, JointRefinementSeedSummary)>, String> {
    let CandidateSeedSummariesCacheInput {
        parameter_bits,
        fitted_parameters,
        context,
        simulation_seeds,
        progress_sender,
        progress_phase,
        completed_units_before_seed_summaries,
        total_units,
    } = input;

    let cache_key = parameter_bits.map(|bits| JointRefinementSeedSummaryCacheKey {
        parameter_bits: bits.to_vec(),
        simulation_seed: 0,
    });

    let mut resolved_seed_summaries = Vec::with_capacity(simulation_seeds.len());
    let mut missing_seeds = Vec::new();

    for simulation_seed in simulation_seeds {
        let per_seed_cache_key =
            cache_key
                .as_ref()
                .map(|candidate_cache_key| JointRefinementSeedSummaryCacheKey {
                    parameter_bits: candidate_cache_key.parameter_bits.clone(),
                    simulation_seed: *simulation_seed,
                });

        if let Some(seed_cache_key) = per_seed_cache_key.as_ref()
            && let Ok(cache_guard) = context.seed_summary_cache.lock()
            && let Some(cached_summary) = cache_guard.get(seed_cache_key)
        {
            context.profiling_state.record_seed_summary_cache_hit();

            // The resolved result owns its reference independently of the cache lock.
            let summary = cached_summary.clone()?;

            resolved_seed_summaries.push((*simulation_seed, summary));

            continue;
        }

        missing_seeds.push(*simulation_seed);
    }

    context
        .profiling_state
        .record_seed_summary_cache_miss(missing_seeds.len());

    let completed_units = completed_units_before_seed_summaries + resolved_seed_summaries.len();

    emit_seed_summary_progress(
        progress_sender,
        progress_phase,
        completed_units,
        total_units,
        None,
    );

    if !missing_seeds.is_empty() {
        let missing_seed_summaries = build_seed_summaries_for_candidate_seeds(
            context,
            fitted_parameters,
            missing_seeds.as_slice(),
            progress_sender,
            progress_phase,
            completed_units,
            total_units,
        )?;

        if let Some(candidate_cache_key) = cache_key
            && let Ok(mut cache_guard) = context.seed_summary_cache.lock()
        {
            for (simulation_seed, seed_summary) in &missing_seed_summaries {
                cache_guard.insert(
                    JointRefinementSeedSummaryCacheKey {
                        parameter_bits: candidate_cache_key.parameter_bits.clone(),
                        simulation_seed: *simulation_seed,
                    },
                    Ok(seed_summary.clone()),
                );
            }
        }

        resolved_seed_summaries.extend(missing_seed_summaries);
    }

    resolved_seed_summaries.sort_by_key(|(simulation_seed, _)| {
        simulation_seeds
            .iter()
            .position(|candidate_seed| candidate_seed == simulation_seed)
            .unwrap_or(simulation_seeds.len())
    });

    Ok(resolved_seed_summaries)
}

pub(in crate::stages::joint_refinement) struct CandidateSeedSummariesCacheInput<'a> {
    pub(in crate::stages::joint_refinement) parameter_bits: Option<&'a [u64]>,
    pub(in crate::stages::joint_refinement) fitted_parameters:
        &'a JointRefinementStageFittedParameters,
    pub(in crate::stages::joint_refinement) context: &'a JointRefinementContext,
    pub(in crate::stages::joint_refinement) simulation_seeds: &'a [u64],
    pub(in crate::stages::joint_refinement) progress_sender:
        Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    pub(in crate::stages::joint_refinement) progress_phase:
        Option<JointRefinementStageProgressPhase>,
    pub(in crate::stages::joint_refinement) completed_units_before_seed_summaries: usize,
    pub(in crate::stages::joint_refinement) total_units: usize,
}

struct SeedSummaryOutput {
    seed_position: usize,
    result: Result<(u64, JointRefinementSeedSummary), String>,
}

pub(crate) fn build_surprise_multipliers_for_interval_grid(
    simulation_start_utc: DateTime<Utc>,
    interval_count: usize,
    interval_duration_nanos: i64,
    surprise_profile: &janiform_calibration_artifacts::SurpriseNewsDeterministicIntensityProfile,
) -> Result<Vec<(u64, f64)>, String> {
    crate::surprise_profile::materialize_surprise_news_weekly_profile_multipliers_for_interval_grid(
        simulation_start_utc,
        interval_count,
        interval_duration_nanos,
        surprise_profile,
    )
    .map_err(|source| source.to_string())
}

fn build_seed_summaries_for_candidate_seeds(
    context: &JointRefinementContext,
    fitted_parameters: &JointRefinementStageFittedParameters,
    simulation_seeds: &[u64],
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    progress_phase: Option<JointRefinementStageProgressPhase>,
    completed_units_before_missing: usize,
    total_units: usize,
) -> Result<Vec<(u64, JointRefinementSeedSummary)>, String> {
    if joint_refinement_has_parallel_worker_capacity(context) && simulation_seeds.len() > 1 {
        return build_seed_summaries_for_candidate_seeds_parallel(
            context,
            fitted_parameters,
            simulation_seeds,
            progress_sender,
            progress_phase,
            completed_units_before_missing,
            total_units,
        );
    }

    let mut completed_units = completed_units_before_missing;
    let mut seed_summaries = Vec::with_capacity(simulation_seeds.len());

    for simulation_seed in simulation_seeds {
        let seed_summary =
            build_seed_summary_for_candidate_seed(context, fitted_parameters, *simulation_seed)?;

        completed_units += 1;

        emit_seed_summary_progress(
            progress_sender,
            progress_phase,
            completed_units,
            total_units,
            Some(*simulation_seed),
        );

        seed_summaries.push((*simulation_seed, seed_summary));
    }

    Ok(seed_summaries)
}

fn build_seed_summaries_for_candidate_seeds_parallel(
    context: &JointRefinementContext,
    fitted_parameters: &JointRefinementStageFittedParameters,
    simulation_seeds: &[u64],
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    progress_phase: Option<JointRefinementStageProgressPhase>,
    completed_units_before_missing: usize,
    total_units: usize,
) -> Result<Vec<(u64, JointRefinementSeedSummary)>, String> {
    let completed_units = Mutex::new(completed_units_before_missing);

    let seed_summary_jobs = simulation_seeds
        .iter()
        .copied()
        .enumerate()
        .collect::<Vec<_>>();

    let mut seed_summary_outputs = std::iter::repeat_with(|| None)
        .take(seed_summary_jobs.len())
        .collect::<Vec<Option<Result<(u64, JointRefinementSeedSummary), String>>>>();

    run_static_work_pool(
        context.stage_thread_pool.as_deref(),
        StaticWorkPoolInput::new(
            &mut seed_summary_outputs,
            context.cancellation_token.shared_handle(),
            seed_summary_jobs,
            joint_refinement_parallel_worker_count(context),
            |(seed_position, simulation_seed), cancellation_token: CancellationToken| {
                if cancellation_token.is_cancelled() {
                    return SeedSummaryOutput {
                        seed_position,
                        result: Err("operation cancelled".to_string()),
                    };
                }

                let result = build_seed_summary_for_candidate_seed(
                    context,
                    fitted_parameters,
                    simulation_seed,
                )
                .map(|seed_summary| (simulation_seed, seed_summary));

                if result.is_ok()
                    && let Ok(mut completed_units_guard) = completed_units.lock()
                {
                    *completed_units_guard = completed_units_guard.saturating_add(1);
                    emit_seed_summary_progress(
                        progress_sender,
                        progress_phase,
                        *completed_units_guard,
                        total_units,
                        Some(simulation_seed),
                    );
                }

                SeedSummaryOutput {
                    seed_position,
                    result,
                }
            },
            |seed_summary_outputs: &mut Vec<
                Option<Result<(u64, JointRefinementSeedSummary), String>>,
            >,
             output: SeedSummaryOutput,
             _status| {
                if let Some(output_slot) = seed_summary_outputs.get_mut(output.seed_position) {
                    *output_slot = Some(output.result);
                }
            },
        ),
    );

    seed_summary_outputs
        .into_iter()
        .map(|output| {
            output.unwrap_or_else(|| Err("seed summary worker did not return result".to_string()))
        })
        .collect()
}

fn build_seed_summary_for_candidate_seed(
    context: &JointRefinementContext,
    fitted_parameters: &JointRefinementStageFittedParameters,
    simulation_seed: u64,
) -> Result<JointRefinementSeedSummary, String> {
    let seed_summary_started_at = Instant::now();

    let seed_summary_result = context
        .summary_simulation_scheduler
        .run_seed_summary_simulation(
            fitted_parameters,
            simulation_seed,
            context.cancellation_token.shared_handle(),
        );

    context
        .profiling_state
        .record_seed_summary_simulation_duration(
            seed_summary_started_at
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
        );

    seed_summary_result.map_err(|error| match error {
        JointRefinementSeedSummaryAcquisitionError::SimulationFailure { error_message } => {
            error_message
        }
        JointRefinementSeedSummaryAcquisitionError::InfrastructureFailure { error_message } => {
            if let Ok(mut failure_guard) = context.simulation_infrastructure_failure.lock()
                && failure_guard.is_none()
            {
                *failure_guard = Some(error_message.clone());
            }

            error_message
        }
    })
}

fn emit_seed_summary_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    progress_phase: Option<JointRefinementStageProgressPhase>,
    completed_units: usize,
    total_units: usize,
    simulation_seed: Option<u64>,
) {
    let Some(progress_phase) = progress_phase else {
        return;
    };

    let Some(progress_sender) = progress_sender else {
        return;
    };

    let _ = progress_sender.send(JointRefinementStageProgressUpdate {
        current_item_label: simulation_seed.map(format_seed_summary_progress_item),
        ..JointRefinementStageProgressUpdate::new(progress_phase, completed_units, total_units)
    });
}

fn format_seed_summary_progress_item(simulation_seed: u64) -> String {
    format!("simulation_seed {simulation_seed}")
}
