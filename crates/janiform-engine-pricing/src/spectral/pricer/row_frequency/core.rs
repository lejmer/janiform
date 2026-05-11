use super::super::domain::SpectralDomain;
use super::super::{
    ComplexScalar, CoreLocalTiltImplicitThetaSharedPlusShiftSensitivity,
    CorePureImaginaryLogMomentSharedPlusShiftSensitivity, LocalTransitionRequestContext,
    LocalTransitionRowState, NodePriceSharedPlusShiftComplexSensitivity,
    NodePriceSharedPlusShiftSensitivity, PricingCharacteristicFunctionSharedPlusShiftSensitivity,
    SpectralThetaWarmStartDriverConfiguration, SpectralThetaWarmStartRowState,
    SpectralThetaWarmStartSensitivity, VanillaOptionPricingRunError,
    certify_local_transition_theta_for_row_with_parallelism, complex_exponential, complex_multiply,
    differentiate_core_local_tilt_theta_for_tilt,
    differentiate_core_pure_imaginary_log_moment_exponent_for_tilt,
    evaluate_pricing_log_return_characteristic_function_batch_for_context,
    evaluate_pricing_log_return_characteristic_function_batch_shared_plus_shift_sensitivities_for_context,
    pricing_ratio_derivative, resolve_local_transition_theta_for_row_with_parallelism_and_hint,
};

#[derive(Debug, Clone)]
pub(in crate::spectral::pricer) struct RowFrequencyBatchSolution {
    pub(in crate::spectral::pricer) theta: f64,
    pub(in crate::spectral::pricer) positive_frequency_values: Vec<ComplexScalar>,
    pub(in crate::spectral::pricer) basis_forward_phases: Vec<ComplexScalar>,
    pub(in crate::spectral::pricer) basis_backward_phases: Vec<ComplexScalar>,
    pub(in crate::spectral::pricer) row_frequency_phases: Vec<ComplexScalar>,
    pub(in crate::spectral::pricer) representative_transform:
        Option<janiform_affine_oracle::affine_oracle::ExponentialAffineTransformIntegrationResult>,
}

#[derive(Debug, Clone)]
pub(in crate::spectral::pricer) struct SharedRowFrequencyBatchKernelSensitivity {
    pub(in crate::spectral::pricer) positive_frequency_value_sensitivities:
        Vec<NodePriceSharedPlusShiftComplexSensitivity>,
    pub(in crate::spectral::pricer) local_expectation_sensitivity:
        NodePriceSharedPlusShiftSensitivity,
}

#[derive(Debug, Clone)]
pub(in crate::spectral::pricer) struct RowFrequencyBatchSolutionSharedPlusShiftSensitivity {
    pub(in crate::spectral::pricer) positive_frequency_value_sensitivities:
        Vec<NodePriceSharedPlusShiftComplexSensitivity>,
    pub(in crate::spectral::pricer) basis_forward_phase_sensitivities:
        Vec<NodePriceSharedPlusShiftComplexSensitivity>,
    pub(in crate::spectral::pricer) basis_backward_phase_sensitivities:
        Vec<NodePriceSharedPlusShiftComplexSensitivity>,
}

#[derive(Debug, Clone)]
pub(in crate::spectral::pricer) struct SharedRowFrequencyBatchKernel {
    pub(in crate::spectral::pricer) theta: f64,
    pub(in crate::spectral::pricer) positive_frequency_values: Vec<ComplexScalar>,
    pub(in crate::spectral::pricer) representative_transform:
        Option<janiform_affine_oracle::affine_oracle::ExponentialAffineTransformIntegrationResult>,
    pub(in crate::spectral::pricer) local_expectations: f64,
}

#[derive(Debug, Clone)]
pub(in crate::spectral::pricer) struct TimedRowFrequencyBatchSolution {
    pub(in crate::spectral::pricer) solution: RowFrequencyBatchSolution,
    pub(in crate::spectral::pricer) theta_warm_start_state: SpectralThetaWarmStartRowState,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::spectral::pricer) struct AdaptiveRowSolveTask {
    pub(in crate::spectral::pricer) left_row_index: usize,
    pub(in crate::spectral::pricer) midpoint_row_index: usize,
    pub(in crate::spectral::pricer) right_row_index: usize,
    pub(in crate::spectral::pricer) predicted_theta: f64,
}

