//! Exponential-affine one-interval transform equation models and RHS evaluation.
//!
//! Equation reference from `docs/design/janiform_compact_model_spec.tex` Section 8:
//! - `F(u_+, u_-, z_+, z_- | Y_0) = exp(C(Delta) + D(Delta) · y_0)`
//! - `y_0 = [Y++, Y+-, Y-+, Y--]`
//! - `k_+ = [alpha_++, 0, alpha_-+, 0]`
//! - `k_- = [0, alpha_+-, 0, alpha_--]`
//! - `r = [rho_++, rho_+-, rho_-+, rho_--]`
//!
//! Mapping policy:
//! - All 4D components use explicit named fields in model order `[++, +-, -+, --]`
//! - `delta` is the interval length `Delta`
//! - `c_at_delta` stores `C(Delta)`
//! - `d_at_delta` stores `D(Delta)`
//! - `transform_value` stores `F`

use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Deserialize;
use serde::Serialize;

use crate::complex_math::{
    complex_exponential, complex_multiply, complex_pow_real_exponent_principal,
};
use crate::hawkes::{
    HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameterError, HawkesKernelParameters,
};
use crate::news::{GammaSeverityMarkConfiguration, NewsMarkConfigurationError};
use crate::nig::ComplexScalar;

pub const EXPONENTIAL_AFFINE_TRANSFORM_EQUATION_MAPPING_POLICY: &str =
    "compact_model_spec_section_8_named_component_mapping.v1";

/// Canonical equation specification shared by pricing and risk solver consumers.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ExponentialAffineTransformEquationSpecification {
    /// Hawkes parameters that provide `lambda_bar`, `beta`, `alpha`, and `rho`.
    pub hawkes_kernel_parameters: HawkesKernelParameters,

    /// Gamma mark parameters that provide `k_U` and `theta_U`.
    pub gamma_severity_mark_configuration: GammaSeverityMarkConfiguration,
}

impl ExponentialAffineTransformEquationSpecification {
    pub fn new(
        hawkes_kernel_parameters: HawkesKernelParameters,
        gamma_severity_mark_configuration: GammaSeverityMarkConfiguration,
    ) -> Self {
        Self {
            hawkes_kernel_parameters,
            gamma_severity_mark_configuration,
        }
    }

    pub fn try_new(
        hawkes_kernel_parameters: HawkesKernelParameters,
        gamma_severity_mark_configuration: GammaSeverityMarkConfiguration,
    ) -> Result<Self, ExponentialAffineTransformEquationSpecificationError> {
        let specification = Self::new(hawkes_kernel_parameters, gamma_severity_mark_configuration);
        specification.validate()?;
        Ok(specification)
    }

    pub fn validate(self) -> Result<(), ExponentialAffineTransformEquationSpecificationError> {
        HawkesKernelParameters::try_new(
            self.hawkes_kernel_parameters.lambda_bar,
            self.hawkes_kernel_parameters.beta,
            self.hawkes_kernel_parameters.alpha,
            self.hawkes_kernel_parameters.rho,
        )
        .map_err(|source| {
            ExponentialAffineTransformEquationSpecificationError::InvalidHawkesKernelParameters {
                source,
            }
        })?;
        self.gamma_severity_mark_configuration
            .validate()
            .map_err(|source| {
                ExponentialAffineTransformEquationSpecificationError::InvalidGammaSeverityMarkConfiguration {
                    source,
                }
            })?;
        Ok(())
    }

    /// Return `k_+ = [alpha_++, 0, alpha_-+, 0]`.
    pub const fn k_plus(self) -> HawkesKernelLoadings {
        HawkesKernelLoadings {
            plus_plus: self.hawkes_kernel_parameters.alpha.plus_plus,
            plus_minus: 0.0,
            minus_plus: self.hawkes_kernel_parameters.alpha.minus_plus,
            minus_minus: 0.0,
        }
    }

