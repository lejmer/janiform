use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

use super::super::domain::SpectralDomain;
use super::super::math::{
    ComplexExpectationSensitivityInput, complex_expectation_sensitivity, complex_scale_sensitivity,
    evaluate_cosine_series, evaluate_cosine_series_sensitivity,
};
use super::super::row_frequency::{
    IntervalRowFrequencyBatchAdaptiveSolveInput, RowFrequencyBatchSolution,
    RowFrequencyBatchSolutionSharedPlusShiftSensitivity, build_row_operator_sensitivities,
    solve_interval_row_frequency_batches_adaptively, theta_warm_start_driver_configuration,
};
use super::super::{
    ComplexScalar, LocalTransitionRequestContext, NodePriceSharedPlusShiftComplexSensitivity,
    PricingProgressEvent, RiskNeutralAnchorMaterialization, SpectralPricingIntervalDiagnostics,
    SpectralThetaWarmStartRowState, SpectralThetaWarmStartState,
    VanillaOptionPricingIntervalFactors, VanillaOptionPricingRequest, VanillaOptionPricingRunError,
    complex_multiply, emit_pricing_progress, install_pricing_parallelism,
    resolved_pricing_cpu_count, values_to_cosine_coefficients,
    values_to_cosine_coefficients_sensitivities,
};
use crate::grids::{
    SpectralThetaWarmStartKey, VanillaOptionGridBatchPricingOperatorApproximation,
    VanillaOptionGridBatchPricingOperatorApproximationDiagnostics,
};

use super::bucketing::{
    BucketedCachedIntervalOperatorBuildInput, build_bucketed_cached_interval_operators,
};

#[derive(Debug, Clone)]
pub(in crate::spectral::pricer) struct CachedIntervalOperator {
    pub(in crate::spectral::pricer) interval_summary: SpectralPricingIntervalDiagnostics,
    pub(in crate::spectral::pricer) row_operators: Vec<RowFrequencyBatchSolution>,
    pub(in crate::spectral::pricer) row_operator_sensitivities:
        Option<Vec<RowFrequencyBatchSolutionSharedPlusShiftSensitivity>>,
}

#[derive(Debug, Clone)]
pub(in crate::spectral::pricer) struct SolvedMaturityLaw {
    pub(in crate::spectral::pricer) maturity_interval: f64,
    pub(in crate::spectral::pricer) terminal_anchor_value: f64,
    pub(in crate::spectral::pricer) deviation_lower_bound: f64,
    pub(in crate::spectral::pricer) deviation_upper_bound: f64,
    pub(in crate::spectral::pricer) node_count: usize,
    pub(in crate::spectral::pricer) observed_log_price: f64,
    pub(in crate::spectral::pricer) representative_transform:
        Option<janiform_affine_oracle::affine_oracle::ExponentialAffineTransformIntegrationResult>,
    pub(in crate::spectral::pricer) characteristic_function_values: Vec<ComplexScalar>,
    pub(in crate::spectral::pricer) characteristic_function_sensitivities:
        Option<Vec<NodePriceSharedPlusShiftComplexSensitivity>>,
    pub(in crate::spectral::pricer) interval_summaries: Vec<SpectralPricingIntervalDiagnostics>,
}

pub(in crate::spectral::pricer) struct CachedIntervalOperatorBuildInput<'a, 'observer, 'callback> {
    pub(in crate::spectral::pricer) theta_warm_start_key: SpectralThetaWarmStartKey,
    pub(in crate::spectral::pricer) pricing_request: &'a VanillaOptionPricingRequest,
    pub(in crate::spectral::pricer) include_node_price_sensitivities: bool,
    pub(in crate::spectral::pricer) interval_factors: &'a [VanillaOptionPricingIntervalFactors],
    pub(in crate::spectral::pricer) materialization: &'a RiskNeutralAnchorMaterialization,
    pub(in crate::spectral::pricer) maximum_maturity_count: usize,
    pub(in crate::spectral::pricer) maturity_counts: &'a [usize],
    pub(in crate::spectral::pricer) spectral_domain: SpectralDomain,
    pub(in crate::spectral::pricer) driver_parameters:
        janiform_model_kernels::nig::NigBuySellDriverParameters,
    pub(in crate::spectral::pricer) operator_approximation:
        VanillaOptionGridBatchPricingOperatorApproximation,
    pub(in crate::spectral::pricer) pricing_thread_pool: Option<&'a rayon::ThreadPool>,
    pub(in crate::spectral::pricer) cpu_count: Option<usize>,
    pub(in crate::spectral::pricer) observer:
        &'observer mut Option<&'callback mut dyn FnMut(PricingProgressEvent)>,
    pub(in crate::spectral::pricer) theta_warm_start_state:
        Option<&'a mut SpectralThetaWarmStartState>,
}

