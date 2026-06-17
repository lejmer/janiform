use janiform_foundations::complex_math::{ComplexScalar, complex_exponential, complex_multiply};
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, target_source_rates_from_shared_activity_loading,
};
use janiform_model_kernels::long_variance::{
    LongVarianceState, LongVarianceStationaryLevelForHawkesKernelError,
};
use janiform_model_kernels::news::expected_news_activity_loading_rate;

use super::super::contracts::{
    ExponentialAffineTransformDVector, ExponentialAffineTransformInput,
    ExponentialAffineTransformOutputError,
};
use super::super::moving_activity_loadings::MovingActivityLoadings;
use super::super::news_transform::{
    GammaNormalNewsTransformComponents, evaluate_gamma_normal_news_transform,
    evaluate_gamma_normal_news_transform_tangent,
};
use super::super::runge_kutta::{combine_complex_runge_kutta, combine_d_vector_runge_kutta};
use super::super::solver::{
    ExponentialAffineTransformGuardFailure, ExponentialAffineTransformGuardFailureCode,
    ExponentialAffineTransformIntegratorMethod, build_guard_failure,
    evaluate_transform_guard_domain_metrics, internally_selected_default_solver_settings,
};
use super::types::{
    ExponentialAffineScheduleCumulantError, ExponentialAffineScheduleCumulantOutput,
    ExponentialAffineScheduleCumulantProgress, ExponentialAffineScheduleCumulantRequest,
    ExponentialAffineScheduleIntervalInput, ExponentialAffineTransformInputTangent,
    validate_finite_output_derivative,
};

/// Evaluates the affine cumulant and tangent derivatives over a full interval schedule.
pub fn solve_exponential_affine_schedule_cumulant(
    request: &ExponentialAffineScheduleCumulantRequest,
) -> Result<ExponentialAffineScheduleCumulantOutput, ExponentialAffineScheduleCumulantError> {
    solve_exponential_affine_schedule_cumulant_with_progress(request, None)
}

/// Evaluates the affine cumulant while reporting schedule interval progress.
pub fn solve_exponential_affine_schedule_cumulant_with_progress(
    request: &ExponentialAffineScheduleCumulantRequest,
    mut progress_observer: Option<&mut dyn FnMut(ExponentialAffineScheduleCumulantProgress)>,
) -> Result<ExponentialAffineScheduleCumulantOutput, ExponentialAffineScheduleCumulantError> {
    request.validate()?;

    let mut exponent = ScheduleExponent::zero();
    let mut completed_step_count = 0_usize;
    let mut rhs_evaluation_count = 0_usize;
    let mut completed_interval_count = 0_usize;

    let total_interval_count = request.intervals.len();

    emit_schedule_cumulant_progress(&mut progress_observer, 0, total_interval_count, None);

    for (interval_position, interval) in request.intervals.iter().enumerate().rev() {
        let interval_output = integrate_schedule_interval(*interval, exponent, interval_position)?;

        exponent = exponent.compose(interval_output.exponent);

        completed_step_count =
            completed_step_count.saturating_add(interval_output.completed_step_count);
        rhs_evaluation_count =
            rhs_evaluation_count.saturating_add(interval_output.rhs_evaluation_count);

        completed_interval_count = completed_interval_count.saturating_add(1);

        emit_schedule_cumulant_progress(
            &mut progress_observer,
            completed_interval_count,
            total_interval_count,
            Some(interval_position),
        );
    }

    let output = ExponentialAffineScheduleCumulantOutput {
        exponent: exponent
            .base
            .evaluated_at(request.initial_state, request.initial_long_variance_state),
        argument_derivative: exponent
            .argument_tangent
            .evaluated_at(request.initial_state, request.initial_long_variance_state),
        buy_activity_premium_derivative: exponent
            .buy_activity_premium_tangent
            .evaluated_at(request.initial_state, request.initial_long_variance_state),
        sell_activity_premium_derivative: exponent
            .sell_activity_premium_tangent
            .evaluated_at(request.initial_state, request.initial_long_variance_state),
        interval_count: request.intervals.len(),
        completed_step_count,
        rhs_evaluation_count,
    };

    validate_finite_output_derivative("argument", output.argument_derivative)?;
    validate_finite_output_derivative(
        "buy_activity_premium",
        output.buy_activity_premium_derivative,
    )?;
    validate_finite_output_derivative(
        "sell_activity_premium",
        output.sell_activity_premium_derivative,
    )?;

    Ok(output)
}

