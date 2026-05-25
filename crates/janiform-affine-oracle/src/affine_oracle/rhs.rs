use janiform_foundations::complex_math::{
    ComplexScalar, complex_exponential, complex_multiply, complex_pow_real_exponent_principal,
};

use super::{ExponentialAffineTransformDVector, ExponentialAffineTransformInput};

/// Shared Hamiltonian values evaluated at one ODE state vector `D`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExponentialAffineTransformHamiltonians {
    pub h_plus_baseline: ComplexScalar,
    pub h_plus_endogenous: ComplexScalar,
    pub h_minus_baseline: ComplexScalar,
    pub h_minus_endogenous: ComplexScalar,
}

impl ExponentialAffineTransformHamiltonians {
    pub const fn new(
        h_plus_baseline: ComplexScalar,
        h_plus_endogenous: ComplexScalar,
        h_minus_baseline: ComplexScalar,
        h_minus_endogenous: ComplexScalar,
    ) -> Self {
        Self {
            h_plus_baseline,
            h_plus_endogenous,
            h_minus_baseline,
            h_minus_endogenous,
        }
    }
}

/// One RHS evaluation for the exponential-affine ODE system.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExponentialAffineTransformOdeRhs {
    pub d_derivative: ExponentialAffineTransformDVector,
    pub c_derivative: ComplexScalar,
}

impl ExponentialAffineTransformOdeRhs {
    pub const fn new(
        d_derivative: ExponentialAffineTransformDVector,
        c_derivative: ComplexScalar,
    ) -> Self {
        Self {
            d_derivative,
            c_derivative,
        }
    }
}

/// Evaluate shared `H_plus` and `H_minus` Hamiltonians at one `D` state.
///
/// Equation reference:
/// - `H_plus_*(D) = z_plus * exp(D · k_plus) - 1 - u_plus_*`
/// - `H_minus_*(D) = z_minus * exp(D · k_minus) - 1 - u_minus_*`
pub fn evaluate_exponential_affine_transform_hamiltonians(
    transform_input: ExponentialAffineTransformInput,
    d_vector: ExponentialAffineTransformDVector,
) -> ExponentialAffineTransformHamiltonians {
    let equation_specification = transform_input.equation_specification;
    let activity_loadings = transform_input.activity_loadings;

    let d_dot_k_plus = d_vector.dot_loadings(equation_specification.k_plus());
    let d_dot_k_minus = d_vector.dot_loadings(equation_specification.k_minus());

    ExponentialAffineTransformHamiltonians::new(
        evaluate_hamiltonian_component(
            activity_loadings.u_plus_baseline,
            transform_input.z_plus,
            d_dot_k_plus,
        ),
        evaluate_hamiltonian_component(
            activity_loadings.u_plus_endogenous,
            transform_input.z_plus,
            d_dot_k_plus,
        ),
        evaluate_hamiltonian_component(
            activity_loadings.u_minus_baseline,
            transform_input.z_minus,
            d_dot_k_minus,
        ),
        evaluate_hamiltonian_component(
            activity_loadings.u_minus_endogenous,
            transform_input.z_minus,
            d_dot_k_minus,
        ),
    )
}

/// Evaluate the ODE RHS for the 4 `D_ij` equations and `C` equation.
///
/// Equation reference from compact model spec section 9:
/// - `D_++' = -beta * D_++ + H_plus_endogenous`
/// - `D_+-' = -beta * D_+- + H_plus_endogenous`
/// - `D_-+' = -beta * D_-+ + H_minus_endogenous`
/// - `D_--' = -beta * D_-- + H_minus_endogenous`
/// - `C' = s_h * lambda_bar * (H_plus_baseline + H_minus_baseline) + nu_h_total * ((1 - theta_U * (D · r))^(-k_U) - 1)`
pub fn evaluate_exponential_affine_transform_ode_rhs(
    transform_input: ExponentialAffineTransformInput,
    d_vector: ExponentialAffineTransformDVector,
) -> ExponentialAffineTransformOdeRhs {
    let equation_specification = transform_input.equation_specification;

    let hamiltonians =
        evaluate_exponential_affine_transform_hamiltonians(transform_input, d_vector);

    let beta = equation_specification.hawkes_kernel_parameters.beta;

    let d_derivative = ExponentialAffineTransformDVector::new(
        d_vector
            .plus_plus
            .scale(-beta)
            .plus(hamiltonians.h_plus_endogenous),
        d_vector
            .plus_minus
            .scale(-beta)
            .plus(hamiltonians.h_plus_endogenous),
        d_vector
            .minus_plus
            .scale(-beta)
            .plus(hamiltonians.h_minus_endogenous),
        d_vector
            .minus_minus
            .scale(-beta)
            .plus(hamiltonians.h_minus_endogenous),
    );

    let trading_term = hamiltonians
        .h_plus_baseline
        .plus(hamiltonians.h_minus_baseline)
        .scale(transform_input.s_h * equation_specification.hawkes_kernel_parameters.lambda_bar);

    let news_term = evaluate_gamma_moment_news_term(transform_input, d_vector);

    ExponentialAffineTransformOdeRhs::new(d_derivative, trading_term.plus(news_term))
}
fn evaluate_hamiltonian_component(
    u_component: ComplexScalar,
    z_component: ComplexScalar,
    d_dot_k_component: ComplexScalar,
) -> ComplexScalar {
    complex_multiply(z_component, complex_exponential(d_dot_k_component))
        .subtract(ComplexScalar::new(1.0, 0.0))
        .subtract(u_component)
}

fn evaluate_gamma_moment_news_term(
    transform_input: ExponentialAffineTransformInput,
    d_vector: ExponentialAffineTransformDVector,
) -> ComplexScalar {
    let equation_specification = transform_input.equation_specification;
    let gamma_configuration = equation_specification.gamma_severity_mark_configuration;

    let one_minus_theta_d_dot_r = ComplexScalar::new(1.0, 0.0).subtract(
        d_vector
            .dot_loadings(equation_specification.r())
            .scale(gamma_configuration.scale_theta),
    );

    let gamma_moment =
        complex_pow_real_exponent_principal(one_minus_theta_d_dot_r, -gamma_configuration.shape_k);

    gamma_moment
        .subtract(ComplexScalar::new(1.0, 0.0))
        .scale(transform_input.news_intensity.total_nu_h())
}
