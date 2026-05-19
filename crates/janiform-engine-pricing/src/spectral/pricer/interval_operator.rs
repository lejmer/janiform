use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

use super::domain::SpectralDomain;
use super::math::{
    ComplexExpectationSensitivityInput, complex_expectation_sensitivity, complex_scale_sensitivity,
    evaluate_cosine_series, evaluate_cosine_series_sensitivity,
};
use super::row_frequency::{
    IntervalRowFrequencyBatchAdaptiveSolveInput, RowFrequencyBatchSolution,
    RowFrequencyBatchSolutionSharedPlusShiftSensitivity, build_row_operator_sensitivities,
    solve_interval_row_frequency_batches_adaptively, theta_warm_start_driver_configuration,
};
use super::{
    CarryGroupKey, ComplexScalar, LocalTransitionRequestContext,
    NodePriceSharedPlusShiftComplexSensitivity, PricingProgressEvent,
    RiskNeutralAnchorMaterialization, SpectralPricingIntervalDiagnostics,
    SpectralThetaWarmStartRowState, SpectralThetaWarmStartState,
    VanillaOptionPricingIntervalFactors, VanillaOptionPricingRequest, VanillaOptionPricingRunError,
    complex_multiply, emit_pricing_progress, install_pricing_parallelism,
    resolved_pricing_cpu_count, values_to_cosine_coefficients,
    values_to_cosine_coefficients_sensitivities,
};

#[derive(Debug)]
pub(super) struct CachedIntervalOperator {
    pub(super) interval_summary: SpectralPricingIntervalDiagnostics,
    pub(super) row_operators: Vec<RowFrequencyBatchSolution>,
    pub(super) row_operator_sensitivities:
        Option<Vec<RowFrequencyBatchSolutionSharedPlusShiftSensitivity>>,
}

#[derive(Debug, Clone)]
pub(super) struct SolvedMaturityLaw {
    pub(super) maturity_interval: f64,
    pub(super) terminal_anchor_value: f64,
    pub(super) deviation_lower_bound: f64,
    pub(super) deviation_upper_bound: f64,
    pub(super) node_count: usize,
    pub(super) observed_log_price: f64,
    pub(super) representative_transform:
        Option<janiform_affine_oracle::affine_oracle::ExponentialAffineTransformIntegrationResult>,
    pub(super) characteristic_function_values: Vec<ComplexScalar>,
    pub(super) characteristic_function_sensitivities:
        Option<Vec<NodePriceSharedPlusShiftComplexSensitivity>>,
    pub(super) interval_summaries: Vec<SpectralPricingIntervalDiagnostics>,
}

pub(super) struct CachedIntervalOperatorBuildInput<'a, 'observer, 'callback> {
    pub(super) carry_group_key: CarryGroupKey,
    pub(super) pricing_request: &'a VanillaOptionPricingRequest,
    pub(super) include_node_price_sensitivities: bool,
    pub(super) interval_factors: &'a [VanillaOptionPricingIntervalFactors],
    pub(super) materialization: &'a RiskNeutralAnchorMaterialization,
    pub(super) maximum_maturity_count: usize,
    pub(super) spectral_domain: SpectralDomain,
    pub(super) driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
    pub(super) pricing_thread_pool: Option<&'a rayon::ThreadPool>,
    pub(super) cpu_count: Option<usize>,
    pub(super) observer: &'observer mut Option<&'callback mut dyn FnMut(PricingProgressEvent)>,
    pub(super) theta_warm_start_state: Option<&'a mut SpectralThetaWarmStartState>,
}

#[derive(Debug)]
pub(super) struct CachedIntervalOperatorJob {
    pub(super) interval_offset: usize,
    pub(super) interval_factor: VanillaOptionPricingIntervalFactors,
    pub(super) theta_hints: Vec<Option<f64>>,
}

#[derive(Debug)]
pub(super) struct CachedIntervalOperatorSolveOutput {
    pub(super) interval_offset: usize,
    pub(super) operator: CachedIntervalOperator,
    pub(super) row_theta_states: Vec<SpectralThetaWarmStartRowState>,
}

#[derive(Clone, Copy)]
pub(super) struct CachedIntervalOperatorSolveContext<'a> {
    pub(super) pricing_request: &'a VanillaOptionPricingRequest,
    pub(super) include_node_price_sensitivities: bool,
    pub(super) materialization: &'a RiskNeutralAnchorMaterialization,
    pub(super) maximum_maturity_count: usize,
    pub(super) spectral_domain: SpectralDomain,
    pub(super) driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
    pub(super) collect_theta_warm_start_sensitivities: bool,
    pub(super) pricing_thread_pool: Option<&'a rayon::ThreadPool>,
    pub(super) cpu_count: Option<usize>,
}