#[derive(Debug)]
pub(in crate::spectral::pricer) struct CachedIntervalOperatorBuildOutput {
    pub(in crate::spectral::pricer) operators: Vec<CachedIntervalOperator>,
    pub(in crate::spectral::pricer) approximation_diagnostics:
        Option<VanillaOptionGridBatchPricingOperatorApproximationDiagnostics>,
}

#[derive(Debug, Clone)]
pub(in crate::spectral::pricer) struct CachedIntervalOperatorJob {
    pub(in crate::spectral::pricer) interval_offset: usize,
    pub(in crate::spectral::pricer) interval_factor: VanillaOptionPricingIntervalFactors,
    pub(in crate::spectral::pricer) theta_hints: Vec<Option<f64>>,
}

#[derive(Debug, Clone)]
pub(in crate::spectral::pricer) struct CachedIntervalOperatorSolveOutput {
    pub(in crate::spectral::pricer) interval_offset: usize,
    pub(in crate::spectral::pricer) operator: CachedIntervalOperator,
    pub(in crate::spectral::pricer) row_theta_states: Vec<SpectralThetaWarmStartRowState>,
}

#[derive(Clone, Copy)]
pub(in crate::spectral::pricer) struct CachedIntervalOperatorSolveContext<'a> {
    pub(in crate::spectral::pricer) pricing_request: &'a VanillaOptionPricingRequest,
    pub(in crate::spectral::pricer) include_node_price_sensitivities: bool,
    pub(in crate::spectral::pricer) materialization: &'a RiskNeutralAnchorMaterialization,
    pub(in crate::spectral::pricer) maximum_maturity_count: usize,
    pub(in crate::spectral::pricer) spectral_domain: SpectralDomain,
    pub(in crate::spectral::pricer) driver_parameters:
        janiform_model_kernels::nig::NigBuySellDriverParameters,
    pub(in crate::spectral::pricer) collect_theta_warm_start_sensitivities: bool,
    pub(in crate::spectral::pricer) pricing_thread_pool: Option<&'a rayon::ThreadPool>,
    pub(in crate::spectral::pricer) cpu_count: Option<usize>,
}