    /// Return `k_- = [0, alpha_+-, 0, alpha_--]`.
    pub const fn k_minus(self) -> HawkesKernelLoadings {
        HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: self.hawkes_kernel_parameters.alpha.plus_minus,
            minus_plus: 0.0,
            minus_minus: self.hawkes_kernel_parameters.alpha.minus_minus,
        }
    }

    /// Return `r = [rho_++, rho_+-, rho_-+, rho_--]`.
    pub const fn r(self) -> HawkesKernelLoadings {
        self.hawkes_kernel_parameters.rho
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExponentialAffineTransformEquationSpecificationError {
    InvalidHawkesKernelParameters { source: HawkesKernelParameterError },
    InvalidGammaSeverityMarkConfiguration { source: NewsMarkConfigurationError },
}

impl Display for ExponentialAffineTransformEquationSpecificationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidHawkesKernelParameters { source } => write!(
                formatter,
                "equation specification hawkes_kernel_parameters are invalid: {source}"
            ),
            Self::InvalidGammaSeverityMarkConfiguration { source } => write!(
                formatter,
                "equation specification gamma_severity_mark_configuration is invalid: {source}"
            ),
        }
    }
}

impl Error for ExponentialAffineTransformEquationSpecificationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidHawkesKernelParameters { source } => Some(source),
            Self::InvalidGammaSeverityMarkConfiguration { source } => Some(source),
        }
    }
}

/// Solver input payload for one interval `Delta`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ExponentialAffineTransformInput {
    pub equation_specification: ExponentialAffineTransformEquationSpecification,
    pub u_plus: ComplexScalar,
    pub u_minus: ComplexScalar,
    pub z_plus: ComplexScalar,
    pub z_minus: ComplexScalar,
    pub s_h: f64,
    pub nu_h: f64,
    pub delta: f64,
    pub initial_state: HawkesExcitationState,
}

impl ExponentialAffineTransformInput {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        equation_specification: ExponentialAffineTransformEquationSpecification,
        u_plus: ComplexScalar,
        u_minus: ComplexScalar,
        z_plus: ComplexScalar,
        z_minus: ComplexScalar,
        s_h: f64,
        nu_h: f64,
        delta: f64,
        initial_state: HawkesExcitationState,
    ) -> Self {
        Self {
            equation_specification,
            u_plus,
            u_minus,
            z_plus,
            z_minus,
            s_h,
            nu_h,
            delta,
            initial_state,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        equation_specification: ExponentialAffineTransformEquationSpecification,
        u_plus: ComplexScalar,
        u_minus: ComplexScalar,
        z_plus: ComplexScalar,
        z_minus: ComplexScalar,
        s_h: f64,
        nu_h: f64,
        delta: f64,
        initial_state: HawkesExcitationState,
    ) -> Result<Self, ExponentialAffineTransformInputError> {
        let input = Self::new(
            equation_specification,
            u_plus,
            u_minus,
            z_plus,
            z_minus,
            s_h,
            nu_h,
            delta,
            initial_state,
        );
        input.validate()?;
        Ok(input)
    }

    pub fn validate(self) -> Result<(), ExponentialAffineTransformInputError> {
        self.equation_specification.validate().map_err(|source| {
            ExponentialAffineTransformInputError::InvalidEquationSpecification { source }
        })?;

        ensure_finite_complex_input("u_plus", self.u_plus)?;
        ensure_finite_complex_input("u_minus", self.u_minus)?;
        ensure_finite_complex_input("z_plus", self.z_plus)?;
        ensure_finite_complex_input("z_minus", self.z_minus)?;

        if !self.s_h.is_finite() || self.s_h < 0.0 {
            return Err(
                ExponentialAffineTransformInputError::InvalidSeasonalityFactor { value: self.s_h },
            );
        }
        if !self.nu_h.is_finite() || self.nu_h < 0.0 {
            return Err(ExponentialAffineTransformInputError::InvalidNewsIntensity {
                value: self.nu_h,
            });
        }
        if !self.delta.is_finite() || self.delta <= 0.0 {
            return Err(ExponentialAffineTransformInputError::InvalidDelta { value: self.delta });
        }

        validate_initial_state(self.initial_state)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExponentialAffineTransformInputError {
    InvalidEquationSpecification {
        source: ExponentialAffineTransformEquationSpecificationError,
    },
    InvalidComplexField {
        field: &'static str,
        value: ComplexScalar,
    },
    InvalidSeasonalityFactor {
        value: f64,
    },
    InvalidNewsIntensity {
        value: f64,
    },
    InvalidDelta {
        value: f64,
    },
    InvalidInitialStateComponent {
        field: &'static str,
        value: f64,
    },
}

impl Display for ExponentialAffineTransformInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidEquationSpecification { source } => {
                write!(
                    formatter,
                    "transform input equation specification is invalid: {source}"
                )
            }
            Self::InvalidComplexField { field, value } => write!(
                formatter,
                "{field} must be finite as a complex scalar, found {value:?}"
            ),
            Self::InvalidSeasonalityFactor { value } => write!(
                formatter,
                "s_h must be finite and non-negative, found {value}"
            ),
            Self::InvalidNewsIntensity { value } => write!(
                formatter,
                "nu_h must be finite and non-negative, found {value}"
            ),
            Self::InvalidDelta { value } => write!(
                formatter,
                "delta must be finite and strictly positive, found {value}"
            ),
            Self::InvalidInitialStateComponent { field, value } => write!(
                formatter,
                "initial_state.{field} must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for ExponentialAffineTransformInputError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidEquationSpecification { source } => Some(source),
            _ => None,
        }
    }
}

