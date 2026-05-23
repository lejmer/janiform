use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

use super::super::domain::SpectralDomain;
use super::super::{
    LocalTransitionRequestContext, LocalTransitionRowState, SpectralThetaWarmStartRowState,
    VanillaOptionPricingRunError, install_pricing_parallelism,
    resolve_local_transition_theta_for_row_with_parallelism_and_hint, resolved_pricing_cpu_count,
};
use super::{
    AdaptiveMidpointRowFrequencyBatchSolveInput, AdaptiveRowSolveTask,
    ExactRowFrequencyBatchSolveInput, RowFrequencyBatchSolution,
    build_row_frequency_batch_from_shared_kernel, interpolate_theta_across_deviation_rows,
    solve_adaptive_midpoint_row_frequency_batch, solve_exact_row_frequency_batch,
    solve_shared_row_frequency_batch_kernel,
};

pub(in crate::spectral::pricer) struct IntervalRowFrequencyBatchAdaptiveSolveInput<'a> {
    pub(in crate::spectral::pricer) base_context: &'a LocalTransitionRequestContext,
    pub(in crate::spectral::pricer) collocation_nodes: &'a [f64],
    pub(in crate::spectral::pricer) row_states: &'a [LocalTransitionRowState],
    pub(in crate::spectral::pricer) theta_hints: &'a [Option<f64>],
    pub(in crate::spectral::pricer) next_anchor_value: f64,
    pub(in crate::spectral::pricer) spectral_domain: SpectralDomain,
    pub(in crate::spectral::pricer) driver_parameters:
        janiform_model_kernels::nig::NigBuySellDriverParameters,
    pub(in crate::spectral::pricer) collect_theta_warm_start_sensitivities: bool,
    pub(in crate::spectral::pricer) pricing_thread_pool: Option<&'a rayon::ThreadPool>,
    pub(in crate::spectral::pricer) cpu_count: Option<usize>,
    pub(in crate::spectral::pricer) progress_observer: Option<&'a mut dyn FnMut(usize)>,
}

pub(in crate::spectral::pricer) fn solve_interval_row_frequency_batches_adaptively(
    input: IntervalRowFrequencyBatchAdaptiveSolveInput<'_>,
) -> Result<
    (
        Vec<RowFrequencyBatchSolution>,
        Vec<SpectralThetaWarmStartRowState>,
    ),
    VanillaOptionPricingRunError,