pub(in crate::spectral::pricer) fn build_cached_interval_operators(
    input: CachedIntervalOperatorBuildInput<'_, '_, '_>,
) -> Result<CachedIntervalOperatorBuildOutput, VanillaOptionPricingRunError> {
    let CachedIntervalOperatorBuildInput {
        theta_warm_start_key,
        pricing_request,
        include_node_price_sensitivities,
        interval_factors,
        materialization,
        maximum_maturity_count,
        maturity_counts,
        spectral_domain,
        driver_parameters,
        operator_approximation,
        pricing_thread_pool,
        cpu_count,
        observer,
        mut theta_warm_start_state,
    } = input;

    let collocation_nodes = spectral_domain.collocation_nodes();

    let current_driver_configuration = theta_warm_start_driver_configuration(driver_parameters);
    let collect_theta_warm_start_sensitivities = theta_warm_start_state.is_some();

    let jobs = interval_factors
        .iter()
        .copied()
        .enumerate()
        .take(maximum_maturity_count)
        .map(|(interval_offset, interval_factor)| {
            let theta_hints = theta_warm_start_state
                .as_deref_mut()
                .map(|state| {
                    state.interval_theta_hints(
                        theta_warm_start_key,
                        interval_offset,
                        collocation_nodes.len(),
                        current_driver_configuration,
                    )
                })
                .unwrap_or_else(|| vec![None; collocation_nodes.len()]);

            CachedIntervalOperatorJob {
                interval_offset,
                interval_factor,
                theta_hints,
            }
        })
        .collect::<Vec<_>>();

    let solve_context = CachedIntervalOperatorSolveContext {
        pricing_request,
        include_node_price_sensitivities,
        materialization,
        maximum_maturity_count,
        spectral_domain,
        driver_parameters,
        collect_theta_warm_start_sensitivities,
        pricing_thread_pool,
        cpu_count,
    };

    if operator_approximation == VanillaOptionGridBatchPricingOperatorApproximation::BucketedSearch
        && !include_node_price_sensitivities
    {
        return build_bucketed_cached_interval_operators(
            BucketedCachedIntervalOperatorBuildInput {
                theta_warm_start_key,
                jobs: jobs.as_slice(),
                solve_context,
                maturity_counts,
                interval_factors,
                materialization,
                driver_parameters,
                observer,
                theta_warm_start_state,
            },
        );
    }

    if maximum_maturity_count > 0 {
        emit_pricing_progress(
            observer,
            PricingProgressEvent::interval_progress(
                0,
                maximum_maturity_count,
                Some(0),
                maximum_maturity_count,
            ),
        );
    }

    let mut solved_interval_outputs =
        if resolved_pricing_cpu_count(cpu_count) > 1 && maximum_maturity_count > 1 {
            solve_cached_interval_operators_in_parallel(jobs.as_slice(), solve_context, observer)?
        } else {
            solve_cached_interval_operators_serially(jobs.as_slice(), solve_context, observer)?
        };

    solved_interval_outputs.sort_by_key(|output| output.interval_offset);

    let mut cached_interval_operators = Vec::with_capacity(solved_interval_outputs.len());

    for solved_interval_output in solved_interval_outputs {
        if let Some(state) = theta_warm_start_state.as_deref_mut() {
            state.record_interval_theta_states(
                theta_warm_start_key,
                solved_interval_output.interval_offset,
                current_driver_configuration,
                solved_interval_output.row_theta_states,
            );
        }

        cached_interval_operators.push(solved_interval_output.operator);
    }

    if maximum_maturity_count > 0 {
        emit_pricing_progress(
            observer,
            PricingProgressEvent::interval_progress(
                maximum_maturity_count,
                maximum_maturity_count,
                None,
                maximum_maturity_count,
            ),
        );
    }

    let approximation_diagnostics = (operator_approximation
        == VanillaOptionGridBatchPricingOperatorApproximation::BucketedSearch)
        .then_some(
            VanillaOptionGridBatchPricingOperatorApproximationDiagnostics {
                exact_interval_count: maximum_maturity_count,
                representative_kernel_count: maximum_maturity_count,
                reused_interval_count: 0,
                compression_ratio: 1.0,
                exact_pricing_forced_by_sensitivities: include_node_price_sensitivities,
            },
        );

    Ok(CachedIntervalOperatorBuildOutput {
        operators: cached_interval_operators,
        approximation_diagnostics,
    })
}

pub(in crate::spectral::pricer) fn solve_cached_interval_operators_serially(
    jobs: &[CachedIntervalOperatorJob],
    solve_context: CachedIntervalOperatorSolveContext<'_>,
    observer: &mut Option<&mut dyn FnMut(PricingProgressEvent)>,
) -> Result<Vec<CachedIntervalOperatorSolveOutput>, VanillaOptionPricingRunError> {
    let mut outputs = Vec::with_capacity(jobs.len());

    for (job_position, job) in jobs.iter().enumerate() {
        let output = solve_cached_interval_operator(job, solve_context)?;
        let completed_interval_count = job_position + 1;

        emit_pricing_progress(
            observer,
            PricingProgressEvent::interval_progress(
                completed_interval_count,
                solve_context.maximum_maturity_count,
                Some(completed_interval_count),
                solve_context.maximum_maturity_count,
            ),
        );

        outputs.push(output);
    }

    Ok(outputs)
}

