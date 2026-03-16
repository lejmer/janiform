use janiform_foundations::complex_math::{
    ComplexScalar, complex_exponential, complex_multiply, complex_pow_real_exponent_principal,
};

use super::{ExponentialAffineTransformDVector, ExponentialAffineTransformInput};

/// Shared Hamiltonian values evaluated at one ODE state vector `D`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExponentialAffineTransformHamiltonians {
    pub h_plus: ComplexScalar,
    pub h_minus: ComplexScalar,
}

impl ExponentialAffineTransformHamiltonians {
    pub const fn new(h_plus: ComplexScalar, h_minus: ComplexScalar) -> Self {
        Self { h_plus, h_minus }
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
/// - `H_plus(D) = z_plus * exp(D · k_plus) - 1 - u_plus`
/// - `H_minus(D) = z_minus * exp(D · k_minus) - 1 - u_minus`
pub fn evaluate_exponential_affine_transform_hamiltonians(
    transform_input: ExponentialAffineTransformInput,
    d_vector: ExponentialAffineTransformDVector,
) -> ExponentialAffineTransformHamiltonians {
    let equation_specification = transform_input.equation_specification;
    let d_dot_k_plus = d_vector.dot_loadings(equation_specification.k_plus());
    let d_dot_k_minus = d_vector.dot_loadings(equation_specification.k_minus());

    ExponentialAffineTransformHamiltonians::new(
        evaluate_hamiltonian_component(
            transform_input.u_plus,
            transform_input.z_plus,
            d_dot_k_plus,
        ),
        evaluate_hamiltonian_component(
            transform_input.u_minus,
            transform_input.z_minus,
            d_dot_k_minus,
        ),
    )
}

/// Evaluate the ODE RHS for the 4 `D_ij` equations and `C` equation.
///
/// Equation reference from compact model spec section 8:
/// - `D_++' = -beta * D_++ + s_h * H_plus`
/// - `D_+-' = -beta * D_+- + s_h * H_plus`
/// - `D_-+' = -beta * D_-+ + s_h * H_minus`
/// - `D_--' = -beta * D_-- + s_h * H_minus`
/// - `C' = s_h * lambda_bar * (H_plus + H_minus) + nu_h_total * ((1 - theta_U * (D · r))^(-k_U) - 1)`
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
            .add(hamiltonians.h_plus.scale(transform_input.s_h)),
        d_vector
            .plus_minus
            .scale(-beta)
            .add(hamiltonians.h_plus.scale(transform_input.s_h)),
        d_vector
            .minus_plus
            .scale(-beta)
            .add(hamiltonians.h_minus.scale(transform_input.s_h)),
        d_vector
            .minus_minus
            .scale(-beta)
            .add(hamiltonians.h_minus.scale(transform_input.s_h)),
    );

    let trading_term = hamiltonians
        .h_plus
        .add(hamiltonians.h_minus)
        .scale(transform_input.s_h * equation_specification.hawkes_kernel_parameters.lambda_bar);
    let news_term = evaluate_gamma_moment_news_term(transform_input, d_vector);

    ExponentialAffineTransformOdeRhs::new(d_derivative, trading_term.add(news_term))
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