pub(in crate::spectral::pricer) fn theta_warm_start_driver_configuration(
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
) -> SpectralThetaWarmStartDriverConfiguration {
    SpectralThetaWarmStartDriverConfiguration {
        alpha: driver_parameters.buy_side_parameters.alpha,
        beta_0: driver_parameters.buy_side_parameters.beta,
        delta: driver_parameters.buy_side_parameters.delta,
        mu: driver_parameters.buy_side_parameters.mu,
        delta_beta: driver_parameters.sell_side_parameters.beta
            - driver_parameters.buy_side_parameters.beta,
    }
}

pub(in crate::spectral::pricer) fn try_resolve_theta_warm_start_sensitivity(
    sensitivity: &CorePureImaginaryLogMomentSharedPlusShiftSensitivity,
) -> Option<SpectralThetaWarmStartSensitivity> {
    // The pricing result is already determined by the exact theta solve. If the
    // derivative-based predictor cannot be evaluated cleanly, preserve the
    // canonical solved theta and skip only the advisory warm-start sensitivity.
    if !sensitivity.theta_derivative.is_finite() || sensitivity.theta_derivative == 0.0 {
        return None;
    }

    let inverse_theta_derivative = -1.0 / sensitivity.theta_derivative;

    Some(SpectralThetaWarmStartSensitivity {
        alpha_derivative: sensitivity.alpha_derivative * inverse_theta_derivative,
        beta_0_derivative: sensitivity.beta_0_derivative * inverse_theta_derivative,
        delta_derivative: sensitivity.delta_derivative * inverse_theta_derivative,
        mu_derivative: sensitivity.mu_derivative * inverse_theta_derivative,
        delta_beta_derivative: sensitivity.delta_beta_derivative * inverse_theta_derivative,
    })
}

pub(in crate::spectral::pricer) fn solve_row_frequency_batch(
    base_context: &LocalTransitionRequestContext,
    transition_row: &LocalTransitionRowState,
    spectral_domain: SpectralDomain,
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
    theta: f64,
) -> Result<RowFrequencyBatchSolution, VanillaOptionPricingRunError> {
    let shared_kernel = solve_shared_row_frequency_batch_kernel(
        base_context,
        spectral_domain,
        driver_parameters,
        theta,
    )?;

    build_row_frequency_batch_from_shared_kernel(
        base_context,
        transition_row,
        spectral_domain,
        &shared_kernel,
    )
}

pub(in crate::spectral::pricer) fn solve_shared_row_frequency_batch_kernel(
    base_context: &LocalTransitionRequestContext,
    spectral_domain: SpectralDomain,
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
    theta: f64,
) -> Result<SharedRowFrequencyBatchKernel, VanillaOptionPricingRunError> {
    let frequency_count = spectral_domain.node_count;

    let mut batch_arguments = Vec::with_capacity(2 * frequency_count - 1);

    for frequency_index in 0..(2 * frequency_count - 1) {
        batch_arguments.push(ComplexScalar::new(
            spectral_domain.basis_frequency(frequency_index),
            -theta,
        ));
    }

    let batch_evaluations = evaluate_pricing_log_return_characteristic_function_batch_for_context(
        base_context,
        driver_parameters,
        batch_arguments.as_slice(),
    )?;

    let denominator = batch_evaluations[0].value;
    let representative_transform = Some(batch_evaluations[0].transform_diagnostics);

    let mut positive_frequency_values = Vec::with_capacity(batch_evaluations.len());

    for evaluation in &batch_evaluations {
        let value = evaluation.value.divide(denominator).ok_or(
            VanillaOptionPricingRunError::InvalidRiskNeutralCharacteristicFunction {
                numerator: evaluation.value,
                denominator,
            },
        )?;

        positive_frequency_values.push(value);
    }

    let local_expectations = derive_expected_increment_from_frequency_values(
        positive_frequency_values.as_slice(),
        spectral_domain,
    )?;

    Ok(SharedRowFrequencyBatchKernel {
        theta,
        positive_frequency_values,
        representative_transform,
        local_expectations,
    })
}