pub(in crate::spectral::pricer) fn solve_cached_interval_operators_in_parallel(
    jobs: &[CachedIntervalOperatorJob],
    solve_context: CachedIntervalOperatorSolveContext<'_>,
    observer: &mut Option<&mut dyn FnMut(PricingProgressEvent)>,
) -> Result<Vec<CachedIntervalOperatorSolveOutput>, VanillaOptionPricingRunError> {
    let (progress_sender, progress_receiver) = std::sync::mpsc::channel();
    let completed_interval_count = std::sync::atomic::AtomicUsize::new(0);

    std::thread::scope(|scope| {
        let solve_handle = scope.spawn(move || {
            install_pricing_parallelism(solve_context.pricing_thread_pool, || {
                jobs.par_iter()
                    .map(|job| {
                        let interval_solve_context = CachedIntervalOperatorSolveContext {
                            cpu_count: Some(1),
                            ..solve_context
                        };

                        let output = solve_cached_interval_operator(job, interval_solve_context)?;

                        let completed_interval_count = completed_interval_count
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                            .saturating_add(1);

                        let _ = progress_sender.send(PricingProgressEvent::interval_progress(
                            completed_interval_count,
                            solve_context.maximum_maturity_count,
                            Some(completed_interval_count),
                            solve_context.maximum_maturity_count,
                        ));

                        Ok(output)
                    })
                    .collect::<Result<Vec<_>, VanillaOptionPricingRunError>>()
            })
        });

        while let Ok(event) = progress_receiver.recv() {
            emit_pricing_progress(observer, event);
        }

        solve_handle.join().unwrap_or_else(|_| {
            Err(VanillaOptionPricingRunError::InternalNumericsFailure {
                message: "parallel spectral interval operator assembly panicked".to_string(),
            })
        })
    })
}

pub(in crate::spectral::pricer::interval_operator) struct IntervalTransitionContext {
    pub(in crate::spectral::pricer::interval_operator) base_context: LocalTransitionRequestContext,
    pub(in crate::spectral::pricer::interval_operator) current_anchor_value: f64,
    pub(in crate::spectral::pricer::interval_operator) next_anchor_value: f64,
    pub(in crate::spectral::pricer::interval_operator) interval_delta: f64,
    pub(in crate::spectral::pricer::interval_operator) expected_hawkes_state_before:
        janiform_model_kernels::hawkes::HawkesExcitationState,
    pub(in crate::spectral::pricer::interval_operator) expected_hawkes_state_after:
        janiform_model_kernels::hawkes::HawkesExcitationState,
}

pub(in crate::spectral::pricer) fn solve_cached_interval_operator(
    job: &CachedIntervalOperatorJob,
    solve_context: CachedIntervalOperatorSolveContext<'_>,
) -> Result<CachedIntervalOperatorSolveOutput, VanillaOptionPricingRunError> {
    let CachedIntervalOperatorSolveContext {
        include_node_price_sensitivities,
        spectral_domain,
        driver_parameters,
        collect_theta_warm_start_sensitivities,
        pricing_thread_pool,
        cpu_count,
        ..
    } = solve_context;

    let transition_context = interval_transition_context(job, solve_context)?;

    let collocation_nodes = spectral_domain.collocation_nodes();

    let row_states = collocation_nodes
        .iter()
        .copied()
        .map(|deviation| transition_context.base_context.build_row_state(deviation))
        .collect::<Result<Vec<_>, _>>()?;

    let (row_operators, row_theta_states) = solve_interval_row_frequency_batches_adaptively(
        IntervalRowFrequencyBatchAdaptiveSolveInput {
            base_context: &transition_context.base_context,
            collocation_nodes: collocation_nodes.as_slice(),
            row_states: row_states.as_slice(),
            theta_hints: job.theta_hints.as_slice(),
            next_anchor_value: transition_context.next_anchor_value,
            spectral_domain,
            driver_parameters,
            collect_theta_warm_start_sensitivities,
            pricing_thread_pool,
            cpu_count,
            progress_observer: None,
        },
    )?;

    let row_operator_sensitivities = if include_node_price_sensitivities {
        Some(build_row_operator_sensitivities(
            &transition_context.base_context,
            row_states.as_slice(),
            row_operators.as_slice(),
            spectral_domain,
            driver_parameters,
        )?)
    } else {
        None
    };

    Ok(CachedIntervalOperatorSolveOutput {
        interval_offset: job.interval_offset,
        operator: CachedIntervalOperator {
            interval_summary: build_interval_summary(
                job.interval_offset,
                solve_context,
                &transition_context,
                row_operators.as_slice(),
            ),
            row_operators,
            row_operator_sensitivities,
        },
        row_theta_states,
    })
}