/// Explicit 4D vector for `D(Delta)` in model order `[++, +-, -+, --]`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ExponentialAffineTransformDVector {
    pub plus_plus: ComplexScalar,
    pub plus_minus: ComplexScalar,
    pub minus_plus: ComplexScalar,
    pub minus_minus: ComplexScalar,
}

impl ExponentialAffineTransformDVector {
    pub fn new(
        plus_plus: ComplexScalar,
        plus_minus: ComplexScalar,
        minus_plus: ComplexScalar,
        minus_minus: ComplexScalar,
    ) -> Self {
        Self {
            plus_plus,
            plus_minus,
            minus_plus,
            minus_minus,
        }
    }

    pub fn try_new(
        plus_plus: ComplexScalar,
        plus_minus: ComplexScalar,
        minus_plus: ComplexScalar,
        minus_minus: ComplexScalar,
    ) -> Result<Self, ExponentialAffineTransformDVectorError> {
        let vector = Self::new(plus_plus, plus_minus, minus_plus, minus_minus);
        vector.validate()?;
        Ok(vector)
    }

    pub fn validate(self) -> Result<(), ExponentialAffineTransformDVectorError> {
        ensure_finite_complex_d_vector("plus_plus", self.plus_plus)?;
        ensure_finite_complex_d_vector("plus_minus", self.plus_minus)?;
        ensure_finite_complex_d_vector("minus_plus", self.minus_plus)?;
        ensure_finite_complex_d_vector("minus_minus", self.minus_minus)?;

        Ok(())
    }

    /// Compute `D(Delta) · y_0`.
    pub fn dot_state(self, state: HawkesExcitationState) -> ComplexScalar {
        let mut total = ComplexScalar::new(0.0, 0.0);
        total = total.add(self.plus_plus.scale(state.y_plus_plus));
        total = total.add(self.plus_minus.scale(state.y_plus_minus));
        total = total.add(self.minus_plus.scale(state.y_minus_plus));
        total.add(self.minus_minus.scale(state.y_minus_minus))
    }

    /// Compute `D · w` using model-order component matching.
    pub fn dot_loadings(self, loadings: HawkesKernelLoadings) -> ComplexScalar {
        let mut total = ComplexScalar::new(0.0, 0.0);
        total = total.add(self.plus_plus.scale(loadings.plus_plus));
        total = total.add(self.plus_minus.scale(loadings.plus_minus));
        total = total.add(self.minus_plus.scale(loadings.minus_plus));
        total.add(self.minus_minus.scale(loadings.minus_minus))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExponentialAffineTransformDVectorError {
    InvalidComponent {
        field: &'static str,
        value: ComplexScalar,
    },
}

impl Display for ExponentialAffineTransformDVectorError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidComponent { field, value } => write!(
                formatter,
                "d_at_delta.{field} must be finite as a complex scalar, found {value:?}"
            ),
        }
    }
}

impl Error for ExponentialAffineTransformDVectorError {}

/// Solver output for one interval `Delta`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ExponentialAffineTransformOutput {
    pub c_at_delta: ComplexScalar,
    pub d_at_delta: ExponentialAffineTransformDVector,
    pub transform_value: ComplexScalar,
}