> {
    let IntervalRowFrequencyBatchAdaptiveSolveInput {
        base_context,
        collocation_nodes,
        row_states,
        theta_hints,
        next_anchor_value,
        spectral_domain,
        driver_parameters,
        collect_theta_warm_start_sensitivities,
        pricing_thread_pool,
        cpu_count,
        mut progress_observer,
    } = input;

    let row_count = collocation_nodes.len();
    if row_count == 0 {
        return Ok((Vec::new(), Vec::new()));
    }

    if row_states
        .iter()
        .all(LocalTransitionRowState::has_row_independent_local_tilt_target)
    {
        return solve_row_independent_interval_row_frequency_batches_with_shared_theta(
            RowIndependentIntervalRowFrequencyBatchSharedThetaSolveInput {
                base_context,
                row_states,
                theta_hints,
                next_anchor_value,
                spectral_domain,
                driver_parameters,
                collect_theta_warm_start_sensitivities,
                pricing_thread_pool,
                cpu_count,
                progress_observer,
            },
        );
    }

    let mut solved_rows = vec![None; row_count];
    let mut solved_theta_warm_start_states = vec![None; row_count];

    let mut seeded_row_indices =
        derive_parallel_adaptive_seed_row_indices(row_count, resolved_pricing_cpu_count(cpu_count));

    for (row_index, row_state) in row_states.iter().enumerate() {
        if row_state.has_row_independent_local_tilt_target() {
            seeded_row_indices.push(row_index);
        }
    }

    seeded_row_indices.sort_unstable();
    seeded_row_indices.dedup();

    let seeded_row_solutions =
        if resolved_pricing_cpu_count(cpu_count) > 1 && seeded_row_indices.len() > 1 {
            install_pricing_parallelism(pricing_thread_pool, || {
                seeded_row_indices
                    .par_iter()
                    .map(|&row_index| {
                        solve_exact_row_frequency_batch(ExactRowFrequencyBatchSolveInput {
                            base_context,
                            transition_row: &row_states[row_index],
                            initial_theta_hint: theta_hints[row_index],
                            next_anchor_value,
                            spectral_domain,
                            driver_parameters,
                            collect_theta_warm_start_sensitivities,
                            pricing_thread_pool,
                            cpu_count,
                        })
                        .map(|solution| (row_index, solution))
                    })
                    .collect::<Result<Vec<_>, _>>()
            })?
        } else {
            let mut seeded_solutions = Vec::with_capacity(seeded_row_indices.len());

            for &row_index in &seeded_row_indices {
                seeded_solutions.push((
                    row_index,
                    solve_exact_row_frequency_batch(ExactRowFrequencyBatchSolveInput {
                        base_context,
                        transition_row: &row_states[row_index],
                        initial_theta_hint: theta_hints[row_index],
                        next_anchor_value,
                        spectral_domain,
                        driver_parameters,
                        collect_theta_warm_start_sensitivities,
                        pricing_thread_pool,
                        cpu_count,
                    })?,
                ));
            }
            seeded_solutions
        };

    let mut completed_row_count = 0usize;

    for (row_index, solution) in seeded_row_solutions {
        solved_theta_warm_start_states[row_index] = Some(solution.theta_warm_start_state);
        solved_rows[row_index] = Some(solution.solution);
        completed_row_count += 1;
        emit_interval_row_progress(&mut progress_observer, completed_row_count);
    }

    let mut active_segments = seeded_row_indices
        .windows(2)
        .filter_map(|window| {
            let left = window[0];
            let right = window[1];
            (right > left + 1).then_some((left, right))
        })
        .collect::<Vec<_>>();

    while !active_segments.is_empty() {
        let tasks = active_segments
            .iter()
            .map(|(left_row_index, right_row_index)| {
                let left_theta = solved_rows[*left_row_index]
                    .as_ref()
                    .ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
                        message: format!(
                            "adaptive spectral interval operator left row {left_row_index} unsolved"
                        ),
                    })?
                    .theta;

                let right_theta = solved_rows[*right_row_index]
                    .as_ref()
                    .ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
                        message: format!(
                            "adaptive spectral interval operator right row {right_row_index} unsolved"
                        ),
                    })?
                    .theta;

                let midpoint_row_index = (left_row_index + right_row_index) / 2;

                let predicted_theta = interpolate_theta_across_deviation_rows(
                    collocation_nodes[*left_row_index],
                    left_theta,
                    collocation_nodes[*right_row_index],
                    right_theta,
                    collocation_nodes[midpoint_row_index],
                );

                Ok(AdaptiveRowSolveTask {
                    left_row_index: *left_row_index,
                    midpoint_row_index,
                    right_row_index: *right_row_index,
                    predicted_theta,
                })
            })
            .collect::<Result<Vec<_>, VanillaOptionPricingRunError>>()?;

        let solved_midpoints = if resolved_pricing_cpu_count(cpu_count) > 1 && tasks.len() > 1 {
            install_pricing_parallelism(pricing_thread_pool, || {
                tasks
                    .par_iter()
                    .map(|task| {
                        solve_adaptive_midpoint_row_frequency_batch(
                            AdaptiveMidpointRowFrequencyBatchSolveInput {
                                base_context,
                                transition_row: &row_states[task.midpoint_row_index],
                                initial_theta_hint: theta_hints[task.midpoint_row_index]
                                    .or(Some(task.predicted_theta)),
                                predicted_theta: task.predicted_theta,
                                next_anchor_value,
                                spectral_domain,
                                driver_parameters,
                                collect_theta_warm_start_sensitivities,
                                pricing_thread_pool,
                                cpu_count,
                            },
                        )
                        .map(|solution| (task.midpoint_row_index, solution))
                    })
                    .collect::<Result<Vec<_>, _>>()
            })?
        } else {
            let mut midpoint_solutions = Vec::with_capacity(tasks.len());

            for task in &tasks {
                midpoint_solutions.push((
                    task.midpoint_row_index,
                    solve_adaptive_midpoint_row_frequency_batch(
                        AdaptiveMidpointRowFrequencyBatchSolveInput {
                            base_context,
                            transition_row: &row_states[task.midpoint_row_index],
                            initial_theta_hint: theta_hints[task.midpoint_row_index]
                                .or(Some(task.predicted_theta)),
                            predicted_theta: task.predicted_theta,
                            next_anchor_value,
                            spectral_domain,
                            driver_parameters,
                            collect_theta_warm_start_sensitivities,
                            pricing_thread_pool,
                            cpu_count,
                        },
                    )?,
                ));
            }

            midpoint_solutions
        };

        for (midpoint_row_index, solution) in solved_midpoints {
            solved_theta_warm_start_states[midpoint_row_index] =
                Some(solution.theta_warm_start_state);

            solved_rows[midpoint_row_index] = Some(solution.solution);

            completed_row_count += 1;

            emit_interval_row_progress(&mut progress_observer, completed_row_count);
        }

        let mut next_segments = Vec::new();

        for task in tasks {
            if task.midpoint_row_index > task.left_row_index + 1 {
                next_segments.push((task.left_row_index, task.midpoint_row_index));
            }

            if task.right_row_index > task.midpoint_row_index + 1 {
                next_segments.push((task.midpoint_row_index, task.right_row_index));
            }
        }

        active_segments = next_segments;
    }

    let row_operators = solved_rows
        .into_iter()
        .enumerate()
        .map(|(row_index, solution)| {
            solution.ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
                message: format!(
                    "adaptive spectral interval operator left row {row_index} unsolved"
                ),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let theta_warm_start_states = solved_theta_warm_start_states
        .into_iter()
        .enumerate()
        .map(|(row_index, row_state)| {
            row_state.ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
                message: format!(
                    "adaptive spectral theta warm-start state left row {row_index} unsolved"
                ),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok((row_operators, theta_warm_start_states))
}

pub(super) fn derive_parallel_adaptive_seed_row_indices(
    row_count: usize,
    resolved_cpu_count: usize,
) -> Vec<usize> {
    if row_count <= 1 {
        return vec![0];
    }

    let last_row_index = row_count - 1;

    // Keep at least one unsolved interior row inside every initial span so the
    // adaptive midpoint certification step has meaningful work to do.
    let target_segment_count = resolved_cpu_count.min(last_row_index / 2).max(1);

    let mut seeded_row_indices = Vec::with_capacity(target_segment_count + 1);

    for segment_boundary_index in 0..=target_segment_count {
        seeded_row_indices.push(segment_boundary_index * last_row_index / target_segment_count);
    }

    seeded_row_indices
}

pub(super) struct RowIndependentIntervalRowFrequencyBatchSharedThetaSolveInput<'a> {
    pub(in crate::spectral::pricer) base_context: &'a LocalTransitionRequestContext,
    pub(in crate::spectral::pricer) row_states: &'a [LocalTransitionRowState],
    pub(in crate::spectral::pricer) theta_hints: &'a [Option<f64>],
    pub(in crate::spectral::pricer) next_anchor_value: f64,
    pub(in crate::spectral::pricer) spectral_domain: SpectralDomain,
    pub(in crate::spectral::pricer) driver_parameters:
        janiform_model_kernels::nig::NigBuySellDriverParameters,
    pub(in crate::spectral::pricer) collect_theta_warm_start_sensitivities: bool,
    pub(in crate::spectral::pricer) pricing_thread_pool: Option<&'a rayon::ThreadPool>,
    pub(in crate::spectral::pricer) cpu_count: Option<usize>,
    pub(in crate::spectral::pricer) progress_observer: Option<&'a mut dyn FnMut(usize)>,
}

pub(super) fn solve_row_independent_interval_row_frequency_batches_with_shared_theta(
    input: RowIndependentIntervalRowFrequencyBatchSharedThetaSolveInput<'_>,
) -> Result<
    (
        Vec<RowFrequencyBatchSolution>,
        Vec<SpectralThetaWarmStartRowState>,
    ),
    VanillaOptionPricingRunError,
> {
    let RowIndependentIntervalRowFrequencyBatchSharedThetaSolveInput {
        base_context,
        row_states,
        theta_hints,
        next_anchor_value,
        spectral_domain,
        driver_parameters,
        collect_theta_warm_start_sensitivities: _collect_theta_warm_start_sensitivities,
        pricing_thread_pool,
        cpu_count,
        mut progress_observer,
    } = input;

    let representative_row_index = row_states
        .iter()
        .enumerate()
        .min_by(|(_, left_row_state), (_, right_row_state)| {
            left_row_state
                .boundary_state
                .deviation
                .abs()
                .total_cmp(&right_row_state.boundary_state.deviation.abs())
        })
        .map(|(row_index, _)| row_index)
        .ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
            message: "row-independent spectral interval solve expected at least one row"
                .to_string(),
        })?;

    let (theta, _) = resolve_local_transition_theta_for_row_with_parallelism_and_hint(
        base_context,
        &row_states[representative_row_index],
        driver_parameters,
        theta_hints[representative_row_index],
        pricing_thread_pool,
        cpu_count,
    )?;

    emit_interval_row_progress(&mut progress_observer, 1);

    let shared_kernel = solve_shared_row_frequency_batch_kernel(
        base_context,
        spectral_domain,
        driver_parameters,
        theta,
    )?;

    let row_operators = if resolved_pricing_cpu_count(cpu_count) > 1 && row_states.len() > 1 {
        install_pricing_parallelism(pricing_thread_pool, || {
            row_states
                .par_iter()
                .map(|row_state| {
                    build_row_frequency_batch_from_shared_kernel(
                        base_context,
                        row_state,
                        next_anchor_value,
                        spectral_domain,
                        &shared_kernel,
                    )
                })
                .collect::<Result<Vec<_>, _>>()
        })?
    } else {
        let mut row_operators = Vec::with_capacity(row_states.len());

        for row_state in row_states {
            row_operators.push(build_row_frequency_batch_from_shared_kernel(
                base_context,
                row_state,
                next_anchor_value,
                spectral_domain,
                &shared_kernel,
            )?);
        }

        row_operators
    };

    emit_interval_row_progress(&mut progress_observer, row_states.len());

    Ok((
        row_operators,
        vec![SpectralThetaWarmStartRowState::from_theta_hint(theta); row_states.len()],
    ))
}

pub(super) fn emit_interval_row_progress(
    progress_observer: &mut Option<&mut dyn FnMut(usize)>,
    completed_row_count: usize,
) {
    if let Some(progress_observer) = progress_observer.as_deref_mut() {
        progress_observer(completed_row_count);
    }
}
