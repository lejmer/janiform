use janiform_foundations::complex_math::{ComplexScalar, complex_exponential, complex_multiply};

use super::moving_activity_loadings::MovingActivityLoadings;
use super::news_transform::{
    GammaNormalNewsTransformComponents, evaluate_gamma_normal_news_transform,
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
/// - `H_plus_*(D) = z_plus * exp(D · k_plus) - 1 - p_plus_* u_plus_*`
/// - `H_minus_*(D) = z_minus * exp(D · k_minus) - 1 - p_minus_* u_minus_*`
pub fn evaluate_exponential_affine_transform_hamiltonians(
    transform_input: ExponentialAffineTransformInput,
    d_vector: ExponentialAffineTransformDVector,
) -> ExponentialAffineTransformHamiltonians {
    let equation_specification = transform_input.equation_specification;

    let moving_activity_loadings = MovingActivityLoadings::from_total_activity_loadings(
        equation_specification.no_move_event_model,
        transform_input.activity_loadings,
    );

    let d_dot_k_plus = d_vector.dot_loadings(equation_specification.k_plus());
    let d_dot_k_minus = d_vector.dot_loadings(equation_specification.k_minus());

    ExponentialAffineTransformHamiltonians::new(
        evaluate_hamiltonian_component(
            moving_activity_loadings.buy_baseline,
            transform_input.z_plus,
            d_dot_k_plus,
        ),
        evaluate_hamiltonian_component(
            moving_activity_loadings.buy_endogenous,
            transform_input.z_plus,
            d_dot_k_plus,
        ),
        evaluate_hamiltonian_component(
            moving_activity_loadings.sell_baseline,
            transform_input.z_minus,
            d_dot_k_minus,
        ),
        evaluate_hamiltonian_component(
            moving_activity_loadings.sell_endogenous,
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
/// - `C' = seasonality_factor * lambda_bar * (H_plus_baseline + H_minus_baseline) + known_news + surprise_news`
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
        .scale(
            transform_input.seasonality_factor
                * equation_specification.hawkes_kernel_parameters.lambda_bar,
        );

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

    evaluate_gamma_normal_news_transform(GammaNormalNewsTransformComponents {
        gamma_severity_mark_configuration: equation_specification.gamma_severity_mark_configuration,
        news_intensity: transform_input.news_intensity,
        known_component_mark_scales: transform_input.known_news_component_mark_scales,
        surprise_component_mark_scales: transform_input.surprise_news_component_mark_scales,
        activity_loading: d_vector.dot_loadings(equation_specification.r()),
        return_loading: transform_input.news_return_loading,
    })
}