fn emit_schedule_cumulant_progress(
    progress_observer: &mut Option<&mut dyn FnMut(ExponentialAffineScheduleCumulantProgress)>,
    completed_interval_count: usize,
    total_interval_count: usize,
    current_interval_position: Option<usize>,
) {
    if let Some(progress_observer) = progress_observer.as_deref_mut() {
        progress_observer(ExponentialAffineScheduleCumulantProgress {
            completed_interval_count,
            total_interval_count,
            current_interval_position,
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct AffineState {
    d_vector: ExponentialAffineTransformDVector,
    long_variance_loading: ComplexScalar,
    c_value: ComplexScalar,
}

impl AffineState {
    fn zero() -> Self {
        Self {
            d_vector: ExponentialAffineTransformDVector::zero(),
            long_variance_loading: ComplexScalar::new(0.0, 0.0),
            c_value: ComplexScalar::new(0.0, 0.0),
        }
    }

    fn add_scaled_rhs(self, rhs: AffineState, scale: f64) -> Self {
        Self {
            d_vector: self.d_vector.plus(rhs.d_vector.scale(scale)),
            long_variance_loading: self
                .long_variance_loading
                .plus(rhs.long_variance_loading.scale(scale)),
            c_value: self.c_value.plus(rhs.c_value.scale(scale)),
        }
    }

    fn evaluated_at(
        self,
        hawkes_state: HawkesExcitationState,
        long_variance_state: LongVarianceState,
    ) -> ComplexScalar {
        self.c_value
            .plus(self.d_vector.dot_state(hawkes_state))
            .plus(self.long_variance_loading.scale(long_variance_state.level))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ScheduleExponent {
    base: AffineState,
    argument_tangent: AffineState,
    buy_activity_premium_tangent: AffineState,
    sell_activity_premium_tangent: AffineState,
}

impl ScheduleExponent {
    fn zero() -> Self {
        Self {
            base: AffineState::zero(),
            argument_tangent: AffineState::zero(),
            buy_activity_premium_tangent: AffineState::zero(),
            sell_activity_premium_tangent: AffineState::zero(),
        }
    }

    fn compose(self, interval_output: Self) -> Self {
        Self {
            base: AffineState {
                d_vector: interval_output.base.d_vector,
                long_variance_loading: interval_output.base.long_variance_loading,
                c_value: self.base.c_value.plus(interval_output.base.c_value),
            },
            argument_tangent: AffineState {
                d_vector: interval_output.argument_tangent.d_vector,
                long_variance_loading: interval_output.argument_tangent.long_variance_loading,
                c_value: self
                    .argument_tangent
                    .c_value
                    .plus(interval_output.argument_tangent.c_value),
            },
            buy_activity_premium_tangent: AffineState {
                d_vector: interval_output.buy_activity_premium_tangent.d_vector,
                long_variance_loading: interval_output
                    .buy_activity_premium_tangent
                    .long_variance_loading,
                c_value: self
                    .buy_activity_premium_tangent
                    .c_value
                    .plus(interval_output.buy_activity_premium_tangent.c_value),
            },
            sell_activity_premium_tangent: AffineState {
                d_vector: interval_output.sell_activity_premium_tangent.d_vector,
                long_variance_loading: interval_output
                    .sell_activity_premium_tangent
                    .long_variance_loading,
                c_value: self
                    .sell_activity_premium_tangent
                    .c_value
                    .plus(interval_output.sell_activity_premium_tangent.c_value),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ScheduleIntervalOutput {
    exponent: ScheduleExponent,
    completed_step_count: usize,
    rhs_evaluation_count: usize,
}

fn integrate_schedule_interval(
    interval: ExponentialAffineScheduleIntervalInput,
    terminal_exponent: ScheduleExponent,
    interval_position: usize,
) -> Result<ScheduleIntervalOutput, ExponentialAffineScheduleCumulantError> {
    terminal_exponent
        .base
        .d_vector
        .validate()
        .map_err(
            |source| ExponentialAffineScheduleCumulantError::InvalidTerminalLoading {
                source: ExponentialAffineTransformOutputError::InvalidDAtDelta { source },
            },
        )?;

    let solver_settings = internally_selected_default_solver_settings(interval.transform_input);

    solver_settings.validate().map_err(|source| {
        ExponentialAffineScheduleCumulantError::InvalidSolverSettings { source }
    })?;

    let step_size = interval.transform_input.delta / solver_settings.step_count as f64;

    let mut state = ScheduleExponent {
        base: AffineState {
            d_vector: terminal_exponent.base.d_vector,
            long_variance_loading: terminal_exponent.base.long_variance_loading,
            c_value: ComplexScalar::new(0.0, 0.0),
        },
        argument_tangent: AffineState {
            d_vector: terminal_exponent.argument_tangent.d_vector,
            long_variance_loading: terminal_exponent.argument_tangent.long_variance_loading,
            c_value: ComplexScalar::new(0.0, 0.0),
        },
        buy_activity_premium_tangent: AffineState {
            d_vector: terminal_exponent.buy_activity_premium_tangent.d_vector,
            long_variance_loading: terminal_exponent
                .buy_activity_premium_tangent
                .long_variance_loading,
            c_value: ComplexScalar::new(0.0, 0.0),
        },
        sell_activity_premium_tangent: AffineState {
            d_vector: terminal_exponent.sell_activity_premium_tangent.d_vector,
            long_variance_loading: terminal_exponent
                .sell_activity_premium_tangent
                .long_variance_loading,
            c_value: ComplexScalar::new(0.0, 0.0),
        },
    };

    let initial_guard =
        evaluate_schedule_guard_checkpoint(interval.transform_input, state.base, 0, step_size);
    if let Some(guard_failure) = initial_guard {
        return Err(ExponentialAffineScheduleCumulantError::GuardFailure {
            interval_position,
            guard_failure,
        });
    }

    let mut completed_step_count = 0_usize;
    let mut rhs_evaluation_count = 0_usize;

    for _ in 0..solver_settings.step_count {
        state = advance_schedule_state(
            interval,
            state,
            step_size,
            solver_settings.integrator_method,
        )?;

        completed_step_count = completed_step_count.saturating_add(1);
        rhs_evaluation_count =
            rhs_evaluation_count.saturating_add(match solver_settings.integrator_method {
                ExponentialAffineTransformIntegratorMethod::ForwardEuler => 1,
                ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta => 4,
            });

        let guard = evaluate_schedule_guard_checkpoint(
            interval.transform_input,
            state.base,
            completed_step_count,
            step_size,
        );
        if let Some(guard_failure) = guard {
            return Err(ExponentialAffineScheduleCumulantError::GuardFailure {
                interval_position,
                guard_failure,
            });
        }
    }

    state.base.d_vector.validate().map_err(|source| {
        ExponentialAffineScheduleCumulantError::InvalidOutput {
            source: ExponentialAffineTransformOutputError::InvalidDAtDelta { source },
        }
    })?;

    if !state.base.c_value.is_finite() {
        return Err(ExponentialAffineScheduleCumulantError::InvalidOutput {
            source: ExponentialAffineTransformOutputError::InvalidCAtDelta {
                value: state.base.c_value,
            },
        });
    }

    Ok(ScheduleIntervalOutput {
        exponent: ScheduleExponent {
            base: state.base,
            argument_tangent: state.argument_tangent,
            buy_activity_premium_tangent: state.buy_activity_premium_tangent,
            sell_activity_premium_tangent: state.sell_activity_premium_tangent,
        },
        completed_step_count,
        rhs_evaluation_count,
    })
}

fn advance_schedule_state(
    interval: ExponentialAffineScheduleIntervalInput,
    state: ScheduleExponent,
    step_size: f64,
    integrator_method: ExponentialAffineTransformIntegratorMethod,
) -> Result<ScheduleExponent, ExponentialAffineScheduleCumulantError> {
    let next_state = match integrator_method {
        ExponentialAffineTransformIntegratorMethod::ForwardEuler => {
            state.add_scaled_rhs(evaluate_schedule_rhs(interval, state)?, step_size)
        }
        ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta => {
            let k1 = evaluate_schedule_rhs(interval, state)?;
            let k2 = evaluate_schedule_rhs(interval, state.add_scaled_rhs(k1, 0.5 * step_size))?;
            let k3 = evaluate_schedule_rhs(interval, state.add_scaled_rhs(k2, 0.5 * step_size))?;
            let k4 = evaluate_schedule_rhs(interval, state.add_scaled_rhs(k3, step_size))?;

            state.add_scaled_rhs(combine_schedule_runge_kutta_rhs(k1, k2, k3, k4), step_size)
        }
    };

    Ok(next_state)
}

impl ScheduleExponent {
    fn add_scaled_rhs(self, rhs: Self, scale: f64) -> Self {
        Self {
            base: self.base.add_scaled_rhs(rhs.base, scale),
            argument_tangent: self
                .argument_tangent
                .add_scaled_rhs(rhs.argument_tangent, scale),
            buy_activity_premium_tangent: self
                .buy_activity_premium_tangent
                .add_scaled_rhs(rhs.buy_activity_premium_tangent, scale),
            sell_activity_premium_tangent: self
                .sell_activity_premium_tangent
                .add_scaled_rhs(rhs.sell_activity_premium_tangent, scale),
        }
    }
}

fn combine_schedule_runge_kutta_rhs(
    k1: ScheduleExponent,
    k2: ScheduleExponent,
    k3: ScheduleExponent,
    k4: ScheduleExponent,
) -> ScheduleExponent {
    ScheduleExponent {
        base: combine_affine_runge_kutta(k1.base, k2.base, k3.base, k4.base),
        argument_tangent: combine_affine_runge_kutta(
            k1.argument_tangent,
            k2.argument_tangent,
            k3.argument_tangent,
            k4.argument_tangent,
        ),
        buy_activity_premium_tangent: combine_affine_runge_kutta(
            k1.buy_activity_premium_tangent,
            k2.buy_activity_premium_tangent,
            k3.buy_activity_premium_tangent,
            k4.buy_activity_premium_tangent,
        ),
        sell_activity_premium_tangent: combine_affine_runge_kutta(
            k1.sell_activity_premium_tangent,
            k2.sell_activity_premium_tangent,
            k3.sell_activity_premium_tangent,
            k4.sell_activity_premium_tangent,
        ),
    }
}

fn combine_affine_runge_kutta(
    k1: AffineState,
    k2: AffineState,
    k3: AffineState,
    k4: AffineState,
) -> AffineState {
    AffineState {
        d_vector: combine_d_vector_runge_kutta(k1.d_vector, k2.d_vector, k3.d_vector, k4.d_vector),
        long_variance_loading: combine_complex_runge_kutta(
            k1.long_variance_loading,
            k2.long_variance_loading,
            k3.long_variance_loading,
            k4.long_variance_loading,
        ),
        c_value: combine_complex_runge_kutta(k1.c_value, k2.c_value, k3.c_value, k4.c_value),
    }
}

fn evaluate_schedule_rhs(
    interval: ExponentialAffineScheduleIntervalInput,
    state: ScheduleExponent,
) -> Result<ScheduleExponent, ExponentialAffineScheduleCumulantError> {
    let base_rhs = evaluate_schedule_base_rhs(interval.transform_input, state.base)?;

    Ok(ScheduleExponent {
        base: AffineState {
            d_vector: base_rhs.d_derivative,
            long_variance_loading: base_rhs.long_variance_derivative,
            c_value: base_rhs.c_derivative,
        },
        argument_tangent: evaluate_schedule_tangent_rhs(
            interval.transform_input,
            state.base,
            state.argument_tangent,
            interval.argument_tangent,
        )?,
        buy_activity_premium_tangent: evaluate_schedule_tangent_rhs(
            interval.transform_input,
            state.base,
            state.buy_activity_premium_tangent,
            interval.buy_activity_premium_tangent,
        )?,
        sell_activity_premium_tangent: evaluate_schedule_tangent_rhs(
            interval.transform_input,
            state.base,
            state.sell_activity_premium_tangent,
            interval.sell_activity_premium_tangent,
        )?,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ScheduleAffineRhs {
    d_derivative: ExponentialAffineTransformDVector,
    long_variance_derivative: ComplexScalar,
    c_derivative: ComplexScalar,
}

fn evaluate_schedule_base_rhs(
    transform_input: ExponentialAffineTransformInput,
    base_state: AffineState,
) -> Result<ScheduleAffineRhs, ExponentialAffineScheduleCumulantError> {
    let equation_specification = transform_input.equation_specification;
    let beta = equation_specification.hawkes_kernel_parameters.beta;

    let hamiltonians = evaluate_long_variance_schedule_hamiltonians(transform_input, base_state);

    let long_variance_level = stationary_long_variance_level(transform_input)?;

    let d_derivative = ExponentialAffineTransformDVector::new(
        base_state
            .d_vector
            .plus_plus
            .scale(-beta)
            .plus(hamiltonians.h_plus_endogenous),
        base_state
            .d_vector
            .plus_minus
            .scale(-beta)
            .plus(hamiltonians.h_plus_endogenous),
        base_state
            .d_vector
            .minus_plus
            .scale(-beta)
            .plus(hamiltonians.h_minus_endogenous),
        base_state
            .d_vector
            .minus_minus
            .scale(-beta)
            .plus(hamiltonians.h_minus_endogenous),
    );

    let long_variance_parameters = equation_specification.long_variance_parameters;

    let moving_activity_loadings = MovingActivityLoadings::from_total_activity_loadings(
        equation_specification.no_move_event_model,
        transform_input.activity_loadings,
    );

    let baseline_rate = transform_input.seasonality_factor
        * equation_specification.hawkes_kernel_parameters.lambda_bar;

    let long_variance_derivative = base_state
        .long_variance_loading
        .scale(-long_variance_parameters.mean_reversion_rate)
        .plus(
            base_state
                .long_variance_loading
                .square()
                .scale(0.5 * long_variance_parameters.vol_of_vol.powi(2)),
        )
        .plus(
            moving_activity_loadings
                .buy_baseline
                .plus(moving_activity_loadings.sell_baseline)
                .scale(-baseline_rate),
        );

    let baseline_event_term = hamiltonians
        .h_plus_baseline
        .plus(moving_activity_loadings.buy_baseline)
        .plus(
            hamiltonians
                .h_minus_baseline
                .plus(moving_activity_loadings.sell_baseline),
        )
        .scale(baseline_rate);

    let c_derivative = base_state
        .long_variance_loading
        .scale(long_variance_parameters.mean_reversion_rate * long_variance_level)
        .plus(baseline_event_term)
        .plus(evaluate_news_base_rhs(transform_input, base_state.d_vector));

    Ok(ScheduleAffineRhs {
        d_derivative,
        long_variance_derivative,
        c_derivative,
    })
}

fn evaluate_schedule_tangent_rhs(
    transform_input: ExponentialAffineTransformInput,
    base_state: AffineState,
    tangent_state: AffineState,
    direct_tangent: ExponentialAffineTransformInputTangent,
) -> Result<AffineState, ExponentialAffineScheduleCumulantError> {
    let equation_specification = transform_input.equation_specification;
    let beta = equation_specification.hawkes_kernel_parameters.beta;
    let long_variance_parameters = equation_specification.long_variance_parameters;

    let moving_direct_loadings = MovingActivityLoadings::from_total_activity_loadings(
        equation_specification.no_move_event_model,
        direct_tangent.activity_loadings,
    );

    let long_variance_level = stationary_long_variance_level(transform_input)?;

    let k_plus = equation_specification.k_plus();
    let k_minus = equation_specification.k_minus();

    let exp_plus_argument = base_state
        .d_vector
        .dot_loadings(equation_specification.k_plus())
        .plus(
            base_state
                .long_variance_loading
                .scale(long_variance_parameters.buy_event_loading),
        );

    let exp_minus_argument = base_state
        .d_vector
        .dot_loadings(equation_specification.k_minus())
        .plus(
            base_state
                .long_variance_loading
                .scale(long_variance_parameters.sell_event_loading),
        );

    let exp_d_dot_k_plus = complex_exponential(exp_plus_argument);
    let exp_d_dot_k_minus = complex_exponential(exp_minus_argument);

    let plus_factor = complex_multiply(transform_input.z_plus, exp_d_dot_k_plus);
    let minus_factor = complex_multiply(transform_input.z_minus, exp_d_dot_k_minus);

    let plus_loading_derivative = tangent_state.d_vector.dot_loadings(k_plus).plus(
        tangent_state
            .long_variance_loading
            .scale(long_variance_parameters.buy_event_loading),
    );

    let minus_loading_derivative = tangent_state.d_vector.dot_loadings(k_minus).plus(
        tangent_state
            .long_variance_loading
            .scale(long_variance_parameters.sell_event_loading),
    );

    let plus_state_derivative = complex_multiply(plus_factor, plus_loading_derivative);
    let minus_state_derivative = complex_multiply(minus_factor, minus_loading_derivative);

    let baseline_rate = transform_input.seasonality_factor
        * equation_specification.hawkes_kernel_parameters.lambda_bar;

    let h_plus_endogenous_derivative = plus_state_derivative
        .plus(complex_multiply(direct_tangent.z_plus, exp_d_dot_k_plus))
        .subtract(moving_direct_loadings.buy_endogenous);

    let h_minus_endogenous_derivative = minus_state_derivative
        .plus(complex_multiply(direct_tangent.z_minus, exp_d_dot_k_minus))
        .subtract(moving_direct_loadings.sell_endogenous);

    let baseline_event_derivative = plus_state_derivative
        .plus(complex_multiply(direct_tangent.z_plus, exp_d_dot_k_plus))
        .plus(
            minus_state_derivative
                .plus(complex_multiply(direct_tangent.z_minus, exp_d_dot_k_minus)),
        )
        .scale(baseline_rate);

    let news_derivative = evaluate_news_tangent_rhs(
        transform_input,
        base_state.d_vector,
        tangent_state.d_vector,
        direct_tangent.news_return_loading,
    );

    Ok(AffineState {
        d_vector: ExponentialAffineTransformDVector::new(
            tangent_state
                .d_vector
                .plus_plus
                .scale(-beta)
                .plus(h_plus_endogenous_derivative),
            tangent_state
                .d_vector
                .plus_minus
                .scale(-beta)
                .plus(h_plus_endogenous_derivative),
            tangent_state
                .d_vector
                .minus_plus
                .scale(-beta)
                .plus(h_minus_endogenous_derivative),
            tangent_state
                .d_vector
                .minus_minus
                .scale(-beta)
                .plus(h_minus_endogenous_derivative),
        ),
        long_variance_loading: tangent_state
            .long_variance_loading
            .scale(-long_variance_parameters.mean_reversion_rate)
            .plus(
                complex_multiply(
                    base_state.long_variance_loading,
                    tangent_state.long_variance_loading,
                )
                .scale(long_variance_parameters.vol_of_vol.powi(2)),
            )
            .plus(
                moving_direct_loadings
                    .buy_baseline
                    .plus(moving_direct_loadings.sell_baseline)
                    .scale(-baseline_rate),
            ),
        c_value: tangent_state
            .long_variance_loading
            .scale(long_variance_parameters.mean_reversion_rate * long_variance_level)
            .plus(baseline_event_derivative)
            .plus(news_derivative),
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ScheduleHamiltonians {
    h_plus_baseline: ComplexScalar,
    h_plus_endogenous: ComplexScalar,
    h_minus_baseline: ComplexScalar,
    h_minus_endogenous: ComplexScalar,
}

fn evaluate_long_variance_schedule_hamiltonians(
    transform_input: ExponentialAffineTransformInput,
    state: AffineState,
) -> ScheduleHamiltonians {
    let equation_specification = transform_input.equation_specification;

    let moving_activity_loadings = MovingActivityLoadings::from_total_activity_loadings(
        equation_specification.no_move_event_model,
        transform_input.activity_loadings,
    );

    let long_variance_parameters = equation_specification.long_variance_parameters;

    let plus_argument = state
        .d_vector
        .dot_loadings(equation_specification.k_plus())
        .plus(
            state
                .long_variance_loading
                .scale(long_variance_parameters.buy_event_loading),
        );

    let minus_argument = state
        .d_vector
        .dot_loadings(equation_specification.k_minus())
        .plus(
            state
                .long_variance_loading
                .scale(long_variance_parameters.sell_event_loading),
        );

    ScheduleHamiltonians {
        h_plus_baseline: evaluate_schedule_hamiltonian_component(
            moving_activity_loadings.buy_baseline,
            transform_input.z_plus,
            plus_argument,
        ),
        h_plus_endogenous: evaluate_schedule_hamiltonian_component(
            moving_activity_loadings.buy_endogenous,
            transform_input.z_plus,
            plus_argument,
        ),
        h_minus_baseline: evaluate_schedule_hamiltonian_component(
            moving_activity_loadings.sell_baseline,
            transform_input.z_minus,
            minus_argument,
        ),
        h_minus_endogenous: evaluate_schedule_hamiltonian_component(
            moving_activity_loadings.sell_endogenous,
            transform_input.z_minus,
            minus_argument,
        ),
    }
}

fn evaluate_schedule_hamiltonian_component(
    activity_loading: ComplexScalar,
    event_loading: ComplexScalar,
    exponent_argument: ComplexScalar,
) -> ComplexScalar {
    complex_multiply(event_loading, complex_exponential(exponent_argument))
        .subtract(ComplexScalar::new(1.0, 0.0))
        .subtract(activity_loading)
}

fn stationary_long_variance_level(
    transform_input: ExponentialAffineTransformInput,
) -> Result<f64, ExponentialAffineScheduleCumulantError> {
    let equation_specification = transform_input.equation_specification;

    let news_activity_loading_rate = expected_news_activity_loading_rate(
        transform_input.news_intensity.known_news_intensity,
        transform_input.news_intensity.surprise_news_intensity,
        transform_input.known_news_component_mark_scales,
        transform_input.surprise_news_component_mark_scales,
        equation_specification.gamma_severity_mark_configuration,
    )
    .map_err(|source| {
        ExponentialAffineScheduleCumulantError::InvalidNewsActivityLoadingRate { source }
    })?;

    let exogenous_source_rates = target_source_rates_from_shared_activity_loading(
        equation_specification.hawkes_kernel_parameters.rho,
        news_activity_loading_rate,
    )
    .ok_or(ExponentialAffineScheduleCumulantError::InvalidStationaryIntensityMultipliers)?;

    equation_specification
        .long_variance_parameters
        .stationary_mean_reversion_level_for_hawkes_kernel(
            equation_specification.hawkes_kernel_parameters,
            exogenous_source_rates,
        )
        .map_err(|source| match source {
            LongVarianceStationaryLevelForHawkesKernelError::InvalidStationaryIntensityMultipliers => {
                ExponentialAffineScheduleCumulantError::InvalidStationaryIntensityMultipliers
            }
            LongVarianceStationaryLevelForHawkesKernelError::InvalidStationaryLevel {
                source,
            } => ExponentialAffineScheduleCumulantError::InvalidLongVarianceStationaryLevel {
                source,
            },
        })
}

fn evaluate_news_tangent_rhs(
    transform_input: ExponentialAffineTransformInput,
    base_d_vector: ExponentialAffineTransformDVector,
    tangent_d_vector: ExponentialAffineTransformDVector,
    return_loading_tangent: ComplexScalar,
) -> ComplexScalar {
    let equation_specification = transform_input.equation_specification;

    evaluate_gamma_normal_news_transform_tangent(
        GammaNormalNewsTransformComponents {
            gamma_severity_mark_configuration: equation_specification
                .gamma_severity_mark_configuration,
            news_intensity: transform_input.news_intensity,
            known_component_mark_scales: transform_input.known_news_component_mark_scales,
            surprise_component_mark_scales: transform_input.surprise_news_component_mark_scales,
            activity_loading: base_d_vector.dot_loadings(equation_specification.r()),
            return_loading: transform_input.news_return_loading,
        },
        tangent_d_vector.dot_loadings(equation_specification.r()),
        return_loading_tangent,
    )
}

fn evaluate_news_base_rhs(
    transform_input: ExponentialAffineTransformInput,
    base_d_vector: ExponentialAffineTransformDVector,
) -> ComplexScalar {
    let equation_specification = transform_input.equation_specification;

    evaluate_gamma_normal_news_transform(GammaNormalNewsTransformComponents {
        gamma_severity_mark_configuration: equation_specification.gamma_severity_mark_configuration,
        news_intensity: transform_input.news_intensity,
        known_component_mark_scales: transform_input.known_news_component_mark_scales,
        surprise_component_mark_scales: transform_input.surprise_news_component_mark_scales,
        activity_loading: base_d_vector.dot_loadings(equation_specification.r()),
        return_loading: transform_input.news_return_loading,
    })
}

fn evaluate_schedule_guard_checkpoint(
    transform_input: ExponentialAffineTransformInput,
    state: AffineState,
    completed_step_count: usize,
    step_size: f64,
) -> Option<ExponentialAffineTransformGuardFailure> {
    let checkpoint_time = completed_step_count as f64 * step_size;
    let guard_domain_metrics =
        evaluate_transform_guard_domain_metrics(transform_input, state.d_vector);

    if !state.c_value.is_finite()
        || !state.long_variance_loading.is_finite()
        || !d_vector_is_finite(state.d_vector)
    {
        return Some(build_guard_failure(
            ExponentialAffineTransformGuardFailureCode::NonFiniteState,
            completed_step_count,
            checkpoint_time,
            guard_domain_metrics.d_dot_r,
            guard_domain_metrics.d_dot_r_upper_bound,
        ));
    }

    if !guard_domain_metrics.d_dot_r.is_finite()
        || !guard_domain_metrics.d_dot_r_upper_bound.is_finite()
    {
        return Some(build_guard_failure(
            ExponentialAffineTransformGuardFailureCode::NonFiniteDomainMetric,
            completed_step_count,
            checkpoint_time,
            guard_domain_metrics.d_dot_r,
            guard_domain_metrics.d_dot_r_upper_bound,
        ));
    }

    if guard_domain_metrics.d_dot_r.real >= guard_domain_metrics.d_dot_r_upper_bound {
        return Some(build_guard_failure(
            ExponentialAffineTransformGuardFailureCode::DomainUpperBoundViolation,
            completed_step_count,
            checkpoint_time,
            guard_domain_metrics.d_dot_r,
            guard_domain_metrics.d_dot_r_upper_bound,
        ));
    }

    None
}

fn d_vector_is_finite(value: ExponentialAffineTransformDVector) -> bool {
    value.plus_plus.is_finite()
        && value.plus_minus.is_finite()
        && value.minus_plus.is_finite()
        && value.minus_minus.is_finite()
}

#[cfg(test)]
mod tests {
    use super::{
        ExponentialAffineScheduleCumulantRequest, ExponentialAffineScheduleIntervalInput,
        ExponentialAffineTransformInput, ExponentialAffineTransformInputTangent,
        solve_exponential_affine_schedule_cumulant,
        solve_exponential_affine_schedule_cumulant_with_progress,
    };
    use crate::affine_oracle::ExponentialAffineTransformDVector;
    use crate::affine_oracle::contracts::{
        ExponentialAffineTransformActivityLoadings, ExponentialAffineTransformNewsIntensityInput,
    };
    use crate::affine_oracle::solver::solve_exponential_affine_transform_with_terminal_loading;
    use crate::affine_oracle::test_support::{
        standard_initial_state, valid_equation_specification,
    };
    use janiform_foundations::complex_math::{ComplexScalar, complex_exponential};
    use janiform_model_kernels::long_variance::LongVarianceState;
    use janiform_model_kernels::news::NewsComponentMarkScaleConfiguration;

    fn transform_input(
        activity_loadings: ExponentialAffineTransformActivityLoadings,
    ) -> ExponentialAffineTransformInput {
        ExponentialAffineTransformInput::try_new(
            valid_equation_specification(),
            activity_loadings,
            ComplexScalar::new(1.0, 0.0),
            ComplexScalar::new(0.95, 0.02),
            ComplexScalar::new(0.0, 0.0),
            1.1,
            ExponentialAffineTransformNewsIntensityInput::try_new(0.03, 0.05)
                .expect("news intensity components should construct"),
            NewsComponentMarkScaleConfiguration::neutral(),
            NewsComponentMarkScaleConfiguration::neutral(),
            1.0,
            standard_initial_state(),
        )
        .expect("valid transform input should construct")
    }

    fn base_loadings() -> ExponentialAffineTransformActivityLoadings {
        ExponentialAffineTransformActivityLoadings::new(
            ComplexScalar::new(0.2, -0.1),
            ComplexScalar::new(0.2, -0.1),
            ComplexScalar::new(0.3, 0.05),
            ComplexScalar::new(0.3, 0.05),
        )
    }

    fn interval_input(
        activity_loadings: ExponentialAffineTransformActivityLoadings,
        argument_tangent: ExponentialAffineTransformInputTangent,
    ) -> ExponentialAffineScheduleIntervalInput {
        ExponentialAffineScheduleIntervalInput {
            transform_input: transform_input(activity_loadings),
            argument_tangent,
            buy_activity_premium_tangent: ExponentialAffineTransformInputTangent::zero(),
            sell_activity_premium_tangent: ExponentialAffineTransformInputTangent::zero(),
        }
    }

    fn schedule_output_for_loadings(
        activity_loadings: ExponentialAffineTransformActivityLoadings,
        argument_tangent: ExponentialAffineTransformInputTangent,
    ) -> ComplexScalar {
        solve_exponential_affine_schedule_cumulant(&ExponentialAffineScheduleCumulantRequest {
            initial_state: standard_initial_state(),
            initial_long_variance_state: LongVarianceState::unit(),
            intervals: vec![interval_input(activity_loadings, argument_tangent)],
        })
        .expect("schedule cumulant should solve")
        .exponent
    }

    #[test]
    fn schedule_cumulant_matches_one_interval_transform_value() {
        let schedule_output =
            solve_exponential_affine_schedule_cumulant(&ExponentialAffineScheduleCumulantRequest {
                initial_state: standard_initial_state(),
                initial_long_variance_state: LongVarianceState::unit(),
                intervals: vec![interval_input(
                    base_loadings(),
                    ExponentialAffineTransformInputTangent::zero(),
                )],
            })
            .expect("schedule cumulant should solve");

        let one_interval_output = solve_exponential_affine_transform_with_terminal_loading(
            transform_input(base_loadings()),
            ExponentialAffineTransformDVector::zero(),
        )
        .expect("one-interval transform should solve")
        .output
        .expect("one-interval transform should produce output");

        let schedule_transform_value = complex_exponential(schedule_output.exponent);
        assert_complex_close(
            schedule_transform_value,
            one_interval_output.transform_value,
        );
    }

    #[test]
    fn schedule_cumulant_argument_tangent_matches_finite_difference() {
        let tangent_scale = ComplexScalar::new(0.7, -0.2);
        let tangent = ExponentialAffineTransformInputTangent {
            activity_loadings: ExponentialAffineTransformActivityLoadings::new(
                tangent_scale,
                tangent_scale,
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
            ),
            z_plus: ComplexScalar::new(0.0, 0.0),
            z_minus: ComplexScalar::new(0.0, 0.0),
            news_return_loading: ComplexScalar::new(0.0, 0.0),
        };

        let analytic =
            solve_exponential_affine_schedule_cumulant(&ExponentialAffineScheduleCumulantRequest {
                initial_state: standard_initial_state(),
                initial_long_variance_state: LongVarianceState::unit(),
                intervals: vec![interval_input(base_loadings(), tangent)],
            })
            .expect("schedule cumulant should solve")
            .argument_derivative;

        let perturbation = 1.0e-6;
        let positive = schedule_output_for_loadings(
            perturb_loadings(base_loadings(), tangent_scale, perturbation),
            ExponentialAffineTransformInputTangent::zero(),
        );
        let negative = schedule_output_for_loadings(
            perturb_loadings(base_loadings(), tangent_scale, -perturbation),
            ExponentialAffineTransformInputTangent::zero(),
        );
        let finite_difference = positive
            .subtract(negative)
            .scale(1.0 / (2.0 * perturbation));

        assert_complex_close(analytic, finite_difference);
    }

    #[test]
    fn schedule_cumulant_reports_completed_interval_counts() {
        let mut progress_updates = Vec::new();
        let mut progress_observer = |progress| progress_updates.push(progress);

        solve_exponential_affine_schedule_cumulant_with_progress(
            &ExponentialAffineScheduleCumulantRequest {
                initial_state: standard_initial_state(),
                initial_long_variance_state: LongVarianceState::unit(),
                intervals: vec![
                    interval_input(
                        base_loadings(),
                        ExponentialAffineTransformInputTangent::zero(),
                    ),
                    interval_input(
                        base_loadings(),
                        ExponentialAffineTransformInputTangent::zero(),
                    ),
                ],
            },
            Some(&mut progress_observer),
        )
        .expect("schedule cumulant should solve");

        assert_eq!(progress_updates.len(), 3);
        assert_eq!(progress_updates[0].completed_interval_count, 0);
        assert_eq!(progress_updates[0].total_interval_count, 2);
        assert_eq!(progress_updates[0].current_interval_position, None);
        assert_eq!(progress_updates[1].completed_interval_count, 1);
        assert_eq!(progress_updates[1].current_interval_position, Some(1));
        assert_eq!(progress_updates[2].completed_interval_count, 2);
        assert_eq!(progress_updates[2].current_interval_position, Some(0));
    }

    fn perturb_loadings(
        activity_loadings: ExponentialAffineTransformActivityLoadings,
        tangent_scale: ComplexScalar,
        perturbation: f64,
    ) -> ExponentialAffineTransformActivityLoadings {
        ExponentialAffineTransformActivityLoadings::new(
            activity_loadings
                .u_plus_baseline
                .plus(tangent_scale.scale(perturbation)),
            activity_loadings
                .u_plus_endogenous
                .plus(tangent_scale.scale(perturbation)),
            activity_loadings.u_minus_baseline,
            activity_loadings.u_minus_endogenous,
        )
    }

    fn assert_complex_close(observed: ComplexScalar, expected: ComplexScalar) {
        let tolerance = 1.0e-6;

        assert!(
            (observed.real - expected.real).abs() <= tolerance,
            "observed real {} expected real {}",
            observed.real,
            expected.real,
        );
        assert!(
            (observed.imaginary - expected.imaginary).abs() <= tolerance,
            "observed imaginary {} expected imaginary {}",
            observed.imaginary,
            expected.imaginary,
        );
    }
}