pub(in crate::spectral::pricer) fn build_row_frequency_batch_from_shared_kernel(
    base_context: &LocalTransitionRequestContext,
    transition_row: &LocalTransitionRowState,
    spectral_domain: SpectralDomain,
    shared_kernel: &SharedRowFrequencyBatchKernel,
) -> Result<RowFrequencyBatchSolution, VanillaOptionPricingRunError> {
    let frequency_count = spectral_domain.node_count;
    let deviation = transition_row.boundary_state.deviation;
    let shift_center =
        base_context.deviation_decay_factor * deviation - shared_kernel.local_expectations;
    let shift_phase_offset = shift_center - spectral_domain.deviation_lower_bound;

    let mut basis_forward_phases = Vec::with_capacity(spectral_domain.node_count);
    let mut basis_backward_phases = Vec::with_capacity(spectral_domain.node_count);

    for basis_index in 0..spectral_domain.node_count {
        let forward_phase = complex_exponential(ComplexScalar::new(
            0.0,
            spectral_domain.basis_frequency(basis_index) * shift_phase_offset,
        ));

        basis_forward_phases.push(forward_phase);
        basis_backward_phases.push(forward_phase.conjugate());
    }

    let mut row_frequency_phases = Vec::with_capacity(frequency_count);

    for frequency_index in 0..frequency_count {
        row_frequency_phases.push(complex_exponential(ComplexScalar::new(
            0.0,
            spectral_domain.basis_frequency(frequency_index)
                * (base_context.deviation_decay_factor - 1.0)
                * deviation,
        )));
    }

    Ok(RowFrequencyBatchSolution {
        theta: shared_kernel.theta,
        positive_frequency_values: shared_kernel.positive_frequency_values.clone(),
        basis_forward_phases,
        basis_backward_phases,
        row_frequency_phases,
        representative_transform: shared_kernel.representative_transform,
    })
}

pub(in crate::spectral::pricer) fn build_row_operator_sensitivities(
    base_context: &LocalTransitionRequestContext,
    row_states: &[LocalTransitionRowState],
    row_operators: &[RowFrequencyBatchSolution],
    spectral_domain: SpectralDomain,
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
) -> Result<Vec<RowFrequencyBatchSolutionSharedPlusShiftSensitivity>, VanillaOptionPricingRunError>
{
    if row_states.len() != row_operators.len() {
        return Err(VanillaOptionPricingRunError::InternalNumericsFailure {
            message: "row operator sensitivity build requires aligned row states and operators"
                .to_string(),
        });
    }

    if row_states.is_empty() {
        return Ok(Vec::new());
    }

    if row_states
        .iter()
        .all(LocalTransitionRowState::uses_nested_transition_measure)
    {
        let shared_theta = row_operators[0].theta;

        let shared_kernel_sensitivity = build_shared_row_frequency_batch_kernel_sensitivity(
            base_context,
            spectral_domain,
            driver_parameters,
            shared_theta,
            NodePriceSharedPlusShiftSensitivity::zero(),
        )?;

        return row_states
            .iter()
            .zip(row_operators.iter())
            .map(|(row_state, row_operator)| {
                build_row_frequency_batch_sensitivity_from_shared_kernel(
                    row_state,
                    row_operator,
                    spectral_domain,
                    &shared_kernel_sensitivity,
                )
            })
            .collect();
    }

    row_states
        .iter()
        .zip(row_operators.iter())
        .map(|(row_state, row_operator)| {
            let theta_sensitivity = node_price_shared_plus_shift_theta_sensitivity(
                differentiate_core_local_tilt_theta_for_tilt(
                    base_context,
                    driver_parameters,
                    row_operator.theta,
                )?,
            );

            let shared_kernel_sensitivity = build_shared_row_frequency_batch_kernel_sensitivity(
                base_context,
                spectral_domain,
                driver_parameters,
                row_operator.theta,
                theta_sensitivity,
            )?;

            build_row_frequency_batch_sensitivity_from_shared_kernel(
                row_state,
                row_operator,
                spectral_domain,
                &shared_kernel_sensitivity,
            )
        })
        .collect()
}

