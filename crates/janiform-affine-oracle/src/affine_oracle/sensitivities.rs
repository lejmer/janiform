use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_foundations::complex_math::{ComplexScalar, complex_exponential, complex_multiply};

use super::contracts::{
    ExponentialAffineTransformDVector, ExponentialAffineTransformInput,
    ExponentialAffineTransformOutput, ExponentialAffineTransformOutputError,
};
use super::news_transform::{
    GammaNormalNewsTransformComponents, evaluate_gamma_normal_news_transform_tangent,
};
use super::rhs::evaluate_exponential_affine_transform_ode_rhs;
use super::runge_kutta::{combine_complex_runge_kutta, combine_d_vector_runge_kutta};
use super::solver::{
    ExponentialAffineTransformIntegratorMethod, ExponentialAffineTransformSolverSettings,
    internally_selected_default_solver_settings,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExponentialAffineTransformOutputSensitivity {
    pub c_at_delta_derivative: ComplexScalar,
    pub d_at_delta_derivative: ExponentialAffineTransformDVector,
    pub transform_value_derivative: ComplexScalar,
}

impl ExponentialAffineTransformOutputSensitivity {
    pub fn plus(self, other: Self) -> Self {
        Self {
            c_at_delta_derivative: self.c_at_delta_derivative.plus(other.c_at_delta_derivative),
            d_at_delta_derivative: self.d_at_delta_derivative.plus(other.d_at_delta_derivative),
            transform_value_derivative: self
                .transform_value_derivative
                .plus(other.transform_value_derivative),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExponentialAffineTransformInputSensitivityOutput {
    pub output: ExponentialAffineTransformOutput,
    pub u_plus_baseline_sensitivity: ExponentialAffineTransformOutputSensitivity,
    pub u_plus_endogenous_sensitivity: ExponentialAffineTransformOutputSensitivity,
    pub u_minus_baseline_sensitivity: ExponentialAffineTransformOutputSensitivity,
    pub u_minus_endogenous_sensitivity: ExponentialAffineTransformOutputSensitivity,
    pub z_plus_sensitivity: ExponentialAffineTransformOutputSensitivity,
    pub z_minus_sensitivity: ExponentialAffineTransformOutputSensitivity,
}

impl ExponentialAffineTransformInputSensitivityOutput {
    pub fn u_plus_total_activity_sensitivity(self) -> ExponentialAffineTransformOutputSensitivity {
        self.u_plus_baseline_sensitivity
            .plus(self.u_plus_endogenous_sensitivity)
    }

    pub fn u_minus_total_activity_sensitivity(self) -> ExponentialAffineTransformOutputSensitivity {
        self.u_minus_baseline_sensitivity
            .plus(self.u_minus_endogenous_sensitivity)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExponentialAffineTransformInputSensitivityError {
    InvalidInput {
        source: super::contracts::ExponentialAffineTransformInputError,
    },
    InvalidSolverSettings {
        source: super::solver::ExponentialAffineTransformSolverSettingsError,
    },
    InvalidOutput {
        source: ExponentialAffineTransformOutputError,
    },
    NonFiniteSensitivity {
        label: &'static str,
        value: ComplexScalar,
    },
}

impl Display for ExponentialAffineTransformInputSensitivityError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput { source } => {
                write!(
                    formatter,
                    "transform input is invalid for sensitivity solve: {source}"
                )
            }
            Self::InvalidSolverSettings { source } => write!(
                formatter,
                "solver settings are invalid for sensitivity solve: {source}"
            ),
            Self::InvalidOutput { source } => write!(
                formatter,
                "base affine transform output is invalid for sensitivity solve: {source}"
            ),
            Self::NonFiniteSensitivity { label, value } => write!(
                formatter,
                "affine transform sensitivity {label} must be finite, found {value:?}"
            ),
        }
    }
}

impl Error for ExponentialAffineTransformInputSensitivityError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidInput { source } => Some(source),
            Self::InvalidSolverSettings { source } => Some(source),
            Self::InvalidOutput { source } => Some(source),
            Self::NonFiniteSensitivity { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ParameterSensitivityState {
    d_vector: ExponentialAffineTransformDVector,
    c_value: ComplexScalar,
}

impl ParameterSensitivityState {
    fn zero() -> Self {
        Self {
            d_vector: ExponentialAffineTransformDVector::zero(),
            c_value: ComplexScalar::new(0.0, 0.0),
        }
    }

    fn add_scaled_rhs(self, rhs: Self, scale: f64) -> Self {
        Self {
            d_vector: self.d_vector.plus(rhs.d_vector.scale(scale)),
            c_value: self.c_value.plus(rhs.c_value.scale(scale)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TransformSensitivityIntegrationState {
    d_vector: ExponentialAffineTransformDVector,
    c_value: ComplexScalar,
    u_plus_baseline: ParameterSensitivityState,
    u_plus_endogenous: ParameterSensitivityState,
    u_minus_baseline: ParameterSensitivityState,
    u_minus_endogenous: ParameterSensitivityState,
    z_plus: ParameterSensitivityState,
    z_minus: ParameterSensitivityState,
}

impl TransformSensitivityIntegrationState {
    fn initial() -> Self {
        Self {
            d_vector: ExponentialAffineTransformDVector::zero(),
            c_value: ComplexScalar::new(0.0, 0.0),
            u_plus_baseline: ParameterSensitivityState::zero(),
            u_plus_endogenous: ParameterSensitivityState::zero(),
            u_minus_baseline: ParameterSensitivityState::zero(),
            u_minus_endogenous: ParameterSensitivityState::zero(),
            z_plus: ParameterSensitivityState::zero(),
            z_minus: ParameterSensitivityState::zero(),
        }
    }

    fn add_scaled_rhs(self, rhs: TransformSensitivityOdeRhs, scale: f64) -> Self {
        Self {
            d_vector: self.d_vector.plus(rhs.d_derivative.scale(scale)),
            c_value: self.c_value.plus(rhs.c_derivative.scale(scale)),
            u_plus_baseline: self
                .u_plus_baseline
                .add_scaled_rhs(rhs.u_plus_baseline, scale),
            u_plus_endogenous: self
                .u_plus_endogenous
                .add_scaled_rhs(rhs.u_plus_endogenous, scale),
            u_minus_baseline: self
                .u_minus_baseline
                .add_scaled_rhs(rhs.u_minus_baseline, scale),
            u_minus_endogenous: self
                .u_minus_endogenous
                .add_scaled_rhs(rhs.u_minus_endogenous, scale),
            z_plus: self.z_plus.add_scaled_rhs(rhs.z_plus, scale),
            z_minus: self.z_minus.add_scaled_rhs(rhs.z_minus, scale),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TransformSensitivityOdeRhs {
    d_derivative: ExponentialAffineTransformDVector,
    c_derivative: ComplexScalar,
    u_plus_baseline: ParameterSensitivityState,
    u_plus_endogenous: ParameterSensitivityState,
    u_minus_baseline: ParameterSensitivityState,
    u_minus_endogenous: ParameterSensitivityState,
    z_plus: ParameterSensitivityState,
    z_minus: ParameterSensitivityState,
}

pub fn solve_exponential_affine_transform_input_sensitivities_with_settings(
    transform_input: ExponentialAffineTransformInput,
    solver_settings: ExponentialAffineTransformSolverSettings,
) -> Result<
    ExponentialAffineTransformInputSensitivityOutput,
    ExponentialAffineTransformInputSensitivityError,
> {
    transform_input.validate().map_err(|source| {
        ExponentialAffineTransformInputSensitivityError::InvalidInput { source }
    })?;
    solver_settings.validate().map_err(|source| {
        ExponentialAffineTransformInputSensitivityError::InvalidSolverSettings { source }
    })?;

    let integrated_state = match solver_settings.integrator_method {
        ExponentialAffineTransformIntegratorMethod::ForwardEuler => {
            run_sensitivity_integration_forward_euler(transform_input, solver_settings)
        }
        ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta => {
            run_sensitivity_integration_runge_kutta(transform_input, solver_settings)
        }
    };

    build_sensitivity_output(transform_input, integrated_state)
}

/// Solve one-interval affine transform input sensitivities using internal stable settings.
pub fn solve_exponential_affine_transform_input_sensitivities(
    transform_input: ExponentialAffineTransformInput,
) -> Result<
    ExponentialAffineTransformInputSensitivityOutput,
    ExponentialAffineTransformInputSensitivityError,
> {
    let solver_settings = internally_selected_default_solver_settings(transform_input);

    solve_exponential_affine_transform_input_sensitivities_with_settings(
        transform_input,
        solver_settings,
    )
}

fn run_sensitivity_integration_forward_euler(
    transform_input: ExponentialAffineTransformInput,
    solver_settings: ExponentialAffineTransformSolverSettings,
) -> TransformSensitivityIntegrationState {
    let step_size = transform_input.delta / solver_settings.step_count as f64;
    let mut integration_state = TransformSensitivityIntegrationState::initial();

    for _ in 0..solver_settings.step_count {
        let rhs = evaluate_transform_sensitivity_ode_rhs(transform_input, integration_state);
        integration_state = integration_state.add_scaled_rhs(rhs, step_size);
    }

    integration_state
}

fn run_sensitivity_integration_runge_kutta(
    transform_input: ExponentialAffineTransformInput,
    solver_settings: ExponentialAffineTransformSolverSettings,
) -> TransformSensitivityIntegrationState {
    let step_size = transform_input.delta / solver_settings.step_count as f64;
    let mut integration_state = TransformSensitivityIntegrationState::initial();

    for _ in 0..solver_settings.step_count {
        let k1 = evaluate_transform_sensitivity_ode_rhs(transform_input, integration_state);
        let k2 = evaluate_transform_sensitivity_ode_rhs(
            transform_input,
            integration_state.add_scaled_rhs(k1, 0.5 * step_size),
        );
        let k3 = evaluate_transform_sensitivity_ode_rhs(
            transform_input,
            integration_state.add_scaled_rhs(k2, 0.5 * step_size),
        );
        let k4 = evaluate_transform_sensitivity_ode_rhs(
            transform_input,
            integration_state.add_scaled_rhs(k3, step_size),
        );

        integration_state =
            integration_state.add_scaled_rhs(weighted_runge_kutta_rhs(k1, k2, k3, k4), step_size);
    }

    integration_state
}

fn weighted_runge_kutta_rhs(
    k1: TransformSensitivityOdeRhs,
    k2: TransformSensitivityOdeRhs,
    k3: TransformSensitivityOdeRhs,
    k4: TransformSensitivityOdeRhs,
) -> TransformSensitivityOdeRhs {
    TransformSensitivityOdeRhs {
        d_derivative: combine_d_vector_runge_kutta(
            k1.d_derivative,
            k2.d_derivative,
            k3.d_derivative,
            k4.d_derivative,
        ),
        c_derivative: combine_complex_runge_kutta(
            k1.c_derivative,
            k2.c_derivative,
            k3.c_derivative,
            k4.c_derivative,
        ),
        u_plus_baseline: combine_parameter_runge_kutta(
            k1.u_plus_baseline,
            k2.u_plus_baseline,
            k3.u_plus_baseline,
            k4.u_plus_baseline,
        ),
        u_plus_endogenous: combine_parameter_runge_kutta(
            k1.u_plus_endogenous,
            k2.u_plus_endogenous,
            k3.u_plus_endogenous,
            k4.u_plus_endogenous,
        ),
        u_minus_baseline: combine_parameter_runge_kutta(
            k1.u_minus_baseline,
            k2.u_minus_baseline,
            k3.u_minus_baseline,
            k4.u_minus_baseline,
        ),
        u_minus_endogenous: combine_parameter_runge_kutta(
            k1.u_minus_endogenous,
            k2.u_minus_endogenous,
            k3.u_minus_endogenous,
            k4.u_minus_endogenous,
        ),
        z_plus: combine_parameter_runge_kutta(k1.z_plus, k2.z_plus, k3.z_plus, k4.z_plus),
        z_minus: combine_parameter_runge_kutta(k1.z_minus, k2.z_minus, k3.z_minus, k4.z_minus),
    }
}

fn combine_parameter_runge_kutta(
    k1: ParameterSensitivityState,
    k2: ParameterSensitivityState,
    k3: ParameterSensitivityState,
    k4: ParameterSensitivityState,
) -> ParameterSensitivityState {
    ParameterSensitivityState {
        d_vector: combine_d_vector_runge_kutta(k1.d_vector, k2.d_vector, k3.d_vector, k4.d_vector),
        c_value: combine_complex_runge_kutta(k1.c_value, k2.c_value, k3.c_value, k4.c_value),
    }
}

fn evaluate_transform_sensitivity_ode_rhs(
    transform_input: ExponentialAffineTransformInput,
    integration_state: TransformSensitivityIntegrationState,
) -> TransformSensitivityOdeRhs {
    let base_rhs =
        evaluate_exponential_affine_transform_ode_rhs(transform_input, integration_state.d_vector);

    TransformSensitivityOdeRhs {
        d_derivative: base_rhs.d_derivative,
        c_derivative: base_rhs.c_derivative,
        u_plus_baseline: evaluate_parameter_sensitivity_ode_rhs(
            transform_input,
            integration_state,
            integration_state.u_plus_baseline,
            DirectSensitivityParameter::UPlusBaseline,
        ),
        u_plus_endogenous: evaluate_parameter_sensitivity_ode_rhs(
            transform_input,
            integration_state,
            integration_state.u_plus_endogenous,
            DirectSensitivityParameter::UPlusEndogenous,
        ),
        u_minus_baseline: evaluate_parameter_sensitivity_ode_rhs(
            transform_input,
            integration_state,
            integration_state.u_minus_baseline,
            DirectSensitivityParameter::UMinusBaseline,
        ),
        u_minus_endogenous: evaluate_parameter_sensitivity_ode_rhs(
            transform_input,
            integration_state,
            integration_state.u_minus_endogenous,
            DirectSensitivityParameter::UMinusEndogenous,
        ),
        z_plus: evaluate_parameter_sensitivity_ode_rhs(
            transform_input,
            integration_state,
            integration_state.z_plus,
            DirectSensitivityParameter::ZPlus,
        ),
        z_minus: evaluate_parameter_sensitivity_ode_rhs(
            transform_input,
            integration_state,
            integration_state.z_minus,
            DirectSensitivityParameter::ZMinus,
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirectSensitivityParameter {
    UPlusBaseline,
    UPlusEndogenous,
    UMinusBaseline,
    UMinusEndogenous,
    ZPlus,
    ZMinus,
}

fn evaluate_parameter_sensitivity_ode_rhs(
    transform_input: ExponentialAffineTransformInput,
    integration_state: TransformSensitivityIntegrationState,
    parameter_state: ParameterSensitivityState,
    direct_parameter: DirectSensitivityParameter,
) -> ParameterSensitivityState {
    let equation_specification = transform_input.equation_specification;

    let beta = equation_specification.hawkes_kernel_parameters.beta;

    let k_plus = equation_specification.k_plus();
    let k_minus = equation_specification.k_minus();

    let d_dot_k_plus = integration_state.d_vector.dot_loadings(k_plus);
    let d_dot_k_minus = integration_state.d_vector.dot_loadings(k_minus);

    let exp_d_dot_k_plus = complex_exponential(d_dot_k_plus);
    let exp_d_dot_k_minus = complex_exponential(d_dot_k_minus);

    let plus_factor = complex_multiply(transform_input.z_plus, exp_d_dot_k_plus);
    let minus_factor = complex_multiply(transform_input.z_minus, exp_d_dot_k_minus);

    let plus_state_derivative =
        complex_multiply(plus_factor, parameter_state.d_vector.dot_loadings(k_plus));
    let minus_state_derivative =
        complex_multiply(minus_factor, parameter_state.d_vector.dot_loadings(k_minus));

    let h_direct =
        hamiltonian_direct_derivatives(direct_parameter, exp_d_dot_k_plus, exp_d_dot_k_minus);

    let h_plus_baseline_derivative = plus_state_derivative.plus(h_direct.h_plus_baseline);
    let h_plus_endogenous_derivative = plus_state_derivative.plus(h_direct.h_plus_endogenous);

    let h_minus_baseline_derivative = minus_state_derivative.plus(h_direct.h_minus_baseline);
    let h_minus_endogenous_derivative = minus_state_derivative.plus(h_direct.h_minus_endogenous);

    let news_derivative = evaluate_news_parameter_sensitivity_derivative(
        transform_input,
        integration_state,
        parameter_state,
    );

    ParameterSensitivityState {
        d_vector: ExponentialAffineTransformDVector::new(
            parameter_state
                .d_vector
                .plus_plus
                .scale(-beta)
                .plus(h_plus_endogenous_derivative),
            parameter_state
                .d_vector
                .plus_minus
                .scale(-beta)
                .plus(h_plus_endogenous_derivative),
            parameter_state
                .d_vector
                .minus_plus
                .scale(-beta)
                .plus(h_minus_endogenous_derivative),
            parameter_state
                .d_vector
                .minus_minus
                .scale(-beta)
                .plus(h_minus_endogenous_derivative),
        ),
        c_value: h_plus_baseline_derivative
            .plus(h_minus_baseline_derivative)
            .scale(
                transform_input.seasonality_factor
                    * equation_specification.hawkes_kernel_parameters.lambda_bar,
            )
            .plus(news_derivative),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct HamiltonianDirectDerivatives {
    h_plus_baseline: ComplexScalar,
    h_plus_endogenous: ComplexScalar,
    h_minus_baseline: ComplexScalar,
    h_minus_endogenous: ComplexScalar,
}

fn hamiltonian_direct_derivatives(
    parameter: DirectSensitivityParameter,
    exp_d_dot_k_plus: ComplexScalar,
    exp_d_dot_k_minus: ComplexScalar,
) -> HamiltonianDirectDerivatives {
    let zero = ComplexScalar::new(0.0, 0.0);
    let negative_one = ComplexScalar::new(-1.0, 0.0);

    match parameter {
        DirectSensitivityParameter::UPlusBaseline => HamiltonianDirectDerivatives {
            h_plus_baseline: negative_one,
            h_plus_endogenous: zero,
            h_minus_baseline: zero,
            h_minus_endogenous: zero,
        },
        DirectSensitivityParameter::UPlusEndogenous => HamiltonianDirectDerivatives {
            h_plus_baseline: zero,
            h_plus_endogenous: negative_one,
            h_minus_baseline: zero,
            h_minus_endogenous: zero,
        },
        DirectSensitivityParameter::UMinusBaseline => HamiltonianDirectDerivatives {
            h_plus_baseline: zero,
            h_plus_endogenous: zero,
            h_minus_baseline: negative_one,
            h_minus_endogenous: zero,
        },
        DirectSensitivityParameter::UMinusEndogenous => HamiltonianDirectDerivatives {
            h_plus_baseline: zero,
            h_plus_endogenous: zero,
            h_minus_baseline: zero,
            h_minus_endogenous: negative_one,
        },
        DirectSensitivityParameter::ZPlus => HamiltonianDirectDerivatives {
            h_plus_baseline: exp_d_dot_k_plus,
            h_plus_endogenous: exp_d_dot_k_plus,
            h_minus_baseline: zero,
            h_minus_endogenous: zero,
        },
        DirectSensitivityParameter::ZMinus => HamiltonianDirectDerivatives {
            h_plus_baseline: zero,
            h_plus_endogenous: zero,
            h_minus_baseline: exp_d_dot_k_minus,
            h_minus_endogenous: exp_d_dot_k_minus,
        },
    }
}

fn evaluate_news_parameter_sensitivity_derivative(
    transform_input: ExponentialAffineTransformInput,
    integration_state: TransformSensitivityIntegrationState,
    parameter_state: ParameterSensitivityState,
) -> ComplexScalar {
    let equation_specification = transform_input.equation_specification;

    let activity_loading = integration_state
        .d_vector
        .dot_loadings(equation_specification.r());

    let activity_loading_tangent = parameter_state
        .d_vector
        .dot_loadings(equation_specification.r());

    evaluate_gamma_normal_news_transform_tangent(
        GammaNormalNewsTransformComponents {
            gamma_severity_mark_configuration: equation_specification
                .gamma_severity_mark_configuration,
            news_intensity: transform_input.news_intensity,
            known_component_mark_scales: transform_input.known_news_component_mark_scales,
            surprise_component_mark_scales: transform_input.surprise_news_component_mark_scales,
            activity_loading,
            return_loading: transform_input.news_return_loading,
        },
        activity_loading_tangent,
        ComplexScalar::new(0.0, 0.0),
    )
}

fn build_sensitivity_output(
    transform_input: ExponentialAffineTransformInput,
    integration_state: TransformSensitivityIntegrationState,
) -> Result<
    ExponentialAffineTransformInputSensitivityOutput,
    ExponentialAffineTransformInputSensitivityError,
> {
    let transform_value = complex_exponential(
        integration_state.c_value.plus(
            integration_state
                .d_vector
                .dot_state(transform_input.initial_state),
        ),
    );

    let output = ExponentialAffineTransformOutput::try_new(
        integration_state.c_value,
        integration_state.d_vector,
        transform_value,
    )
    .map_err(|source| ExponentialAffineTransformInputSensitivityError::InvalidOutput { source })?;

    let u_plus_baseline_sensitivity = build_parameter_sensitivity_output(
        transform_input,
        transform_value,
        integration_state.u_plus_baseline,
    );

    let u_plus_endogenous_sensitivity = build_parameter_sensitivity_output(
        transform_input,
        transform_value,
        integration_state.u_plus_endogenous,
    );

    let u_minus_baseline_sensitivity = build_parameter_sensitivity_output(
        transform_input,
        transform_value,
        integration_state.u_minus_baseline,
    );

    let u_minus_endogenous_sensitivity = build_parameter_sensitivity_output(
        transform_input,
        transform_value,
        integration_state.u_minus_endogenous,
    );

    let z_plus_sensitivity = build_parameter_sensitivity_output(
        transform_input,
        transform_value,
        integration_state.z_plus,
    );

    let z_minus_sensitivity = build_parameter_sensitivity_output(
        transform_input,
        transform_value,
        integration_state.z_minus,
    );

    validate_output_sensitivity("u_plus_baseline", u_plus_baseline_sensitivity)?;
    validate_output_sensitivity("u_plus_endogenous", u_plus_endogenous_sensitivity)?;
    validate_output_sensitivity("u_minus_baseline", u_minus_baseline_sensitivity)?;
    validate_output_sensitivity("u_minus_endogenous", u_minus_endogenous_sensitivity)?;
    validate_output_sensitivity("z_plus", z_plus_sensitivity)?;
    validate_output_sensitivity("z_minus", z_minus_sensitivity)?;

    Ok(ExponentialAffineTransformInputSensitivityOutput {
        output,
        u_plus_baseline_sensitivity,
        u_plus_endogenous_sensitivity,
        u_minus_baseline_sensitivity,
        u_minus_endogenous_sensitivity,
        z_plus_sensitivity,
        z_minus_sensitivity,
    })
}

fn build_parameter_sensitivity_output(
    transform_input: ExponentialAffineTransformInput,
    transform_value: ComplexScalar,
    parameter_state: ParameterSensitivityState,
) -> ExponentialAffineTransformOutputSensitivity {
    let transform_value_derivative = complex_multiply(
        transform_value,
        parameter_state.c_value.plus(
            parameter_state
                .d_vector
                .dot_state(transform_input.initial_state),
        ),
    );

    ExponentialAffineTransformOutputSensitivity {
        c_at_delta_derivative: parameter_state.c_value,
        d_at_delta_derivative: parameter_state.d_vector,
        transform_value_derivative,
    }
}

fn validate_output_sensitivity(
    label: &'static str,
    sensitivity: ExponentialAffineTransformOutputSensitivity,
) -> Result<(), ExponentialAffineTransformInputSensitivityError> {
    validate_finite_complex_sensitivity(label, sensitivity.transform_value_derivative)?;
    validate_finite_d_vector_sensitivity(label, sensitivity.d_at_delta_derivative)?;
    validate_finite_complex_sensitivity(label, sensitivity.c_at_delta_derivative)?;

    Ok(())
}

fn validate_finite_d_vector_sensitivity(
    label: &'static str,
    sensitivity: ExponentialAffineTransformDVector,
) -> Result<(), ExponentialAffineTransformInputSensitivityError> {
    validate_finite_complex_sensitivity(label, sensitivity.plus_plus)?;
    validate_finite_complex_sensitivity(label, sensitivity.plus_minus)?;
    validate_finite_complex_sensitivity(label, sensitivity.minus_plus)?;
    validate_finite_complex_sensitivity(label, sensitivity.minus_minus)?;

    Ok(())
}

fn validate_finite_complex_sensitivity(
    label: &'static str,
    value: ComplexScalar,
) -> Result<(), ExponentialAffineTransformInputSensitivityError> {
    if !value.is_finite() {
        return Err(
            ExponentialAffineTransformInputSensitivityError::NonFiniteSensitivity { label, value },
        );
    }

    Ok(())
}
