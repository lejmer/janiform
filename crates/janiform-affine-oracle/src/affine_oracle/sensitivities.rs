use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_foundations::complex_math::{
    ComplexScalar, complex_exponential, complex_multiply, complex_pow_real_exponent_principal,
};

use super::contracts::{
    ExponentialAffineTransformDVector, ExponentialAffineTransformInput,
    ExponentialAffineTransformOutput, ExponentialAffineTransformOutputError,
};
use super::rhs::evaluate_exponential_affine_transform_hamiltonians;
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExponentialAffineTransformInputSensitivityOutput {
    pub output: ExponentialAffineTransformOutput,
    pub u_plus_sensitivity: ExponentialAffineTransformOutputSensitivity,
    pub u_minus_sensitivity: ExponentialAffineTransformOutputSensitivity,
    pub z_plus_sensitivity: ExponentialAffineTransformOutputSensitivity,
    pub z_minus_sensitivity: ExponentialAffineTransformOutputSensitivity,
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
struct TransformSensitivityIntegrationState {
    d_vector: ExponentialAffineTransformDVector,
    c_value: ComplexScalar,
    u_plus_d_vector: ExponentialAffineTransformDVector,
    u_plus_c_value: ComplexScalar,
    u_minus_d_vector: ExponentialAffineTransformDVector,
    u_minus_c_value: ComplexScalar,
    z_plus_d_vector: ExponentialAffineTransformDVector,
    z_plus_c_value: ComplexScalar,
    z_minus_d_vector: ExponentialAffineTransformDVector,
    z_minus_c_value: ComplexScalar,
}

impl TransformSensitivityIntegrationState {
    fn initial() -> Self {
        Self {
            d_vector: ExponentialAffineTransformDVector::new(
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
            ),
            c_value: ComplexScalar::new(0.0, 0.0),
            u_plus_d_vector: ExponentialAffineTransformDVector::new(
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
            ),
            u_plus_c_value: ComplexScalar::new(0.0, 0.0),
            u_minus_d_vector: ExponentialAffineTransformDVector::new(
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
            ),
            u_minus_c_value: ComplexScalar::new(0.0, 0.0),
            z_plus_d_vector: ExponentialAffineTransformDVector::new(
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
            ),
            z_plus_c_value: ComplexScalar::new(0.0, 0.0),
            z_minus_d_vector: ExponentialAffineTransformDVector::new(
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
            ),
            z_minus_c_value: ComplexScalar::new(0.0, 0.0),
        }
    }

    fn add_scaled_rhs(self, rhs: TransformSensitivityOdeRhs, scale: f64) -> Self {
        Self {
            d_vector: self.d_vector.plus(rhs.d_derivative.scale(scale)),
            c_value: self.c_value.plus(rhs.c_derivative.scale(scale)),
            u_plus_d_vector: self
                .u_plus_d_vector
                .plus(rhs.u_plus_d_derivative.scale(scale)),
            u_plus_c_value: self
                .u_plus_c_value
                .plus(rhs.u_plus_c_derivative.scale(scale)),
            u_minus_d_vector: self
                .u_minus_d_vector
                .plus(rhs.u_minus_d_derivative.scale(scale)),
            u_minus_c_value: self
                .u_minus_c_value
                .plus(rhs.u_minus_c_derivative.scale(scale)),
            z_plus_d_vector: self
                .z_plus_d_vector
                .plus(rhs.z_plus_d_derivative.scale(scale)),
            z_plus_c_value: self
                .z_plus_c_value
                .plus(rhs.z_plus_c_derivative.scale(scale)),
            z_minus_d_vector: self
                .z_minus_d_vector
                .plus(rhs.z_minus_d_derivative.scale(scale)),
            z_minus_c_value: self
                .z_minus_c_value
                .plus(rhs.z_minus_c_derivative.scale(scale)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TransformSensitivityOdeRhs {
    d_derivative: ExponentialAffineTransformDVector,
    c_derivative: ComplexScalar,
    u_plus_d_derivative: ExponentialAffineTransformDVector,
    u_plus_c_derivative: ComplexScalar,
    u_minus_d_derivative: ExponentialAffineTransformDVector,
    u_minus_c_derivative: ComplexScalar,
    z_plus_d_derivative: ExponentialAffineTransformDVector,
    z_plus_c_derivative: ComplexScalar,
    z_minus_d_derivative: ExponentialAffineTransformDVector,
    z_minus_c_derivative: ComplexScalar,
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

        integration_state = integration_state.add_scaled_rhs(
            TransformSensitivityOdeRhs {
                d_derivative: k1
                    .d_derivative
                    .plus(k2.d_derivative.scale(2.0))
                    .plus(k3.d_derivative.scale(2.0))
                    .plus(k4.d_derivative)
                    .scale(1.0 / 6.0),
                c_derivative: k1
                    .c_derivative
                    .plus(k2.c_derivative.scale(2.0))
                    .plus(k3.c_derivative.scale(2.0))
                    .plus(k4.c_derivative)
                    .scale(1.0 / 6.0),
                u_plus_d_derivative: k1
                    .u_plus_d_derivative
                    .plus(k2.u_plus_d_derivative.scale(2.0))
                    .plus(k3.u_plus_d_derivative.scale(2.0))
                    .plus(k4.u_plus_d_derivative)
                    .scale(1.0 / 6.0),
                u_plus_c_derivative: k1
                    .u_plus_c_derivative
                    .plus(k2.u_plus_c_derivative.scale(2.0))
                    .plus(k3.u_plus_c_derivative.scale(2.0))
                    .plus(k4.u_plus_c_derivative)
                    .scale(1.0 / 6.0),
                u_minus_d_derivative: k1
                    .u_minus_d_derivative
                    .plus(k2.u_minus_d_derivative.scale(2.0))
                    .plus(k3.u_minus_d_derivative.scale(2.0))
                    .plus(k4.u_minus_d_derivative)
                    .scale(1.0 / 6.0),
                u_minus_c_derivative: k1
                    .u_minus_c_derivative
                    .plus(k2.u_minus_c_derivative.scale(2.0))
                    .plus(k3.u_minus_c_derivative.scale(2.0))
                    .plus(k4.u_minus_c_derivative)
                    .scale(1.0 / 6.0),
                z_plus_d_derivative: k1
                    .z_plus_d_derivative
                    .plus(k2.z_plus_d_derivative.scale(2.0))
                    .plus(k3.z_plus_d_derivative.scale(2.0))
                    .plus(k4.z_plus_d_derivative)
                    .scale(1.0 / 6.0),
                z_plus_c_derivative: k1
                    .z_plus_c_derivative
                    .plus(k2.z_plus_c_derivative.scale(2.0))
                    .plus(k3.z_plus_c_derivative.scale(2.0))
                    .plus(k4.z_plus_c_derivative)
                    .scale(1.0 / 6.0),
                z_minus_d_derivative: k1
                    .z_minus_d_derivative
                    .plus(k2.z_minus_d_derivative.scale(2.0))
                    .plus(k3.z_minus_d_derivative.scale(2.0))
                    .plus(k4.z_minus_d_derivative)
                    .scale(1.0 / 6.0),
                z_minus_c_derivative: k1
                    .z_minus_c_derivative
                    .plus(k2.z_minus_c_derivative.scale(2.0))
                    .plus(k3.z_minus_c_derivative.scale(2.0))
                    .plus(k4.z_minus_c_derivative)
                    .scale(1.0 / 6.0),
            },
            step_size,
        );
    }

    integration_state
}

fn evaluate_transform_sensitivity_ode_rhs(
    transform_input: ExponentialAffineTransformInput,
    integration_state: TransformSensitivityIntegrationState,
) -> TransformSensitivityOdeRhs {
    let equation_specification = transform_input.equation_specification;

    let hamiltonians = evaluate_exponential_affine_transform_hamiltonians(
        transform_input,
        integration_state.d_vector,
    );

    let beta = equation_specification.hawkes_kernel_parameters.beta;

    let k_plus = equation_specification.k_plus();
    let k_minus = equation_specification.k_minus();

    let r = equation_specification.r();

    let d_dot_k_plus = integration_state.d_vector.dot_loadings(k_plus);
    let d_dot_k_minus = integration_state.d_vector.dot_loadings(k_minus);

    let plus_factor = complex_multiply(transform_input.z_plus, complex_exponential(d_dot_k_plus));
    let minus_factor =
        complex_multiply(transform_input.z_minus, complex_exponential(d_dot_k_minus));

    let u_plus_h_plus_derivative = complex_multiply(
        plus_factor,
        integration_state.u_plus_d_vector.dot_loadings(k_plus),
    )
    .subtract(ComplexScalar::new(1.0, 0.0));

    let u_plus_h_minus_derivative = complex_multiply(
        minus_factor,
        integration_state.u_plus_d_vector.dot_loadings(k_minus),
    );

    let u_minus_h_plus_derivative = complex_multiply(
        plus_factor,
        integration_state.u_minus_d_vector.dot_loadings(k_plus),
    );

    let u_minus_h_minus_derivative = complex_multiply(
        minus_factor,
        integration_state.u_minus_d_vector.dot_loadings(k_minus),
    )
    .subtract(ComplexScalar::new(1.0, 0.0));

    let z_plus_h_plus_derivative = complex_exponential(d_dot_k_plus).plus(complex_multiply(
        plus_factor,
        integration_state.z_plus_d_vector.dot_loadings(k_plus),
    ));

    let z_plus_h_minus_derivative = complex_multiply(
        minus_factor,
        integration_state.z_plus_d_vector.dot_loadings(k_minus),
    );

    let z_minus_h_plus_derivative = complex_multiply(
        plus_factor,
        integration_state.z_minus_d_vector.dot_loadings(k_plus),
    );

    let z_minus_h_minus_derivative = complex_exponential(d_dot_k_minus).plus(complex_multiply(
        minus_factor,
        integration_state.z_minus_d_vector.dot_loadings(k_minus),
    ));

    let gamma_configuration = equation_specification.gamma_severity_mark_configuration;

    let one_minus_theta_d_dot_r = ComplexScalar::new(1.0, 0.0).subtract(
        integration_state
            .d_vector
            .dot_loadings(r)
            .scale(gamma_configuration.scale_theta),
    );

    let gamma_power = complex_pow_real_exponent_principal(
        one_minus_theta_d_dot_r,
        -(gamma_configuration.shape_k + 1.0),
    );

    let gamma_prefactor = transform_input.news_intensity.total_nu_h()
        * gamma_configuration.shape_k
        * gamma_configuration.scale_theta;

    let u_plus_news_derivative = complex_multiply(
        gamma_power.scale(gamma_prefactor),
        integration_state.u_plus_d_vector.dot_loadings(r),
    );

    let u_minus_news_derivative = complex_multiply(
        gamma_power.scale(gamma_prefactor),
        integration_state.u_minus_d_vector.dot_loadings(r),
    );

    let z_plus_news_derivative = complex_multiply(
        gamma_power.scale(gamma_prefactor),
        integration_state.z_plus_d_vector.dot_loadings(r),
    );

    let z_minus_news_derivative = complex_multiply(
        gamma_power.scale(gamma_prefactor),
        integration_state.z_minus_d_vector.dot_loadings(r),
    );

    TransformSensitivityOdeRhs {
        d_derivative: ExponentialAffineTransformDVector::new(
            integration_state
                .d_vector
                .plus_plus
                .scale(-beta)
                .plus(hamiltonians.h_plus),
            integration_state
                .d_vector
                .plus_minus
                .scale(-beta)
                .plus(hamiltonians.h_plus),
            integration_state
                .d_vector
                .minus_plus
                .scale(-beta)
                .plus(hamiltonians.h_minus),
            integration_state
                .d_vector
                .minus_minus
                .scale(-beta)
                .plus(hamiltonians.h_minus),
        ),
        c_derivative: hamiltonians
            .h_plus
            .plus(hamiltonians.h_minus)
            .scale(transform_input.s_h * equation_specification.hawkes_kernel_parameters.lambda_bar)
            .plus(
                complex_pow_real_exponent_principal(
                    one_minus_theta_d_dot_r,
                    -gamma_configuration.shape_k,
                )
                .subtract(ComplexScalar::new(1.0, 0.0))
                .scale(transform_input.news_intensity.total_nu_h()),
            ),
        u_plus_d_derivative: ExponentialAffineTransformDVector::new(
            integration_state
                .u_plus_d_vector
                .plus_plus
                .scale(-beta)
                .plus(u_plus_h_plus_derivative),
            integration_state
                .u_plus_d_vector
                .plus_minus
                .scale(-beta)
                .plus(u_plus_h_plus_derivative),
            integration_state
                .u_plus_d_vector
                .minus_plus
                .scale(-beta)
                .plus(u_plus_h_minus_derivative),
            integration_state
                .u_plus_d_vector
                .minus_minus
                .scale(-beta)
                .plus(u_plus_h_minus_derivative),
        ),
        u_plus_c_derivative: u_plus_h_plus_derivative
            .plus(u_plus_h_minus_derivative)
            .scale(transform_input.s_h * equation_specification.hawkes_kernel_parameters.lambda_bar)
            .plus(u_plus_news_derivative),
        u_minus_d_derivative: ExponentialAffineTransformDVector::new(
            integration_state
                .u_minus_d_vector
                .plus_plus
                .scale(-beta)
                .plus(u_minus_h_plus_derivative),
            integration_state
                .u_minus_d_vector
                .plus_minus
                .scale(-beta)
                .plus(u_minus_h_plus_derivative),
            integration_state
                .u_minus_d_vector
                .minus_plus
                .scale(-beta)
                .plus(u_minus_h_minus_derivative),
            integration_state
                .u_minus_d_vector
                .minus_minus
                .scale(-beta)
                .plus(u_minus_h_minus_derivative),
        ),
        u_minus_c_derivative: u_minus_h_plus_derivative
            .plus(u_minus_h_minus_derivative)
            .scale(transform_input.s_h * equation_specification.hawkes_kernel_parameters.lambda_bar)
            .plus(u_minus_news_derivative),
        z_plus_d_derivative: ExponentialAffineTransformDVector::new(
            integration_state
                .z_plus_d_vector
                .plus_plus
                .scale(-beta)
                .plus(z_plus_h_plus_derivative),
            integration_state
                .z_plus_d_vector
                .plus_minus
                .scale(-beta)
                .plus(z_plus_h_plus_derivative),
            integration_state
                .z_plus_d_vector
                .minus_plus
                .scale(-beta)
                .plus(z_plus_h_minus_derivative),
            integration_state
                .z_plus_d_vector
                .minus_minus
                .scale(-beta)
                .plus(z_plus_h_minus_derivative),
        ),
        z_plus_c_derivative: z_plus_h_plus_derivative
            .plus(z_plus_h_minus_derivative)
            .scale(transform_input.s_h * equation_specification.hawkes_kernel_parameters.lambda_bar)
            .plus(z_plus_news_derivative),
        z_minus_d_derivative: ExponentialAffineTransformDVector::new(
            integration_state
                .z_minus_d_vector
                .plus_plus
                .scale(-beta)
                .plus(z_minus_h_plus_derivative),
            integration_state
                .z_minus_d_vector
                .plus_minus
                .scale(-beta)
                .plus(z_minus_h_plus_derivative),
            integration_state
                .z_minus_d_vector
                .minus_plus
                .scale(-beta)
                .plus(z_minus_h_minus_derivative),
            integration_state
                .z_minus_d_vector
                .minus_minus
                .scale(-beta)
                .plus(z_minus_h_minus_derivative),
        ),
        z_minus_c_derivative: z_minus_h_plus_derivative
            .plus(z_minus_h_minus_derivative)
            .scale(transform_input.s_h * equation_specification.hawkes_kernel_parameters.lambda_bar)
            .plus(z_minus_news_derivative),
    }
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

    let u_plus_transform_value_derivative = complex_multiply(
        transform_value,
        integration_state.u_plus_c_value.plus(
            integration_state
                .u_plus_d_vector
                .dot_state(transform_input.initial_state),
        ),
    );

    let u_minus_transform_value_derivative = complex_multiply(
        transform_value,
        integration_state.u_minus_c_value.plus(
            integration_state
                .u_minus_d_vector
                .dot_state(transform_input.initial_state),
        ),
    );

    let z_plus_transform_value_derivative = complex_multiply(
        transform_value,
        integration_state.z_plus_c_value.plus(
            integration_state
                .z_plus_d_vector
                .dot_state(transform_input.initial_state),
        ),
    );

    let z_minus_transform_value_derivative = complex_multiply(
        transform_value,
        integration_state.z_minus_c_value.plus(
            integration_state
                .z_minus_d_vector
                .dot_state(transform_input.initial_state),
        ),
    );

    validate_finite_complex_sensitivity(
        "u_plus.transform_value",
        u_plus_transform_value_derivative,
    )?;

    validate_finite_complex_sensitivity(
        "u_minus.transform_value",
        u_minus_transform_value_derivative,
    )?;

    validate_finite_complex_sensitivity(
        "z_plus.transform_value",
        z_plus_transform_value_derivative,
    )?;

    validate_finite_complex_sensitivity(
        "z_minus.transform_value",
        z_minus_transform_value_derivative,
    )?;

    validate_finite_d_vector_sensitivity("u_plus.d_at_delta", integration_state.u_plus_d_vector)?;
    validate_finite_d_vector_sensitivity("u_minus.d_at_delta", integration_state.u_minus_d_vector)?;
    validate_finite_d_vector_sensitivity("z_plus.d_at_delta", integration_state.z_plus_d_vector)?;
    validate_finite_d_vector_sensitivity("z_minus.d_at_delta", integration_state.z_minus_d_vector)?;

    validate_finite_complex_sensitivity("u_plus.c_at_delta", integration_state.u_plus_c_value)?;
    validate_finite_complex_sensitivity("u_minus.c_at_delta", integration_state.u_minus_c_value)?;
    validate_finite_complex_sensitivity("z_plus.c_at_delta", integration_state.z_plus_c_value)?;
    validate_finite_complex_sensitivity("z_minus.c_at_delta", integration_state.z_minus_c_value)?;

    Ok(ExponentialAffineTransformInputSensitivityOutput {
        output,
        u_plus_sensitivity: ExponentialAffineTransformOutputSensitivity {
            c_at_delta_derivative: integration_state.u_plus_c_value,
            d_at_delta_derivative: integration_state.u_plus_d_vector,
            transform_value_derivative: u_plus_transform_value_derivative,
        },
        u_minus_sensitivity: ExponentialAffineTransformOutputSensitivity {
            c_at_delta_derivative: integration_state.u_minus_c_value,
            d_at_delta_derivative: integration_state.u_minus_d_vector,
            transform_value_derivative: u_minus_transform_value_derivative,
        },
        z_plus_sensitivity: ExponentialAffineTransformOutputSensitivity {
            c_at_delta_derivative: integration_state.z_plus_c_value,
            d_at_delta_derivative: integration_state.z_plus_d_vector,
            transform_value_derivative: z_plus_transform_value_derivative,
        },
        z_minus_sensitivity: ExponentialAffineTransformOutputSensitivity {
            c_at_delta_derivative: integration_state.z_minus_c_value,
            d_at_delta_derivative: integration_state.z_minus_d_vector,
            transform_value_derivative: z_minus_transform_value_derivative,
        },
    })
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