pub(in crate::spectral::pricer) fn node_price_shared_plus_shift_theta_sensitivity(
    theta_sensitivity: CoreLocalTiltImplicitThetaSharedPlusShiftSensitivity,
) -> NodePriceSharedPlusShiftSensitivity {
    NodePriceSharedPlusShiftSensitivity {
        alpha_derivative: theta_sensitivity.alpha_derivative,
        beta_0_derivative: theta_sensitivity.beta_0_derivative,
        delta_derivative: theta_sensitivity.delta_derivative,
        delta_beta_derivative: theta_sensitivity.delta_beta_derivative,
    }
}

pub(in crate::spectral::pricer) fn node_price_shared_plus_shift_sensitivity_from_characteristic(
    sensitivity: PricingCharacteristicFunctionSharedPlusShiftSensitivity,
    theta_sensitivity: NodePriceSharedPlusShiftSensitivity,
) -> NodePriceSharedPlusShiftComplexSensitivity {
    NodePriceSharedPlusShiftComplexSensitivity {
        alpha_derivative: sensitivity.alpha_derivative.plus(
            sensitivity
                .theta_derivative
                .scale(theta_sensitivity.alpha_derivative),
        ),
        beta_0_derivative: sensitivity.beta_0_derivative.plus(
            sensitivity
                .theta_derivative
                .scale(theta_sensitivity.beta_0_derivative),
        ),
        delta_derivative: sensitivity.delta_derivative.plus(
            sensitivity
                .theta_derivative
                .scale(theta_sensitivity.delta_derivative),
        ),
        delta_beta_derivative: sensitivity.delta_beta_derivative.plus(
            sensitivity
                .theta_derivative
                .scale(theta_sensitivity.delta_beta_derivative),
        ),
    }
}

pub(in crate::spectral::pricer) fn build_shared_row_frequency_batch_kernel_sensitivity(
    base_context: &LocalTransitionRequestContext,
    spectral_domain: SpectralDomain,
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
    theta: f64,
    theta_sensitivity: NodePriceSharedPlusShiftSensitivity,
) -> Result<SharedRowFrequencyBatchKernelSensitivity, VanillaOptionPricingRunError> {
    let frequency_count = spectral_domain.node_count;

    let mut batch_arguments = Vec::with_capacity(2 * frequency_count - 1);

    for frequency_index in 0..(2 * frequency_count - 1) {
        batch_arguments.push(ComplexScalar::new(
            spectral_domain.basis_frequency(frequency_index),
            -theta,
        ));
    }

    let batch_sensitivities =
        evaluate_pricing_log_return_characteristic_function_batch_shared_plus_shift_sensitivities_for_context(
            base_context,
            driver_parameters,
            batch_arguments.as_slice(),
        )?;

    let denominator_sensitivity = batch_sensitivities[0];
    let denominator = denominator_sensitivity.value;

    let positive_frequency_value_sensitivities = batch_sensitivities
        .into_iter()
        .map(|sensitivity| {
            Ok(node_price_shared_plus_shift_sensitivity_from_characteristic(
                PricingCharacteristicFunctionSharedPlusShiftSensitivity {
                    value: sensitivity
                        .value
                        .divide(denominator)
                        .ok_or(
                            VanillaOptionPricingRunError::InvalidRiskNeutralCharacteristicFunction {
                                numerator: sensitivity.value,
                                denominator,
                            },
                        )?,
                    alpha_derivative: pricing_ratio_derivative(
                        sensitivity.value,
                        sensitivity
                            .alpha_derivative
                            .plus(sensitivity.theta_derivative.scale(theta_sensitivity.alpha_derivative)),
                        denominator_sensitivity.value,
                        denominator_sensitivity.alpha_derivative.plus(
                            denominator_sensitivity
                                .theta_derivative
                                .scale(theta_sensitivity.alpha_derivative),
                        ),
                    )?,
                    beta_0_derivative: pricing_ratio_derivative(
                        sensitivity.value,
                        sensitivity.beta_0_derivative.plus(
                            sensitivity.theta_derivative.scale(theta_sensitivity.beta_0_derivative),
                        ),
                        denominator_sensitivity.value,
                        denominator_sensitivity.beta_0_derivative.plus(
                            denominator_sensitivity
                                .theta_derivative
                                .scale(theta_sensitivity.beta_0_derivative),
                        ),
                    )?,
                    delta_derivative: pricing_ratio_derivative(
                        sensitivity.value,
                        sensitivity
                            .delta_derivative
                            .plus(sensitivity.theta_derivative.scale(theta_sensitivity.delta_derivative)),
                        denominator_sensitivity.value,
                        denominator_sensitivity.delta_derivative.plus(
                            denominator_sensitivity
                                .theta_derivative
                                .scale(theta_sensitivity.delta_derivative),
                        ),
                    )?,
                    mu_derivative: ComplexScalar::new(0.0, 0.0),
                    delta_beta_derivative: pricing_ratio_derivative(
                        sensitivity.value,
                        sensitivity.delta_beta_derivative.plus(
                            sensitivity
                                .theta_derivative
                                .scale(theta_sensitivity.delta_beta_derivative),
                        ),
                        denominator_sensitivity.value,
                        denominator_sensitivity.delta_beta_derivative.plus(
                            denominator_sensitivity
                                .theta_derivative
                                .scale(theta_sensitivity.delta_beta_derivative),
                        ),
                    )?,
                    theta_derivative: ComplexScalar::new(0.0, 0.0),
                },
                NodePriceSharedPlusShiftSensitivity::zero(),
            ))
        })
        .collect::<Result<Vec<_>, VanillaOptionPricingRunError>>()?;

    let local_expectation_sensitivity =
        derive_expected_increment_sensitivity_from_frequency_values(
            positive_frequency_value_sensitivities.as_slice(),
            spectral_domain,
        )?;

    Ok(SharedRowFrequencyBatchKernelSensitivity {
        positive_frequency_value_sensitivities,
        local_expectation_sensitivity,
    })
}