impl ExponentialAffineTransformOutput {
    pub fn new(
        c_at_delta: ComplexScalar,
        d_at_delta: ExponentialAffineTransformDVector,
        transform_value: ComplexScalar,
    ) -> Self {
        Self {
            c_at_delta,
            d_at_delta,
            transform_value,
        }
    }

    pub fn try_new(
        c_at_delta: ComplexScalar,
        d_at_delta: ExponentialAffineTransformDVector,
        transform_value: ComplexScalar,
    ) -> Result<Self, ExponentialAffineTransformOutputError> {
        let output = Self::new(c_at_delta, d_at_delta, transform_value);
        output.validate()?;
        Ok(output)
    }

    pub fn validate(self) -> Result<(), ExponentialAffineTransformOutputError> {
        if !self.c_at_delta.is_finite() {
            return Err(ExponentialAffineTransformOutputError::InvalidCAtDelta {
                value: self.c_at_delta,
            });
        }
        self.d_at_delta
            .validate()
            .map_err(|source| ExponentialAffineTransformOutputError::InvalidDAtDelta { source })?;
        if !self.transform_value.is_finite() {
            return Err(
                ExponentialAffineTransformOutputError::InvalidTransformValue {
                    value: self.transform_value,
                },
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExponentialAffineTransformOutputError {
    InvalidCAtDelta {
        value: ComplexScalar,
    },
    InvalidDAtDelta {
        source: ExponentialAffineTransformDVectorError,
    },
    InvalidTransformValue {
        value: ComplexScalar,
    },
}

impl Display for ExponentialAffineTransformOutputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCAtDelta { value } => write!(
                formatter,
                "c_at_delta must be finite as a complex scalar, found {value:?}"
            ),
            Self::InvalidDAtDelta { source } => write!(formatter, "{source}"),
            Self::InvalidTransformValue { value } => write!(
                formatter,
                "transform_value must be finite as a complex scalar, found {value:?}"
            ),
        }
    }
}

impl Error for ExponentialAffineTransformOutputError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidDAtDelta { source } => Some(source),
            _ => None,
        }
    }
}

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
/// - `C' = s_h * lambda_bar * (H_plus + H_minus) + nu_h * ((1 - theta_U * (D · r))^(-k_U) - 1)`
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

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExponentialAffineTransformEquationSpecificationPayload {
    hawkes_kernel_parameters: HawkesKernelParameters,
    gamma_severity_mark_configuration: GammaSeverityMarkConfiguration,
}