pub(in crate::spectral::pricer::interval_operator) fn interval_transition_context(
    job: &CachedIntervalOperatorJob,
    solve_context: CachedIntervalOperatorSolveContext<'_>,
) -> Result<IntervalTransitionContext, VanillaOptionPricingRunError> {
    interval_transition_context_from_parts(
        solve_context.pricing_request,
        solve_context.materialization,
        job.interval_factor,
        job.interval_offset,
    )
}

pub(in crate::spectral::pricer::interval_operator) fn interval_transition_context_from_parts(
    pricing_request: &VanillaOptionPricingRequest,
    materialization: &RiskNeutralAnchorMaterialization,
    interval_factor: VanillaOptionPricingIntervalFactors,
    interval_offset: usize,
) -> Result<IntervalTransitionContext, VanillaOptionPricingRunError> {
    let runtime_anchors = &materialization.runtime_interval_anchors;

    let current_anchor_value = if interval_offset == 0 {
        runtime_anchors.start_anchor_value
    } else {
        runtime_anchors.end_anchor_values[interval_offset - 1]
    };

    let current_pressure_state = if interval_offset == 0 {
        runtime_anchors.start_pressure_state
    } else {
        runtime_anchors.end_pressure_states[interval_offset - 1]
    };

    let interval_delta = runtime_anchors.interval_deltas[interval_offset];

    let expected_hawkes_state_before = materialization.expected_hawkes_state_path[interval_offset];
    let expected_hawkes_state_after =
        materialization.expected_hawkes_state_path[interval_offset + 1];

    let next_anchor_value = runtime_anchors.end_anchor_values[interval_offset];

    let base_context = LocalTransitionRequestContext::from_interval_state(
        pricing_request,
        interval_factor,
        pricing_request.boundary_input.interval_index + interval_offset,
        expected_hawkes_state_before,
        current_anchor_value,
        current_pressure_state,
        interval_delta,
    )?;

    Ok(IntervalTransitionContext {
        base_context,
        current_anchor_value,
        next_anchor_value,
        interval_delta,
        expected_hawkes_state_before,
        expected_hawkes_state_after,
    })
}

pub(in crate::spectral::pricer::interval_operator) fn build_interval_summary(
    interval_offset: usize,
    solve_context: CachedIntervalOperatorSolveContext<'_>,
    transition_context: &IntervalTransitionContext,
    row_operators: &[RowFrequencyBatchSolution],
) -> SpectralPricingIntervalDiagnostics {
    SpectralPricingIntervalDiagnostics {
        interval_index: solve_context.pricing_request.boundary_input.interval_index
            + interval_offset,
        current_anchor_value: transition_context.current_anchor_value,
        next_anchor_value: transition_context.next_anchor_value,
        interval_delta: transition_context.interval_delta,
        expected_hawkes_boundary_state_before: transition_context.expected_hawkes_state_before,
        expected_hawkes_boundary_state_after: transition_context.expected_hawkes_state_after,
        minimum_solved_theta: row_operators
            .iter()
            .map(|row_operator| row_operator.theta)
            .fold(f64::INFINITY, f64::min),
        maximum_solved_theta: row_operators
            .iter()
            .map(|row_operator| row_operator.theta)
            .fold(f64::NEG_INFINITY, f64::max),
    }
}