pub(super) fn build_cached_interval_operators(
    input: CachedIntervalOperatorBuildInput<'_, '_, '_>,
) -> Result<Vec<CachedIntervalOperator>, VanillaOptionPricingRunError> {
    let CachedIntervalOperatorBuildInput {
        carry_group_key,
        pricing_request,
        include_node_price_sensitivities,
        interval_factors,
        materialization,
        maximum_maturity_count,
        spectral_domain,
        driver_parameters,
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
                        (
                            carry_group_key.risk_free_rate_bits,
                            carry_group_key.dividend_yield_bits,
                        ),
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
                (
                    carry_group_key.risk_free_rate_bits,
                    carry_group_key.dividend_yield_bits,
                ),
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

    Ok(cached_interval_operators)
}

pub(super) fn solve_cached_interval_operators_serially(
    jobs: &[CachedIntervalOperatorJob],
    solve_context: CachedIntervalOperatorSolveContext<'_>,
    observer: &mut Option<&mut dyn FnMut(PricingProgressEvent)>,
) -> Result<Vec<CachedIntervalOperatorSolveOutput>, VanillaOptionPricingRunError> {
    let mut outputs = Vec::with_capacity(jobs.len());

    for job in jobs {
        emit_pricing_progress(
            observer,
            PricingProgressEvent::interval_progress(
                job.interval_offset,
                solve_context.maximum_maturity_count,
                Some(job.interval_offset),
                solve_context.maximum_maturity_count,
            ),
        );

        let output = solve_cached_interval_operator(job, solve_context)?;

        outputs.push(output);
    }

    Ok(outputs)
}

pub(super) fn solve_cached_interval_operators_in_parallel(
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
                            Some(job.interval_offset),
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

pub(super) fn solve_cached_interval_operator(
    job: &CachedIntervalOperatorJob,
    solve_context: CachedIntervalOperatorSolveContext<'_>,
) -> Result<CachedIntervalOperatorSolveOutput, VanillaOptionPricingRunError> {
    let CachedIntervalOperatorSolveContext {
        pricing_request,
        include_node_price_sensitivities,
        materialization,
        spectral_domain,
        driver_parameters,
        collect_theta_warm_start_sensitivities,
        pricing_thread_pool,
        cpu_count,
        ..
    } = solve_context;

    let runtime_anchors = &materialization.runtime_interval_anchors;
    let current_anchor_value = if job.interval_offset == 0 {
        runtime_anchors.start_anchor_value
    } else {
        runtime_anchors.end_anchor_values[job.interval_offset - 1]
    };

    let current_pressure_state = if job.interval_offset == 0 {
        runtime_anchors.start_pressure_state
    } else {
        runtime_anchors.end_pressure_states[job.interval_offset - 1]
    };

    let interval_delta = runtime_anchors.interval_deltas[job.interval_offset];

    let expected_hawkes_state_before =
        materialization.expected_hawkes_state_path[job.interval_offset];
    let expected_hawkes_state_after =
        materialization.expected_hawkes_state_path[job.interval_offset + 1];

    let next_anchor_value = runtime_anchors.end_anchor_values[job.interval_offset];

    let base_context = LocalTransitionRequestContext::from_interval_state(
        pricing_request,
        job.interval_factor,
        pricing_request.boundary_input.interval_index + job.interval_offset,
        expected_hawkes_state_before,
        current_anchor_value,
        current_pressure_state,
        interval_delta,
    )?;

    let collocation_nodes = spectral_domain.collocation_nodes();

    let row_states = collocation_nodes
        .iter()
        .copied()
        .map(|deviation| base_context.build_row_state(deviation))
        .collect::<Result<Vec<_>, _>>()?;

    let (row_operators, row_theta_states) = solve_interval_row_frequency_batches_adaptively(
        IntervalRowFrequencyBatchAdaptiveSolveInput {
            base_context: &base_context,
            collocation_nodes: collocation_nodes.as_slice(),
            row_states: row_states.as_slice(),
            theta_hints: job.theta_hints.as_slice(),
            next_anchor_value,
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
            &base_context,
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
            interval_summary: SpectralPricingIntervalDiagnostics {
                interval_index: pricing_request.boundary_input.interval_index + job.interval_offset,
                current_anchor_value,
                next_anchor_value,
                interval_delta,
                expected_hawkes_boundary_state_before: expected_hawkes_state_before,
                expected_hawkes_boundary_state_after: expected_hawkes_state_after,
                minimum_solved_theta: row_operators
                    .iter()
                    .map(|row_operator| row_operator.theta)
                    .fold(f64::INFINITY, f64::min),
                maximum_solved_theta: row_operators
                    .iter()
                    .map(|row_operator| row_operator.theta)
                    .fold(f64::NEG_INFINITY, f64::max),
            },
            row_operators,
            row_operator_sensitivities,
        },
        row_theta_states,
    })
}

pub(super) fn solve_maturity_law(
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

pub(super) fn apply_cached_interval_operator(
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
