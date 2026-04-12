use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use janiform_foundations::complex_math::ComplexScalar;
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameterError, HawkesKernelParameters,
};
use janiform_model_kernels::news::{GammaSeverityMarkConfiguration, NewsMarkConfigurationError};

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

/// One-interval news intensity input with explicit known/surprise components.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExponentialAffineTransformNewsIntensityInput {
    pub known_nu_h: f64,
    pub surprise_nu_h: f64,
}

impl ExponentialAffineTransformNewsIntensityInput {
    pub const fn zero() -> Self {
        Self {
            known_nu_h: 0.0,
            surprise_nu_h: 0.0,
        }
    }

    pub fn try_new(
        known_nu_h: f64,
        surprise_nu_h: f64,
    ) -> Result<Self, ExponentialAffineTransformNewsIntensityInputError> {
        let input = Self {
            known_nu_h,
            surprise_nu_h,
        };
        input.validate()?;
        Ok(input)
    }

    pub fn validate(self) -> Result<(), ExponentialAffineTransformNewsIntensityInputError> {
        if !self.known_nu_h.is_finite() || self.known_nu_h < 0.0 {
            return Err(
                ExponentialAffineTransformNewsIntensityInputError::InvalidKnownNewsIntensity {
                    value: self.known_nu_h,
                },
            );
        }
        if !self.surprise_nu_h.is_finite() || self.surprise_nu_h < 0.0 {
            return Err(
                ExponentialAffineTransformNewsIntensityInputError::InvalidSurpriseNewsIntensity {
                    value: self.surprise_nu_h,
                },
            );
        }

        if !self.total_nu_h().is_finite() {
            return Err(
                ExponentialAffineTransformNewsIntensityInputError::NonFiniteAggregateNewsIntensity {
                    known_nu_h: self.known_nu_h,
                    surprise_nu_h: self.surprise_nu_h,
                },
            );
        }

        Ok(())
    }

    pub fn total_nu_h(self) -> f64 {
        self.known_nu_h + self.surprise_nu_h
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExponentialAffineTransformNewsIntensityInputError {
    InvalidKnownNewsIntensity { value: f64 },
    InvalidSurpriseNewsIntensity { value: f64 },
    NonFiniteAggregateNewsIntensity { known_nu_h: f64, surprise_nu_h: f64 },
}

impl Display for ExponentialAffineTransformNewsIntensityInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidKnownNewsIntensity { value } => write!(
                formatter,
                "known_nu_h must be finite and non-negative, found {value}"
            ),
            Self::InvalidSurpriseNewsIntensity { value } => write!(
                formatter,
                "surprise_nu_h must be finite and non-negative, found {value}"
            ),
            Self::NonFiniteAggregateNewsIntensity {
                known_nu_h,
                surprise_nu_h,
            } => write!(
                formatter,
                "known_nu_h + surprise_nu_h must be finite, found known_nu_h={known_nu_h}, surprise_nu_h={surprise_nu_h}"
            ),
        }
    }
}

impl Error for ExponentialAffineTransformNewsIntensityInputError {}

/// Solver input payload for one interval `Delta`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ExponentialAffineTransformInput {
    pub equation_specification: ExponentialAffineTransformEquationSpecification,
    pub u_plus: ComplexScalar,
    pub u_minus: ComplexScalar,
    pub z_plus: ComplexScalar,
    pub z_minus: ComplexScalar,
    pub s_h: f64,
    pub news_intensity: ExponentialAffineTransformNewsIntensityInput,
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
        news_intensity: ExponentialAffineTransformNewsIntensityInput,
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
            news_intensity,
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
        news_intensity: ExponentialAffineTransformNewsIntensityInput,
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
            news_intensity,
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
        self.news_intensity.validate().map_err(|source| {
            ExponentialAffineTransformInputError::InvalidNewsIntensityInput { source }
        })?;
        if !self.delta.is_finite() || self.delta <= 0.0 {
            return Err(ExponentialAffineTransformInputError::InvalidDelta { value: self.delta });
        }

        validate_initial_state(self.initial_state)?;
        Ok(())
    }

    pub(crate) fn is_exact_identity_query(self) -> bool {
        self.u_plus == ComplexScalar::new(0.0, 0.0)
            && self.u_minus == ComplexScalar::new(0.0, 0.0)
            && self.z_plus == ComplexScalar::new(1.0, 0.0)
            && self.z_minus == ComplexScalar::new(1.0, 0.0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExponentialAffineTransformInputError {
    InvalidEquationSpecification {
        source: ExponentialAffineTransformEquationSpecificationError,
    },
    InvalidNewsIntensityInput {
        source: ExponentialAffineTransformNewsIntensityInputError,
    },
    InvalidComplexField {
        field: &'static str,
        value: ComplexScalar,
    },
    InvalidSeasonalityFactor {
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
            Self::InvalidNewsIntensityInput { source } => {
                write!(
                    formatter,
                    "transform input news_intensity is invalid: {source}"
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
            Self::InvalidNewsIntensityInput { source } => Some(source),
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

    /// Componentwise addition in model order `[++, +-, -+, --]`.
    pub fn add(self, other: Self) -> Self {
        Self::new(
            self.plus_plus.add(other.plus_plus),
            self.plus_minus.add(other.plus_minus),
            self.minus_plus.add(other.minus_plus),
            self.minus_minus.add(other.minus_minus),
        )
    }

    /// Uniform scalar multiplication across all components.
    pub fn scale(self, value: f64) -> Self {
        Self::new(
            self.plus_plus.scale(value),
            self.plus_minus.scale(value),
            self.minus_plus.scale(value),
            self.minus_minus.scale(value),
        )
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