impl<'de> Deserialize<'de> for ExponentialAffineTransformEquationSpecification {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload =
            ExponentialAffineTransformEquationSpecificationPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.hawkes_kernel_parameters,
            payload.gamma_severity_mark_configuration,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComplexScalarPayload {
    real: f64,
    imaginary: f64,
}

impl From<ComplexScalarPayload> for ComplexScalar {
    fn from(payload: ComplexScalarPayload) -> Self {
        Self::new(payload.real, payload.imaginary)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct HawkesExcitationStatePayload {
    y_plus_plus: f64,
    y_plus_minus: f64,
    y_minus_plus: f64,
    y_minus_minus: f64,
}

impl From<HawkesExcitationStatePayload> for HawkesExcitationState {
    fn from(payload: HawkesExcitationStatePayload) -> Self {
        Self {
            y_plus_plus: payload.y_plus_plus,
            y_plus_minus: payload.y_plus_minus,
            y_minus_plus: payload.y_minus_plus,
            y_minus_minus: payload.y_minus_minus,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExponentialAffineTransformInputPayload {
    equation_specification: ExponentialAffineTransformEquationSpecification,
    u_plus: ComplexScalarPayload,
    u_minus: ComplexScalarPayload,
    z_plus: ComplexScalarPayload,
    z_minus: ComplexScalarPayload,
    s_h: f64,
    nu_h: f64,
    delta: f64,
    initial_state: HawkesExcitationStatePayload,
}

impl<'de> Deserialize<'de> for ExponentialAffineTransformInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = ExponentialAffineTransformInputPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.equation_specification,
            payload.u_plus.into(),
            payload.u_minus.into(),
            payload.z_plus.into(),
            payload.z_minus.into(),
            payload.s_h,
            payload.nu_h,
            payload.delta,
            payload.initial_state.into(),
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExponentialAffineTransformDVectorPayload {
    plus_plus: ComplexScalarPayload,
    plus_minus: ComplexScalarPayload,
    minus_plus: ComplexScalarPayload,
    minus_minus: ComplexScalarPayload,
}

impl<'de> Deserialize<'de> for ExponentialAffineTransformDVector {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = ExponentialAffineTransformDVectorPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.plus_plus.into(),
            payload.plus_minus.into(),
            payload.minus_plus.into(),
            payload.minus_minus.into(),
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExponentialAffineTransformOutputPayload {
    c_at_delta: ComplexScalarPayload,
    d_at_delta: ExponentialAffineTransformDVector,
    transform_value: ComplexScalarPayload,
}

impl<'de> Deserialize<'de> for ExponentialAffineTransformOutput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = ExponentialAffineTransformOutputPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.c_at_delta.into(),
            payload.d_at_delta,
            payload.transform_value.into(),
        )
        .map_err(serde::de::Error::custom)
    }
}

fn ensure_finite_complex_input(
    field: &'static str,
    value: ComplexScalar,
) -> Result<(), ExponentialAffineTransformInputError> {
    if !value.is_finite() {
        return Err(ExponentialAffineTransformInputError::InvalidComplexField { field, value });
    }

    Ok(())
}

fn ensure_finite_complex_d_vector(
    field: &'static str,
    value: ComplexScalar,
) -> Result<(), ExponentialAffineTransformDVectorError> {
    if !value.is_finite() {
        return Err(ExponentialAffineTransformDVectorError::InvalidComponent { field, value });
    }

    Ok(())
}

fn validate_initial_state(
    initial_state: HawkesExcitationState,
) -> Result<(), ExponentialAffineTransformInputError> {
    validate_initial_state_component("y_plus_plus", initial_state.y_plus_plus)?;
    validate_initial_state_component("y_plus_minus", initial_state.y_plus_minus)?;
    validate_initial_state_component("y_minus_plus", initial_state.y_minus_plus)?;
    validate_initial_state_component("y_minus_minus", initial_state.y_minus_minus)?;

    Ok(())
}

fn validate_initial_state_component(
    field: &'static str,
    value: f64,
) -> Result<(), ExponentialAffineTransformInputError> {
    if !value.is_finite() || value < 0.0 {
        return Err(
            ExponentialAffineTransformInputError::InvalidInitialStateComponent { field, value },
        );
    }

    Ok(())
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
        .scale(transform_input.nu_h)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        EXPONENTIAL_AFFINE_TRANSFORM_EQUATION_MAPPING_POLICY, ExponentialAffineTransformDVector,
        ExponentialAffineTransformEquationSpecification, ExponentialAffineTransformInput,
        ExponentialAffineTransformOutput, evaluate_exponential_affine_transform_hamiltonians,
        evaluate_exponential_affine_transform_ode_rhs,
    };
    use crate::hawkes::{HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters};
    use crate::news::GammaSeverityMarkConfiguration;
    use crate::nig::ComplexScalar;

    fn valid_equation_specification() -> ExponentialAffineTransformEquationSpecification {
        ExponentialAffineTransformEquationSpecification::try_new(
            HawkesKernelParameters::try_new(
                0.8,
                1.2,
                HawkesKernelLoadings {
                    plus_plus: 0.20,
                    plus_minus: 0.08,
                    minus_plus: 0.10,
                    minus_minus: 0.24,
                },
                HawkesKernelLoadings {
                    plus_plus: 0.03,
                    plus_minus: 0.02,
                    minus_plus: 0.01,
                    minus_minus: 0.04,
                },
            )
            .expect("valid kernel payload should construct"),
            GammaSeverityMarkConfiguration::try_new(1.8, 0.9)
                .expect("valid gamma configuration should construct"),
        )
        .expect("valid equation specification should construct")
    }

    fn valid_input() -> ExponentialAffineTransformInput {
        ExponentialAffineTransformInput::try_new(
            valid_equation_specification(),
            ComplexScalar::new(0.2, -0.1),
            ComplexScalar::new(0.3, 0.05),
            ComplexScalar::new(1.0, 0.0),
            ComplexScalar::new(0.95, 0.02),
            1.1,
            0.08,
            1.0,
            HawkesExcitationState {
                y_plus_plus: 0.4,
                y_plus_minus: 0.2,
                y_minus_plus: 0.3,
                y_minus_minus: 0.5,
            },
        )
        .expect("valid transform input should construct")
    }

    fn valid_output() -> ExponentialAffineTransformOutput {
        ExponentialAffineTransformOutput::try_new(
            ComplexScalar::new(-0.03, 0.01),
            ExponentialAffineTransformDVector::try_new(
                ComplexScalar::new(0.10, -0.02),
                ComplexScalar::new(0.08, 0.01),
                ComplexScalar::new(0.07, -0.03),
                ComplexScalar::new(0.09, 0.00),
            )
            .expect("valid d_at_delta should construct"),
            ComplexScalar::new(0.98, -0.04),
        )
        .expect("valid output should construct")
    }

    fn assert_complex_close(
        observed: ComplexScalar,
        expected: ComplexScalar,
        tolerance: f64,
        context: &str,
    ) {
        assert!(
            (observed.real - expected.real).abs() <= tolerance,
            "{context}: real mismatch, observed={} expected={}",
            observed.real,
            expected.real
        );
        assert!(
            (observed.imaginary - expected.imaginary).abs() <= tolerance,
            "{context}: imaginary mismatch, observed={} expected={}",
            observed.imaginary,
            expected.imaginary
        );
    }

    #[test]
    fn transform_input_constructs_from_reference_payload() {
        let payload = json!({
            "equation_specification": {
                "hawkes_kernel_parameters": {
                    "lambda_bar": 0.8,
                    "beta": 1.2,
                    "alpha": {
                        "plus_plus": 0.20,
                        "plus_minus": 0.08,
                        "minus_plus": 0.10,
                        "minus_minus": 0.24
                    },
                    "rho": {
                        "plus_plus": 0.03,
                        "plus_minus": 0.02,
                        "minus_plus": 0.01,
                        "minus_minus": 0.04
                    }
                },
                "gamma_severity_mark_configuration": {
                    "shape_k": 1.8,
                    "scale_theta": 0.9
                }
            },
            "u_plus": {
                "real": 0.2,
                "imaginary": -0.1
            },
            "u_minus": {
                "real": 0.3,
                "imaginary": 0.05
            },
            "z_plus": {
                "real": 1.0,
                "imaginary": 0.0
            },
            "z_minus": {
                "real": 0.95,
                "imaginary": 0.02
            },
            "s_h": 1.1,
            "nu_h": 0.08,
            "delta": 1.0,
            "initial_state": {
                "y_plus_plus": 0.4,
                "y_plus_minus": 0.2,
                "y_minus_plus": 0.3,
                "y_minus_minus": 0.5
            }
        });

        let parsed: ExponentialAffineTransformInput =
            serde_json::from_value(payload).expect("reference payload should deserialize");
        assert_eq!(parsed, valid_input());
    }

    #[test]
    fn transform_models_round_trip_through_json() {
        let input = valid_input();
        let output = valid_output();

        let serialized_input =
            serde_json::to_string(&input).expect("input payload should serialize");
        let parsed_input: ExponentialAffineTransformInput =
            serde_json::from_str(&serialized_input).expect("input payload should deserialize");
        assert_eq!(parsed_input, input);

        let serialized_output =
            serde_json::to_string(&output).expect("output payload should serialize");
        let parsed_output: ExponentialAffineTransformOutput =
            serde_json::from_str(&serialized_output).expect("output payload should deserialize");
        assert_eq!(parsed_output, output);
    }

    #[test]
    fn transform_input_deserialization_reports_missing_required_field() {
        let payload = json!({
            "equation_specification": {
                "hawkes_kernel_parameters": {
                    "lambda_bar": 0.8,
                    "beta": 1.2,
                    "alpha": {
                        "plus_plus": 0.20,
                        "plus_minus": 0.08,
                        "minus_plus": 0.10,
                        "minus_minus": 0.24
                    },
                    "rho": {
                        "plus_plus": 0.03,
                        "plus_minus": 0.02,
                        "minus_plus": 0.01,
                        "minus_minus": 0.04
                    }
                },
                "gamma_severity_mark_configuration": {
                    "shape_k": 1.8,
                    "scale_theta": 0.9
                }
            },
            "u_plus": {
                "real": 0.2,
                "imaginary": -0.1
            },
            "u_minus": {
                "real": 0.3,
                "imaginary": 0.05
            },
            "z_plus": {
                "real": 1.0,
                "imaginary": 0.0
            },
            "z_minus": {
                "real": 0.95,
                "imaginary": 0.02
            },
            "s_h": 1.1,
            "nu_h": 0.08,
            "initial_state": {
                "y_plus_plus": 0.4,
                "y_plus_minus": 0.2,
                "y_minus_plus": 0.3,
                "y_minus_minus": 0.5
            }
        });

        let parse_result = serde_json::from_value::<ExponentialAffineTransformInput>(payload);
        let error_text = parse_result
            .expect_err("missing delta field must fail")
            .to_string();
        assert!(
            error_text.contains("missing field `delta`"),
            "unexpected error message: {error_text}"
        );
    }

    #[test]
    fn transform_input_deserialization_reports_malformed_field_type() {
        let payload = json!({
            "equation_specification": {
                "hawkes_kernel_parameters": {
                    "lambda_bar": 0.8,
                    "beta": 1.2,
                    "alpha": {
                        "plus_plus": 0.20,
                        "plus_minus": 0.08,
                        "minus_plus": 0.10,
                        "minus_minus": 0.24
                    },
                    "rho": {
                        "plus_plus": 0.03,
                        "plus_minus": 0.02,
                        "minus_plus": 0.01,
                        "minus_minus": 0.04
                    }
                },
                "gamma_severity_mark_configuration": {
                    "shape_k": 1.8,
                    "scale_theta": 0.9
                }
            },
            "u_plus": {
                "real": 0.2,
                "imaginary": -0.1
            },
            "u_minus": {
                "real": 0.3,
                "imaginary": 0.05
            },
            "z_plus": {
                "real": 1.0,
                "imaginary": 0.0
            },
            "z_minus": {
                "real": 0.95,
                "imaginary": 0.02
            },
            "s_h": "high",
            "nu_h": 0.08,
            "delta": 1.0,
            "initial_state": {
                "y_plus_plus": 0.4,
                "y_plus_minus": 0.2,
                "y_minus_plus": 0.3,
                "y_minus_minus": 0.5
            }
        });

        let parse_result = serde_json::from_value::<ExponentialAffineTransformInput>(payload);
        let error_text = parse_result
            .expect_err("malformed s_h field type must fail")
            .to_string();
        assert!(
            error_text.contains("invalid type"),
            "unexpected error message: {error_text}"
        );
    }

    #[test]
    fn transform_input_deserialization_reports_invalid_delta_value() {
        let payload = json!({
            "equation_specification": {
                "hawkes_kernel_parameters": {
                    "lambda_bar": 0.8,
                    "beta": 1.2,
                    "alpha": {
                        "plus_plus": 0.20,
                        "plus_minus": 0.08,
                        "minus_plus": 0.10,
                        "minus_minus": 0.24
                    },
                    "rho": {
                        "plus_plus": 0.03,
                        "plus_minus": 0.02,
                        "minus_plus": 0.01,
                        "minus_minus": 0.04
                    }
                },
                "gamma_severity_mark_configuration": {
                    "shape_k": 1.8,
                    "scale_theta": 0.9
                }
            },
            "u_plus": {
                "real": 0.2,
                "imaginary": -0.1
            },
            "u_minus": {
                "real": 0.3,
                "imaginary": 0.05
            },
            "z_plus": {
                "real": 1.0,
                "imaginary": 0.0
            },
            "z_minus": {
                "real": 0.95,
                "imaginary": 0.02
            },
            "s_h": 1.1,
            "nu_h": 0.08,
            "delta": 0.0,
            "initial_state": {
                "y_plus_plus": 0.4,
                "y_plus_minus": 0.2,
                "y_minus_plus": 0.3,
                "y_minus_minus": 0.5
            }
        });

        let parse_result = serde_json::from_value::<ExponentialAffineTransformInput>(payload);
        let error_text = parse_result
            .expect_err("invalid delta value must fail")
            .to_string();
        assert!(
            error_text.contains("delta must be finite and strictly positive"),
            "unexpected error message: {error_text}"
        );
    }

    #[test]
    fn equation_mapping_policy_constant_is_stable() {
        assert_eq!(
            EXPONENTIAL_AFFINE_TRANSFORM_EQUATION_MAPPING_POLICY,
            "compact_model_spec_section_8_named_component_mapping.v1"
        );
    }

    #[test]
    fn hamiltonians_match_hand_checked_complex_fixture() {
        let transform_input = valid_input();
        let d_vector = ExponentialAffineTransformDVector::new(
            ComplexScalar::new(0.10, -0.02),
            ComplexScalar::new(0.08, 0.01),
            ComplexScalar::new(0.07, -0.03),
            ComplexScalar::new(0.09, 0.00),
        );

        let hamiltonians =
            evaluate_exponential_affine_transform_hamiltonians(transform_input, d_vector);

        assert_complex_close(
            hamiltonians.h_plus,
            ComplexScalar::new(-0.17265736764489886, 0.09280848411170442),
            1.0e-12,
            "H_plus",
        );
        assert_complex_close(
            hamiltonians.h_minus,
            ComplexScalar::new(-0.32304086676111327, -0.028650512256511856),
            1.0e-12,
            "H_minus",
        );
    }

    #[test]
    fn ode_rhs_matches_hand_checked_real_fixture() {
        let transform_input = ExponentialAffineTransformInput::try_new(
            valid_equation_specification(),
            ComplexScalar::new(0.2, 0.0),
            ComplexScalar::new(0.3, 0.0),
            ComplexScalar::new(0.9, 0.0),
            ComplexScalar::new(1.1, 0.0),
            1.1,
            0.08,
            1.0,
            HawkesExcitationState {
                y_plus_plus: 0.4,
                y_plus_minus: 0.2,
                y_minus_plus: 0.3,
                y_minus_minus: 0.5,
            },
        )
        .expect("fixture input should construct");
        let d_vector = ExponentialAffineTransformDVector::new(
            ComplexScalar::new(0.4, 0.0),
            ComplexScalar::new(0.2, 0.0),
            ComplexScalar::new(0.3, 0.0),
            ComplexScalar::new(0.1, 0.0),
        );

        let rhs = evaluate_exponential_affine_transform_ode_rhs(transform_input, d_vector);

        assert_complex_close(
            rhs.d_derivative.plus_plus,
            ComplexScalar::new(-0.6948847102457174, 0.0),
            1.0e-12,
            "D_++'",
        );
        assert_complex_close(
            rhs.d_derivative.plus_minus,
            ComplexScalar::new(-0.45488471024571747, 0.0),
            1.0e-12,
            "D_+-'",
        );
        assert_complex_close(
            rhs.d_derivative.minus_plus,
            ComplexScalar::new(-0.5306189632272101, 0.0),
            1.0e-12,
            "D_-+'",
        );
        assert_complex_close(
            rhs.d_derivative.minus_minus,
            ComplexScalar::new(-0.29061896322721015, 0.0),
            1.0e-12,
            "D_--'",
        );
        assert_complex_close(
            rhs.c_derivative,
            ComplexScalar::new(-0.3053334325713209, 0.0),
            1.0e-12,
            "C'",
        );
    }

    #[test]
    fn ode_rhs_is_deterministic_for_identical_inputs() {
        let transform_input = valid_input();
        let d_vector = ExponentialAffineTransformDVector::new(
            ComplexScalar::new(0.14, -0.02),
            ComplexScalar::new(0.05, 0.04),
            ComplexScalar::new(0.09, -0.01),
            ComplexScalar::new(0.11, 0.03),
        );

        let first = evaluate_exponential_affine_transform_ode_rhs(transform_input, d_vector);
        let second = evaluate_exponential_affine_transform_ode_rhs(transform_input, d_vector);

        assert_eq!(first, second);
    }
}