pub(in crate::spectral::pricer) fn derive_expected_increment_sensitivity_from_frequency_values(
    positive_frequency_value_sensitivities: &[NodePriceSharedPlusShiftComplexSensitivity],
    spectral_domain: SpectralDomain,
) -> Result<NodePriceSharedPlusShiftSensitivity, VanillaOptionPricingRunError> {
    let first_frequency = spectral_domain.basis_frequency(1);

    let first_sensitivity = positive_frequency_value_sensitivities
        .get(1)
        .copied()
        .ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
            message:
                "spectral expected-increment sensitivity recovery requires at least two frequencies"
                    .to_string(),
        })?;

    Ok(NodePriceSharedPlusShiftSensitivity {
        alpha_derivative: first_sensitivity.alpha_derivative.imaginary / first_frequency,
        beta_0_derivative: first_sensitivity.beta_0_derivative.imaginary / first_frequency,
        delta_derivative: first_sensitivity.delta_derivative.imaginary / first_frequency,
        delta_beta_derivative: first_sensitivity.delta_beta_derivative.imaginary / first_frequency,
    })
}

pub(in crate::spectral::pricer) fn build_row_frequency_batch_sensitivity_from_shared_kernel(
    _transition_row: &LocalTransitionRowState,
    row_operator: &RowFrequencyBatchSolution,
    spectral_domain: SpectralDomain,
    shared_kernel_sensitivity: &SharedRowFrequencyBatchKernelSensitivity,
) -> Result<RowFrequencyBatchSolutionSharedPlusShiftSensitivity, VanillaOptionPricingRunError> {
    let basis_forward_phase_sensitivities = row_operator
        .basis_forward_phases
        .iter()
        .copied()
        .enumerate()
        .map(|(basis_index, phase)| {
            basis_phase_sensitivity(
                phase,
                spectral_domain.basis_frequency(basis_index),
                shared_kernel_sensitivity.local_expectation_sensitivity,
            )
        })
        .collect::<Vec<_>>();

    let basis_backward_phase_sensitivities = basis_forward_phase_sensitivities
        .iter()
        .copied()
        .map(NodePriceSharedPlusShiftComplexSensitivity::conjugate)
        .collect::<Vec<_>>();

    Ok(RowFrequencyBatchSolutionSharedPlusShiftSensitivity {
        positive_frequency_value_sensitivities: shared_kernel_sensitivity
            .positive_frequency_value_sensitivities
            .clone(),
        basis_forward_phase_sensitivities,
        basis_backward_phase_sensitivities,
    })
}