pub(in crate::spectral::pricer) fn solve_maturity_law(
    pricing_request: &VanillaOptionPricingRequest,
    materialization: &RiskNeutralAnchorMaterialization,
    maturity_count: usize,
    spectral_domain: SpectralDomain,
    cached_interval_operators: &[CachedIntervalOperator],
) -> Result<SolvedMaturityLaw, VanillaOptionPricingRunError> {
    let maturity_interval = materialization.runtime_interval_anchors.interval_deltas
        [..maturity_count]
        .iter()
        .sum::<f64>();

    let basis_size = spectral_domain.node_count;
    let frequency_count = basis_size;

    let mut continuation_coefficients =
        vec![vec![ComplexScalar::new(0.0, 0.0); basis_size]; frequency_count];

    let mut continuation_coefficient_sensitivities = cached_interval_operators
        .first()
        .and_then(|interval_operator| interval_operator.row_operator_sensitivities.as_ref())
        .map(|_| {
            vec![
                vec![NodePriceSharedPlusShiftComplexSensitivity::zero(); basis_size];
                frequency_count
            ]
        });

    for continuation_coefficient_row in continuation_coefficients.iter_mut() {
        continuation_coefficient_row[0] = ComplexScalar::new(1.0, 0.0);
    }

    for cached_interval_operator in cached_interval_operators[..maturity_count].iter().rev() {
        let (updated_coefficients, updated_sensitivities) = apply_cached_interval_operator(
            continuation_coefficients,
            continuation_coefficient_sensitivities,
            cached_interval_operator,
            spectral_domain,
        );

        continuation_coefficients = updated_coefficients;
        continuation_coefficient_sensitivities = updated_sensitivities;
    }

    let characteristic_function_values = continuation_coefficients
        .iter()
        .enumerate()
        .map(|(frequency_index, coefficients)| {
            evaluate_cosine_series(
                coefficients.as_slice(),
                spectral_domain,
                pricing_request
                    .boundary_input
                    .observed_boundary_state
                    .deviation,
                frequency_index,
            )
        })
        .collect::<Vec<_>>();

    let characteristic_function_sensitivities =
        continuation_coefficient_sensitivities.map(|coefficient_sensitivities| {
            coefficient_sensitivities
                .iter()
                .enumerate()
                .map(|(frequency_index, sensitivities)| {
                    evaluate_cosine_series_sensitivity(
                        sensitivities.as_slice(),
                        spectral_domain,
                        pricing_request
                            .boundary_input
                            .observed_boundary_state
                            .deviation,
                        frequency_index,
                    )
                })
                .collect::<Vec<_>>()
        });

    Ok(SolvedMaturityLaw {
        maturity_interval,
        terminal_anchor_value: materialization.runtime_interval_anchors.end_anchor_values
            [maturity_count - 1],
        deviation_lower_bound: spectral_domain.deviation_lower_bound,
        deviation_upper_bound: spectral_domain.deviation_upper_bound,
        node_count: spectral_domain.node_count,
        observed_log_price: pricing_request
            .boundary_input
            .observed_boundary_state
            .observed_log_price,
        representative_transform: cached_interval_operators[..maturity_count]
            .iter()
            .flat_map(|interval_operator| interval_operator.row_operators.iter())
            .find_map(|row_operator| row_operator.representative_transform),
        characteristic_function_values,
        characteristic_function_sensitivities,
        interval_summaries: cached_interval_operators[..maturity_count]
            .iter()
            .map(|interval_operator| interval_operator.interval_summary.clone())
            .collect(),
    })
}