pub(in crate::spectral::pricer) fn basis_phase_sensitivity(
    phase: ComplexScalar,
    basis_frequency: f64,
    local_expectation_sensitivity: NodePriceSharedPlusShiftSensitivity,
) -> NodePriceSharedPlusShiftComplexSensitivity {
    let scaling_factor = ComplexScalar::new(0.0, -basis_frequency);

    NodePriceSharedPlusShiftComplexSensitivity {
        alpha_derivative: complex_multiply(
            phase,
            scaling_factor.scale(local_expectation_sensitivity.alpha_derivative),
        ),
        beta_0_derivative: complex_multiply(
            phase,
            scaling_factor.scale(local_expectation_sensitivity.beta_0_derivative),
        ),
        delta_derivative: complex_multiply(
            phase,
            scaling_factor.scale(local_expectation_sensitivity.delta_derivative),
        ),
        delta_beta_derivative: complex_multiply(
            phase,
            scaling_factor.scale(local_expectation_sensitivity.delta_beta_derivative),
        ),
    }
}

#[derive(Clone, Copy)]
pub(in crate::spectral::pricer) struct ExactRowFrequencyBatchSolveInput<'a> {
    pub(in crate::spectral::pricer) base_context: &'a LocalTransitionRequestContext,
    pub(in crate::spectral::pricer) transition_row: &'a LocalTransitionRowState,
    pub(in crate::spectral::pricer) initial_theta_hint: Option<f64>,
    pub(in crate::spectral::pricer) spectral_domain: SpectralDomain,
    pub(in crate::spectral::pricer) driver_parameters:
        janiform_model_kernels::nig::NigBuySellDriverParameters,
    pub(in crate::spectral::pricer) collect_theta_warm_start_sensitivities: bool,
    pub(in crate::spectral::pricer) pricing_thread_pool: Option<&'a rayon::ThreadPool>,
    pub(in crate::spectral::pricer) cpu_count: Option<usize>,
}

pub(in crate::spectral::pricer) fn solve_exact_row_frequency_batch(
    input: ExactRowFrequencyBatchSolveInput<'_>,
) -> Result<TimedRowFrequencyBatchSolution, VanillaOptionPricingRunError> {
    let ExactRowFrequencyBatchSolveInput {
        base_context,
        transition_row,
        initial_theta_hint,
        spectral_domain,
        driver_parameters,
        collect_theta_warm_start_sensitivities,
        pricing_thread_pool,
        cpu_count,
    } = input;

    let (theta, _) = resolve_local_transition_theta_for_row_with_parallelism_and_hint(
        base_context,
        transition_row,
        driver_parameters,
        initial_theta_hint,
        pricing_thread_pool,
        cpu_count,
    )?;

    let solution = solve_row_frequency_batch(
        base_context,
        transition_row,
        spectral_domain,
        driver_parameters,
        theta,
    )?;

    let core_log_moment_sensitivity = if collect_theta_warm_start_sensitivities {
        Some(
            differentiate_core_pure_imaginary_log_moment_exponent_for_tilt(
                base_context,
                driver_parameters,
                theta,
            )?,
        )
    } else {
        None
    };

    let theta_warm_start_state = if collect_theta_warm_start_sensitivities {
        if let Some(theta_warm_start_sensitivity) = core_log_moment_sensitivity
            .as_ref()
            .and_then(try_resolve_theta_warm_start_sensitivity)
        {
            SpectralThetaWarmStartRowState::from_theta_hint(theta)
                .with_local_theta_sensitivity(theta_warm_start_sensitivity)
        } else {
            SpectralThetaWarmStartRowState::from_theta_hint(theta)
        }
    } else {
        SpectralThetaWarmStartRowState::from_theta_hint(theta)
    };

    Ok(TimedRowFrequencyBatchSolution {
        solution,
        theta_warm_start_state,
    })
}

#[derive(Clone, Copy)]
pub(in crate::spectral::pricer) struct AdaptiveMidpointRowFrequencyBatchSolveInput<'a> {
    pub(in crate::spectral::pricer) base_context: &'a LocalTransitionRequestContext,
    pub(in crate::spectral::pricer) transition_row: &'a LocalTransitionRowState,
    pub(in crate::spectral::pricer) initial_theta_hint: Option<f64>,
    pub(in crate::spectral::pricer) predicted_theta: f64,
    pub(in crate::spectral::pricer) spectral_domain: SpectralDomain,
    pub(in crate::spectral::pricer) driver_parameters:
        janiform_model_kernels::nig::NigBuySellDriverParameters,
    pub(in crate::spectral::pricer) collect_theta_warm_start_sensitivities: bool,
    pub(in crate::spectral::pricer) pricing_thread_pool: Option<&'a rayon::ThreadPool>,
    pub(in crate::spectral::pricer) cpu_count: Option<usize>,
}

pub(in crate::spectral::pricer) fn solve_adaptive_midpoint_row_frequency_batch(
    input: AdaptiveMidpointRowFrequencyBatchSolveInput<'_>,
) -> Result<TimedRowFrequencyBatchSolution, VanillaOptionPricingRunError> {
    let AdaptiveMidpointRowFrequencyBatchSolveInput {
        base_context,
        transition_row,
        initial_theta_hint,
        predicted_theta,
        spectral_domain,
        driver_parameters,
        collect_theta_warm_start_sensitivities,
        pricing_thread_pool,
        cpu_count,
    } = input;

    if certify_local_transition_theta_for_row_with_parallelism(
        base_context,
        transition_row,
        driver_parameters,
        predicted_theta,
        pricing_thread_pool,
        cpu_count,
    )? {
        let solution = solve_row_frequency_batch(
            base_context,
            transition_row,
            spectral_domain,
            driver_parameters,
            predicted_theta,
        )?;

        return Ok(TimedRowFrequencyBatchSolution {
            solution,
            theta_warm_start_state: SpectralThetaWarmStartRowState::from_theta_hint(
                predicted_theta,
            ),
        });
    }

    let exact_solution = solve_exact_row_frequency_batch(ExactRowFrequencyBatchSolveInput {
        base_context,
        transition_row,
        initial_theta_hint,
        spectral_domain,
        driver_parameters,
        collect_theta_warm_start_sensitivities,
        pricing_thread_pool,
        cpu_count,
    })?;

    Ok(TimedRowFrequencyBatchSolution {
        solution: exact_solution.solution,
        theta_warm_start_state: exact_solution.theta_warm_start_state,
    })
}

pub(in crate::spectral::pricer) fn interpolate_theta_across_deviation_rows(
    left_deviation: f64,
    left_theta: f64,
    right_deviation: f64,
    right_theta: f64,
    target_deviation: f64,
) -> f64 {
    let deviation_span = right_deviation - left_deviation;
    if deviation_span == 0.0 {
        return left_theta;
    }

    let interpolation_weight = (target_deviation - left_deviation) / deviation_span;

    left_theta + interpolation_weight * (right_theta - left_theta)
}

pub(in crate::spectral::pricer) fn derive_expected_increment_from_frequency_values(
    positive_frequency_values: &[ComplexScalar],
    spectral_domain: SpectralDomain,
) -> Result<f64, VanillaOptionPricingRunError> {
    let first_frequency = spectral_domain.basis_frequency(1);
    let first_value = positive_frequency_values.get(1).copied().ok_or_else(|| {
        VanillaOptionPricingRunError::InternalNumericsFailure {
            message: "spectral expected-increment recovery requires at least two frequencies"
                .to_string(),
        }
    })?;

    Ok(first_value.imaginary / first_frequency)
}