pub(in crate::spectral::pricer) fn apply_cached_interval_operator(
    continuation_coefficients: Vec<Vec<ComplexScalar>>,
    continuation_coefficient_sensitivities: Option<
        Vec<Vec<NodePriceSharedPlusShiftComplexSensitivity>>,
    >,
    cached_interval_operator: &CachedIntervalOperator,
    spectral_domain: SpectralDomain,
) -> (
    Vec<Vec<ComplexScalar>>,
    Option<Vec<Vec<NodePriceSharedPlusShiftComplexSensitivity>>>,
) {
    let frequency_count = spectral_domain.node_count;

    let mut interval_values =
        vec![vec![ComplexScalar::new(0.0, 0.0); spectral_domain.node_count]; frequency_count];

    let mut interval_value_sensitivities = continuation_coefficient_sensitivities
        .as_ref()
        .map(|_| {
            vec![
                vec![NodePriceSharedPlusShiftComplexSensitivity::zero(); spectral_domain.node_count];
                frequency_count
            ]
        });

    for (row_index, row_operator) in cached_interval_operator.row_operators.iter().enumerate() {
        let row_operator_sensitivity = cached_interval_operator
            .row_operator_sensitivities
            .as_ref()
            .and_then(|row_operator_sensitivities| row_operator_sensitivities.get(row_index));

        for frequency_index in 0..frequency_count {
            let mut row_value = ComplexScalar::new(0.0, 0.0);
            let mut row_value_sensitivity = NodePriceSharedPlusShiftComplexSensitivity::zero();

            for basis_index in 0..spectral_domain.node_count {
                let difference_value = if frequency_index >= basis_index {
                    row_operator.positive_frequency_values[frequency_index - basis_index]
                } else {
                    row_operator.positive_frequency_values[basis_index - frequency_index]
                        .conjugate()
                };

                let expectation = complex_multiply(
                    row_operator.basis_forward_phases[basis_index],
                    row_operator.positive_frequency_values[frequency_index + basis_index],
                )
                .plus(complex_multiply(
                    row_operator.basis_backward_phases[basis_index],
                    difference_value,
                ))
                .scale(0.5);

                let expectation_sensitivity = row_operator_sensitivity.map_or(
                    NodePriceSharedPlusShiftComplexSensitivity::zero(),
                    |row_operator_sensitivity| {
                        let positive_frequency_sensitivity = row_operator_sensitivity
                            .positive_frequency_value_sensitivities[frequency_index + basis_index];

                        let difference_sensitivity = if frequency_index >= basis_index {
                            row_operator_sensitivity.positive_frequency_value_sensitivities
                                [frequency_index - basis_index]
                        } else {
                            row_operator_sensitivity.positive_frequency_value_sensitivities
                                [basis_index - frequency_index]
                                .conjugate()
                        };

                        complex_expectation_sensitivity(ComplexExpectationSensitivityInput {
                            basis_forward_phase: row_operator.basis_forward_phases[basis_index],
                            basis_forward_phase_sensitivity: row_operator_sensitivity
                                .basis_forward_phase_sensitivities[basis_index],
                            forward_value: row_operator.positive_frequency_values
                                [frequency_index + basis_index],
                            forward_value_sensitivity: positive_frequency_sensitivity,
                            basis_backward_phase: row_operator.basis_backward_phases[basis_index],
                            basis_backward_phase_sensitivity: row_operator_sensitivity
                                .basis_backward_phase_sensitivities[basis_index],
                            backward_value: difference_value,
                            backward_value_sensitivity: difference_sensitivity,
                        })
                    },
                );

                let continuation_sensitivity = continuation_coefficient_sensitivities
                    .as_ref()
                    .map(|coefficient_sensitivities| {
                        coefficient_sensitivities[frequency_index][basis_index]
                    })
                    .unwrap_or_else(NodePriceSharedPlusShiftComplexSensitivity::zero);

                row_value = row_value.plus(complex_multiply(
                    continuation_coefficients[frequency_index][basis_index],
                    expectation,
                ));

                row_value_sensitivity = row_value_sensitivity
                    .plus(complex_scale_sensitivity(
                        expectation,
                        continuation_sensitivity,
                    ))
                    .plus(complex_scale_sensitivity(
                        continuation_coefficients[frequency_index][basis_index],
                        expectation_sensitivity,
                    ));
            }

            interval_values[frequency_index][row_index] = complex_multiply(
                row_operator.row_frequency_phases[frequency_index],
                row_value,
            );

            if let Some(interval_value_sensitivities) = interval_value_sensitivities.as_mut() {
                interval_value_sensitivities[frequency_index][row_index] =
                    complex_scale_sensitivity(
                        row_operator.row_frequency_phases[frequency_index],
                        row_value_sensitivity,
                    );
            }
        }
    }

    let coefficients = values_to_cosine_coefficients(interval_values.as_slice());

    let coefficient_sensitivities = interval_value_sensitivities
        .as_ref()
        .map(|values| values_to_cosine_coefficients_sensitivities(values.as_slice()));

    (coefficients, coefficient_sensitivities)
}
