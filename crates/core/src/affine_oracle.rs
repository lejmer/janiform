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

use std::collections::BTreeSet;
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
pub const EXPONENTIAL_AFFINE_TRANSFORM_DEFAULT_SOLVER_POLICY: &str =
    "fourth_order_runge_kutta_fixed_step.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_DOMAIN_GUARD_POLICY: &str =
    "enforce_d_dot_r_real_strict_upper_bound_at_runtime_checkpoints.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_BLOWUP_TERMINATION_POLICY: &str =
    "terminate_on_non_finite_state_or_guard_metric_without_clamping.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_RESULT_SCHEMA_VERSION: &str =
    "affine_oracle.integration_result.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_METADATA_SCHEMA_VERSION: &str =
    "affine_oracle.integration_metadata.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_GUARD_FAILURE_SCHEMA_VERSION: &str =
    "affine_oracle.integration_guard_failure.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_TELEMETRY_SCHEMA_VERSION: &str =
    "affine_oracle.integration_telemetry.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "affine_oracle.integration_output_schema_metadata.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_TOLERANCE_POLICY_SCHEMA_VERSION: &str =
    "affine_oracle.method_comparison.tolerance_policy.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REPORT_SCHEMA_VERSION: &str =
    "affine_oracle.method_comparison.report.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "affine_oracle.method_comparison.output_schema_metadata.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_AGREEMENT_POLICY: &str =
    "require_completed_status_and_absolute_tolerance_agreement.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_INTERVAL_SELECTION_POLICY: &str =
    "first_band_with_delta_upper_bound_greater_or_equal_to_delta.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REFERENCE_FIXTURE_POLICY: &str =
    "method_comparison_checks_are_complementary_to_reference_fixtures.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REQUIRED_REPORT_FIELDS: [&str; 12] = [
    "schema_version",
    "output_schema_metadata",
    "policy_schema_version",
    "policy_version",
    "agreement_policy",
    "interval_selection_policy",
    "reference_fixture_policy",
    "input_delta",
    "tolerance_band",
    "baseline_run",
    "alternative_runs",
    "all_checks_passed",
];
pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REQUIRED_TOLERANCE_BAND_FIELDS: [&str; 3] = [
    "interval_grid_label",
    "delta_upper_bound",
    "tolerance_window",
];
pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REQUIRED_TOLERANCE_WINDOW_FIELDS: [&str;
    3] = [
    "c_at_delta_absolute_tolerance",
    "d_component_absolute_tolerance",
    "transform_value_absolute_tolerance",
];
pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REQUIRED_RUN_SUMMARY_FIELDS: [&str; 4] = [
    "integrator_method",
    "step_count",
    "status",
    "guard_failure_code",
];
pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REQUIRED_ALTERNATIVE_RUN_FIELDS: [&str;
    6] = [
    "run_summary",
    "status_matches_baseline",
    "output_difference",
    "tolerance_window",
    "passed",
    "failure_codes",
];
pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REQUIRED_OUTPUT_DIFFERENCE_FIELDS: [&str;
    3] = [
    "c_at_delta_absolute_difference",
    "d_component_absolute_difference",
    "transform_value_absolute_difference",
];
pub const EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_RESULT_FIELDS: [&str; 6] = [
    "schema_version",
    "output_schema_metadata",
    "status",
    "metadata",
    "output",
    "telemetry",
];
pub const EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_METADATA_FIELDS: [&str; 11] = [
    "schema_version",
    "integrator_method",
    "telemetry_mode",
    "status_flags",
    "configured_step_count",
    "completed_step_count",
    "step_size",
    "rhs_evaluation_count",
    "domain_guard_policy",
    "blowup_termination_policy",
    "guard_failure",
];
pub const EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_GUARD_FAILURE_FIELDS: [&str; 6] = [
    "schema_version",
    "code",
    "step_index",
    "time",
    "d_dot_r",
    "d_dot_r_upper_bound",
];
pub const EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_TELEMETRY_FIELDS: [&str; 8] = [
    "schema_version",
    "checkpoint_count",
    "max_abs_c_value",
    "max_abs_d_component",
    "max_d_dot_r_real",
    "min_domain_margin",
    "guard_triggered",
    "guard_trigger",
];

const DEFAULT_EXPONENTIAL_AFFINE_TRANSFORM_SOLVER_STEP_COUNT: usize = 64;

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

/// Supported one-step numerical integration methods for affine transform ODE solving.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExponentialAffineTransformIntegratorMethod {
    FourthOrderRungeKutta,
    ForwardEuler,
}

impl Default for ExponentialAffineTransformIntegratorMethod {
    fn default() -> Self {
        Self::FourthOrderRungeKutta
    }
}

/// Debug telemetry emission mode for one-interval affine transform integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExponentialAffineTransformTelemetryMode {
    Disabled,
    Enabled,
}

impl Default for ExponentialAffineTransformTelemetryMode {
    fn default() -> Self {
        Self::Disabled
    }
}

/// Solver settings for one-interval affine transform integration.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExponentialAffineTransformSolverSettings {
    pub integrator_method: ExponentialAffineTransformIntegratorMethod,
    pub step_count: usize,
    pub telemetry_mode: ExponentialAffineTransformTelemetryMode,
}

impl Default for ExponentialAffineTransformSolverSettings {
    fn default() -> Self {
        Self {
            integrator_method: ExponentialAffineTransformIntegratorMethod::default(),
            step_count: DEFAULT_EXPONENTIAL_AFFINE_TRANSFORM_SOLVER_STEP_COUNT,
            telemetry_mode: ExponentialAffineTransformTelemetryMode::default(),
        }
    }
}

impl ExponentialAffineTransformSolverSettings {
    pub fn new(
        integrator_method: ExponentialAffineTransformIntegratorMethod,
        step_count: usize,
    ) -> Self {
        Self {
            integrator_method,
            step_count,
            telemetry_mode: ExponentialAffineTransformTelemetryMode::Disabled,
        }
    }

    pub fn try_new(
        integrator_method: ExponentialAffineTransformIntegratorMethod,
        step_count: usize,
    ) -> Result<Self, ExponentialAffineTransformSolverSettingsError> {
        let settings = Self::new(integrator_method, step_count);
        settings.validate()?;
        Ok(settings)
    }

    pub fn new_with_telemetry(
        integrator_method: ExponentialAffineTransformIntegratorMethod,
        step_count: usize,
        telemetry_mode: ExponentialAffineTransformTelemetryMode,
    ) -> Self {
        Self {
            integrator_method,
            step_count,
            telemetry_mode,
        }
    }

    pub fn try_new_with_telemetry(
        integrator_method: ExponentialAffineTransformIntegratorMethod,
        step_count: usize,
        telemetry_mode: ExponentialAffineTransformTelemetryMode,
    ) -> Result<Self, ExponentialAffineTransformSolverSettingsError> {
        let settings = Self::new_with_telemetry(integrator_method, step_count, telemetry_mode);
        settings.validate()?;
        Ok(settings)
    }

    pub fn with_telemetry_mode(
        self,
        telemetry_mode: ExponentialAffineTransformTelemetryMode,
    ) -> Self {
        Self {
            integrator_method: self.integrator_method,
            step_count: self.step_count,
            telemetry_mode,
        }
    }

    pub fn validate(self) -> Result<(), ExponentialAffineTransformSolverSettingsError> {
        if self.step_count == 0 {
            return Err(
                ExponentialAffineTransformSolverSettingsError::InvalidStepCount {
                    value: self.step_count,
                },
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExponentialAffineTransformSolverSettingsError {
    InvalidStepCount { value: usize },
}

impl Display for ExponentialAffineTransformSolverSettingsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidStepCount { value } => write!(
                formatter,
                "solver step_count must be strictly positive, found {value}"
            ),
        }
    }
}

impl Error for ExponentialAffineTransformSolverSettingsError {}

/// Structured integration status for one affine transform solve call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExponentialAffineTransformIntegrationStatus {
    Completed,
    DomainGuardViolation,
    BlowupTerminated,
}

/// Machine-readable reason code for guard-triggered integration termination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExponentialAffineTransformGuardFailureCode {
    DomainUpperBoundViolation,
    NonFiniteState,
    NonFiniteDomainMetric,
}

/// Guard-trigger metadata captured at deterministic integration checkpoints.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ExponentialAffineTransformGuardFailure {
    pub schema_version: &'static str,
    pub code: ExponentialAffineTransformGuardFailureCode,
    pub step_index: usize,
    pub time: f64,
    pub d_dot_r: ComplexScalar,
    pub d_dot_r_upper_bound: f64,
}

/// Deterministic boolean status flags mirroring the integration status enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ExponentialAffineTransformIntegrationStatusFlags {
    pub completed: bool,
    pub domain_guard_violation: bool,
    pub blowup_terminated: bool,
}

impl ExponentialAffineTransformIntegrationStatusFlags {
    pub const fn from_status(status: ExponentialAffineTransformIntegrationStatus) -> Self {
        match status {
            ExponentialAffineTransformIntegrationStatus::Completed => Self {
                completed: true,
                domain_guard_violation: false,
                blowup_terminated: false,
            },
            ExponentialAffineTransformIntegrationStatus::DomainGuardViolation => Self {
                completed: false,
                domain_guard_violation: true,
                blowup_terminated: false,
            },
            ExponentialAffineTransformIntegrationStatus::BlowupTerminated => Self {
                completed: false,
                domain_guard_violation: false,
                blowup_terminated: true,
            },
        }
    }
}

/// Machine-readable diagnostics schema metadata for integration outputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ExponentialAffineTransformIntegrationOutputSchemaMetadata {
    pub schema_version: &'static str,
    pub integration_metadata_schema_version: &'static str,
    pub guard_failure_schema_version: &'static str,
    pub telemetry_schema_version: &'static str,
    pub required_result_fields: &'static [&'static str],
    pub required_metadata_fields: &'static [&'static str],
    pub required_guard_failure_fields: &'static [&'static str],
    pub required_telemetry_fields: &'static [&'static str],
}

pub const EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_OUTPUT_SCHEMA_METADATA:
    ExponentialAffineTransformIntegrationOutputSchemaMetadata =
    ExponentialAffineTransformIntegrationOutputSchemaMetadata {
        schema_version: EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_OUTPUT_SCHEMA_METADATA_VERSION,
        integration_metadata_schema_version:
            EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_METADATA_SCHEMA_VERSION,
        guard_failure_schema_version: EXPONENTIAL_AFFINE_TRANSFORM_GUARD_FAILURE_SCHEMA_VERSION,
        telemetry_schema_version: EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_TELEMETRY_SCHEMA_VERSION,
        required_result_fields: &EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_RESULT_FIELDS,
        required_metadata_fields:
            &EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_METADATA_FIELDS,
        required_guard_failure_fields:
            &EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_GUARD_FAILURE_FIELDS,
        required_telemetry_fields:
            &EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_TELEMETRY_FIELDS,
    };

/// Optional debug telemetry payload for integration quality triage.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ExponentialAffineTransformIntegrationTelemetry {
    pub schema_version: &'static str,
    pub checkpoint_count: usize,
    pub max_abs_c_value: Option<f64>,
    pub max_abs_d_component: Option<f64>,
    pub max_d_dot_r_real: Option<f64>,
    pub min_domain_margin: Option<f64>,
    pub guard_triggered: bool,
    pub guard_trigger: Option<ExponentialAffineTransformGuardFailure>,
}

/// Metadata describing one completed affine transform integration run.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ExponentialAffineTransformIntegrationMetadata {
    pub schema_version: &'static str,
    pub integrator_method: ExponentialAffineTransformIntegratorMethod,
    pub telemetry_mode: ExponentialAffineTransformTelemetryMode,
    pub status_flags: ExponentialAffineTransformIntegrationStatusFlags,
    pub configured_step_count: usize,
    pub completed_step_count: usize,
    pub step_size: f64,
    pub rhs_evaluation_count: usize,
    pub domain_guard_policy: &'static str,
    pub blowup_termination_policy: &'static str,
    pub guard_failure: Option<ExponentialAffineTransformGuardFailure>,
}

/// Result payload for one affine transform solve call.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ExponentialAffineTransformIntegrationResult {
    pub schema_version: &'static str,
    pub output_schema_metadata: ExponentialAffineTransformIntegrationOutputSchemaMetadata,
    pub status: ExponentialAffineTransformIntegrationStatus,
    pub metadata: ExponentialAffineTransformIntegrationMetadata,
    pub output: Option<ExponentialAffineTransformOutput>,
    pub telemetry: Option<ExponentialAffineTransformIntegrationTelemetry>,
}

/// Structured failure modes for affine transform solver execution.
#[derive(Debug, Clone, PartialEq)]
pub enum ExponentialAffineTransformSolverError {
    InvalidInput {
        source: ExponentialAffineTransformInputError,
    },
    InvalidSolverSettings {
        source: ExponentialAffineTransformSolverSettingsError,
    },
    InvalidOutput {
        source: ExponentialAffineTransformOutputError,
    },
}

impl Display for ExponentialAffineTransformSolverError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput { source } => {
                write!(
                    formatter,
                    "affine transform solver input is invalid: {source}"
                )
            }
            Self::InvalidSolverSettings { source } => write!(
                formatter,
                "affine transform solver settings are invalid: {source}"
            ),
            Self::InvalidOutput { source } => write!(
                formatter,
                "affine transform solver produced invalid output: {source}"
            ),
        }
    }
}

impl Error for ExponentialAffineTransformSolverError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidInput { source } => Some(source),
            Self::InvalidSolverSettings { source } => Some(source),
            Self::InvalidOutput { source } => Some(source),
        }
    }
}

/// Machine-readable output schema metadata for method-comparison reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ExponentialAffineTransformMethodComparisonOutputSchemaMetadata {
    pub schema_version: &'static str,
    pub required_report_fields: &'static [&'static str],
    pub required_tolerance_band_fields: &'static [&'static str],
    pub required_tolerance_window_fields: &'static [&'static str],
    pub required_run_summary_fields: &'static [&'static str],
    pub required_alternative_run_fields: &'static [&'static str],
    pub required_output_difference_fields: &'static [&'static str],
}

pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_OUTPUT_SCHEMA_METADATA:
    ExponentialAffineTransformMethodComparisonOutputSchemaMetadata =
    ExponentialAffineTransformMethodComparisonOutputSchemaMetadata {
        schema_version:
            EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_OUTPUT_SCHEMA_METADATA_VERSION,
        required_report_fields:
            &EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REQUIRED_REPORT_FIELDS,
        required_tolerance_band_fields:
            &EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REQUIRED_TOLERANCE_BAND_FIELDS,
        required_tolerance_window_fields:
            &EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REQUIRED_TOLERANCE_WINDOW_FIELDS,
        required_run_summary_fields:
            &EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REQUIRED_RUN_SUMMARY_FIELDS,
        required_alternative_run_fields:
            &EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REQUIRED_ALTERNATIVE_RUN_FIELDS,
        required_output_difference_fields:
            &EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REQUIRED_OUTPUT_DIFFERENCE_FIELDS,
    };

/// Method and step configuration used by the method-comparison harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExponentialAffineTransformMethodComparisonSolverSettings {
    pub integrator_method: ExponentialAffineTransformIntegratorMethod,
    pub step_count: usize,
}

impl ExponentialAffineTransformMethodComparisonSolverSettings {
    pub const fn new(
        integrator_method: ExponentialAffineTransformIntegratorMethod,
        step_count: usize,
    ) -> Self {
        Self {
            integrator_method,
            step_count,
        }
    }

    pub fn try_new(
        integrator_method: ExponentialAffineTransformIntegratorMethod,
        step_count: usize,
    ) -> Result<Self, ExponentialAffineTransformSolverSettingsError> {
        let settings = Self::new(integrator_method, step_count);
        settings.validate()?;
        Ok(settings)
    }

    pub fn validate(self) -> Result<(), ExponentialAffineTransformSolverSettingsError> {
        ExponentialAffineTransformSolverSettings::new(self.integrator_method, self.step_count)
            .validate()
    }

    pub fn to_solver_settings(self) -> ExponentialAffineTransformSolverSettings {
        ExponentialAffineTransformSolverSettings::new(self.integrator_method, self.step_count)
    }
}

/// Numeric tolerances used when comparing solver outputs across integrator methods.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExponentialAffineTransformMethodComparisonToleranceWindow {
    pub c_at_delta_absolute_tolerance: f64,
    pub d_component_absolute_tolerance: f64,
    pub transform_value_absolute_tolerance: f64,
}

impl ExponentialAffineTransformMethodComparisonToleranceWindow {
    pub fn new(
        c_at_delta_absolute_tolerance: f64,
        d_component_absolute_tolerance: f64,
        transform_value_absolute_tolerance: f64,
    ) -> Self {
        Self {
            c_at_delta_absolute_tolerance,
            d_component_absolute_tolerance,
            transform_value_absolute_tolerance,
        }
    }

    pub fn try_new(
        c_at_delta_absolute_tolerance: f64,
        d_component_absolute_tolerance: f64,
        transform_value_absolute_tolerance: f64,
    ) -> Result<Self, ExponentialAffineTransformMethodComparisonToleranceWindowError> {
        let tolerance_window = Self::new(
            c_at_delta_absolute_tolerance,
            d_component_absolute_tolerance,
            transform_value_absolute_tolerance,
        );
        tolerance_window.validate()?;
        Ok(tolerance_window)
    }

    pub fn validate(
        &self,
    ) -> Result<(), ExponentialAffineTransformMethodComparisonToleranceWindowError> {
        validate_positive_finite_tolerance_value(
            "c_at_delta_absolute_tolerance",
            self.c_at_delta_absolute_tolerance,
        )?;
        validate_positive_finite_tolerance_value(
            "d_component_absolute_tolerance",
            self.d_component_absolute_tolerance,
        )?;
        validate_positive_finite_tolerance_value(
            "transform_value_absolute_tolerance",
            self.transform_value_absolute_tolerance,
        )?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExponentialAffineTransformMethodComparisonToleranceWindowError {
    InvalidToleranceValue { field: &'static str, value: f64 },
}

impl Display for ExponentialAffineTransformMethodComparisonToleranceWindowError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidToleranceValue { field, value } => write!(
                formatter,
                "{field} must be finite and strictly positive, found {value}"
            ),
        }
    }
}

impl Error for ExponentialAffineTransformMethodComparisonToleranceWindowError {}

/// Interval-specific tolerance band for method comparison.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExponentialAffineTransformMethodComparisonIntervalToleranceBand {
    pub interval_grid_label: String,
    pub delta_upper_bound: f64,
    pub tolerance_window: ExponentialAffineTransformMethodComparisonToleranceWindow,
}

impl ExponentialAffineTransformMethodComparisonIntervalToleranceBand {
    pub fn new(
        interval_grid_label: String,
        delta_upper_bound: f64,
        tolerance_window: ExponentialAffineTransformMethodComparisonToleranceWindow,
    ) -> Self {
        Self {
            interval_grid_label,
            delta_upper_bound,
            tolerance_window,
        }
    }

    pub fn try_new(
        interval_grid_label: String,
        delta_upper_bound: f64,
        tolerance_window: ExponentialAffineTransformMethodComparisonToleranceWindow,
    ) -> Result<Self, ExponentialAffineTransformMethodComparisonToleranceBandError> {
        let tolerance_band = Self::new(interval_grid_label, delta_upper_bound, tolerance_window);
        tolerance_band.validate()?;
        Ok(tolerance_band)
    }

    pub fn validate(
        &self,
    ) -> Result<(), ExponentialAffineTransformMethodComparisonToleranceBandError> {
        if self.interval_grid_label.trim().is_empty() {
            return Err(
                ExponentialAffineTransformMethodComparisonToleranceBandError::EmptyIntervalGridLabel,
            );
        }

        if !self.delta_upper_bound.is_finite() || self.delta_upper_bound <= 0.0 {
            return Err(
                ExponentialAffineTransformMethodComparisonToleranceBandError::InvalidDeltaUpperBound {
                    value: self.delta_upper_bound,
                },
            );
        }

        self.tolerance_window.validate().map_err(|source| {
            ExponentialAffineTransformMethodComparisonToleranceBandError::InvalidToleranceWindow {
                source,
            }
        })?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExponentialAffineTransformMethodComparisonToleranceBandError {
    EmptyIntervalGridLabel,
    InvalidDeltaUpperBound {
        value: f64,
    },
    InvalidToleranceWindow {
        source: ExponentialAffineTransformMethodComparisonToleranceWindowError,
    },
}

impl Display for ExponentialAffineTransformMethodComparisonToleranceBandError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyIntervalGridLabel => {
                write!(formatter, "interval_grid_label must be a non-empty string")
            }
            Self::InvalidDeltaUpperBound { value } => write!(
                formatter,
                "delta_upper_bound must be finite and strictly positive, found {value}"
            ),
            Self::InvalidToleranceWindow { source } => write!(formatter, "{source}"),
        }
    }
}

impl Error for ExponentialAffineTransformMethodComparisonToleranceBandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidToleranceWindow { source } => Some(source),
            _ => None,
        }
    }
}

/// Versioned changelog entry for tolerance policy updates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExponentialAffineTransformMethodComparisonPolicyChangeLogEntry {
    pub policy_version: String,
    pub recorded_at_utc: String,
    pub rationale: String,
}

impl ExponentialAffineTransformMethodComparisonPolicyChangeLogEntry {
    pub fn new(policy_version: String, recorded_at_utc: String, rationale: String) -> Self {
        Self {
            policy_version,
            recorded_at_utc,
            rationale,
        }
    }

    pub fn try_new(
        policy_version: String,
        recorded_at_utc: String,
        rationale: String,
    ) -> Result<Self, ExponentialAffineTransformMethodComparisonPolicyChangeLogEntryError> {
        let change_log_entry = Self::new(policy_version, recorded_at_utc, rationale);
        change_log_entry.validate()?;
        Ok(change_log_entry)
    }

    pub fn validate(
        &self,
    ) -> Result<(), ExponentialAffineTransformMethodComparisonPolicyChangeLogEntryError> {
        if self.policy_version.trim().is_empty() {
            return Err(
                ExponentialAffineTransformMethodComparisonPolicyChangeLogEntryError::EmptyPolicyVersion,
            );
        }

        if self.recorded_at_utc.trim().is_empty() {
            return Err(
                ExponentialAffineTransformMethodComparisonPolicyChangeLogEntryError::EmptyRecordedAtUtc,
            );
        }

        if self.rationale.trim().is_empty() {
            return Err(
                ExponentialAffineTransformMethodComparisonPolicyChangeLogEntryError::EmptyRationale,
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExponentialAffineTransformMethodComparisonPolicyChangeLogEntryError {
    EmptyPolicyVersion,
    EmptyRecordedAtUtc,
    EmptyRationale,
}

impl Display for ExponentialAffineTransformMethodComparisonPolicyChangeLogEntryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyPolicyVersion => {
                write!(formatter, "policy_version must be a non-empty string")
            }
            Self::EmptyRecordedAtUtc => {
                write!(formatter, "recorded_at_utc must be a non-empty string")
            }
            Self::EmptyRationale => write!(formatter, "rationale must be a non-empty string"),
        }
    }
}

impl Error for ExponentialAffineTransformMethodComparisonPolicyChangeLogEntryError {}

/// Versioned tolerance policy for method comparison checks.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExponentialAffineTransformMethodComparisonTolerancePolicy {
    pub schema_version: String,
    pub policy_version: String,
    pub baseline_solver_settings: ExponentialAffineTransformMethodComparisonSolverSettings,
    pub alternative_solver_settings: Vec<ExponentialAffineTransformMethodComparisonSolverSettings>,
    pub interval_tolerance_bands:
        Vec<ExponentialAffineTransformMethodComparisonIntervalToleranceBand>,
    pub change_log: Vec<ExponentialAffineTransformMethodComparisonPolicyChangeLogEntry>,
}

impl ExponentialAffineTransformMethodComparisonTolerancePolicy {
    pub fn new(
        schema_version: String,
        policy_version: String,
        baseline_solver_settings: ExponentialAffineTransformMethodComparisonSolverSettings,
        alternative_solver_settings: Vec<ExponentialAffineTransformMethodComparisonSolverSettings>,
        interval_tolerance_bands: Vec<
            ExponentialAffineTransformMethodComparisonIntervalToleranceBand,
        >,
        change_log: Vec<ExponentialAffineTransformMethodComparisonPolicyChangeLogEntry>,
    ) -> Self {
        Self {
            schema_version,
            policy_version,
            baseline_solver_settings,
            alternative_solver_settings,
            interval_tolerance_bands,
            change_log,
        }
    }

    pub fn try_new(
        schema_version: String,
        policy_version: String,
        baseline_solver_settings: ExponentialAffineTransformMethodComparisonSolverSettings,
        alternative_solver_settings: Vec<ExponentialAffineTransformMethodComparisonSolverSettings>,
        interval_tolerance_bands: Vec<
            ExponentialAffineTransformMethodComparisonIntervalToleranceBand,
        >,
        change_log: Vec<ExponentialAffineTransformMethodComparisonPolicyChangeLogEntry>,
    ) -> Result<Self, ExponentialAffineTransformMethodComparisonTolerancePolicyError> {
        let tolerance_policy = Self::new(
            schema_version,
            policy_version,
            baseline_solver_settings,
            alternative_solver_settings,
            interval_tolerance_bands,
            change_log,
        );
        tolerance_policy.validate()?;
        Ok(tolerance_policy)
    }

    pub fn validate(
        &self,
    ) -> Result<(), ExponentialAffineTransformMethodComparisonTolerancePolicyError> {
        if self.schema_version
            != EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_TOLERANCE_POLICY_SCHEMA_VERSION
        {
            return Err(
                ExponentialAffineTransformMethodComparisonTolerancePolicyError::InvalidSchemaVersion {
                    expected:
                        EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_TOLERANCE_POLICY_SCHEMA_VERSION,
                    actual: self.schema_version.clone(),
                },
            );
        }

        if self.policy_version.trim().is_empty() {
            return Err(
                ExponentialAffineTransformMethodComparisonTolerancePolicyError::EmptyPolicyVersion,
            );
        }

        self.baseline_solver_settings
            .validate()
            .map_err(|source| {
                ExponentialAffineTransformMethodComparisonTolerancePolicyError::InvalidBaselineSolverSettings {
                    source,
                }
            })?;

        if self.alternative_solver_settings.is_empty() {
            return Err(
                ExponentialAffineTransformMethodComparisonTolerancePolicyError::EmptyAlternativeSolverSettings,
            );
        }

        let mut observed_settings_identities = BTreeSet::new();
        for (alternative_position, alternative_settings) in
            self.alternative_solver_settings.iter().enumerate()
        {
            alternative_settings.validate().map_err(|source| {
                ExponentialAffineTransformMethodComparisonTolerancePolicyError::InvalidAlternativeSolverSettings {
                    position: alternative_position,
                    source,
                }
            })?;

            if *alternative_settings == self.baseline_solver_settings {
                return Err(
                    ExponentialAffineTransformMethodComparisonTolerancePolicyError::AlternativeMatchesBaselineSettings {
                        integrator_method: alternative_settings.integrator_method,
                        step_count: alternative_settings.step_count,
                    },
                );
            }

            let settings_identity =
                method_comparison_solver_settings_identity(*alternative_settings);
            if !observed_settings_identities.insert(settings_identity) {
                return Err(
                    ExponentialAffineTransformMethodComparisonTolerancePolicyError::DuplicateAlternativeSolverSettings {
                        integrator_method: alternative_settings.integrator_method,
                        step_count: alternative_settings.step_count,
                    },
                );
            }
        }

        if self.interval_tolerance_bands.is_empty() {
            return Err(
                ExponentialAffineTransformMethodComparisonTolerancePolicyError::EmptyIntervalToleranceBands,
            );
        }

        let mut previous_delta_upper_bound: Option<f64> = None;
        for (band_position, tolerance_band) in self.interval_tolerance_bands.iter().enumerate() {
            tolerance_band.validate().map_err(|source| {
                ExponentialAffineTransformMethodComparisonTolerancePolicyError::InvalidIntervalToleranceBand {
                    position: band_position,
                    source,
                }
            })?;

            if let Some(previous_delta_upper_bound) = previous_delta_upper_bound {
                if tolerance_band.delta_upper_bound <= previous_delta_upper_bound {
                    return Err(
                        ExponentialAffineTransformMethodComparisonTolerancePolicyError::NonIncreasingDeltaUpperBound {
                            previous: previous_delta_upper_bound,
                            current: tolerance_band.delta_upper_bound,
                        },
                    );
                }
            }

            previous_delta_upper_bound = Some(tolerance_band.delta_upper_bound);
        }

        if self.change_log.is_empty() {
            return Err(
                ExponentialAffineTransformMethodComparisonTolerancePolicyError::EmptyChangeLog,
            );
        }

        let mut observed_policy_versions = BTreeSet::new();
        let mut has_current_policy_version = false;
        for (change_log_position, change_log_entry) in self.change_log.iter().enumerate() {
            change_log_entry.validate().map_err(|source| {
                ExponentialAffineTransformMethodComparisonTolerancePolicyError::InvalidChangeLogEntry {
                    position: change_log_position,
                    source,
                }
            })?;

            if !observed_policy_versions.insert(change_log_entry.policy_version.clone()) {
                return Err(
                    ExponentialAffineTransformMethodComparisonTolerancePolicyError::DuplicateChangeLogPolicyVersion {
                        policy_version: change_log_entry.policy_version.clone(),
                    },
                );
            }

            has_current_policy_version = has_current_policy_version
                || change_log_entry.policy_version == self.policy_version;
        }

        if !has_current_policy_version {
            return Err(
                ExponentialAffineTransformMethodComparisonTolerancePolicyError::MissingCurrentPolicyVersionInChangeLog {
                    policy_version: self.policy_version.clone(),
                },
            );
        }

        Ok(())
    }

    pub fn select_tolerance_band_for_delta(
        &self,
        delta: f64,
    ) -> Result<
        ExponentialAffineTransformMethodComparisonIntervalToleranceBand,
        ExponentialAffineTransformMethodComparisonTolerancePolicyError,
    > {
        for tolerance_band in &self.interval_tolerance_bands {
            if delta <= tolerance_band.delta_upper_bound {
                return Ok(tolerance_band.clone());
            }
        }

        let maximum_supported_delta = self
            .interval_tolerance_bands
            .last()
            .map_or(0.0, |tolerance_band| tolerance_band.delta_upper_bound);
        Err(
            ExponentialAffineTransformMethodComparisonTolerancePolicyError::DeltaOutsideToleranceBands {
                delta,
                max_supported_delta: maximum_supported_delta,
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExponentialAffineTransformMethodComparisonTolerancePolicyError {
    InvalidSchemaVersion {
        expected: &'static str,
        actual: String,
    },
    EmptyPolicyVersion,
    InvalidBaselineSolverSettings {
        source: ExponentialAffineTransformSolverSettingsError,
    },
    EmptyAlternativeSolverSettings,
    InvalidAlternativeSolverSettings {
        position: usize,
        source: ExponentialAffineTransformSolverSettingsError,
    },
    AlternativeMatchesBaselineSettings {
        integrator_method: ExponentialAffineTransformIntegratorMethod,
        step_count: usize,
    },
    DuplicateAlternativeSolverSettings {
        integrator_method: ExponentialAffineTransformIntegratorMethod,
        step_count: usize,
    },
    EmptyIntervalToleranceBands,
    InvalidIntervalToleranceBand {
        position: usize,
        source: ExponentialAffineTransformMethodComparisonToleranceBandError,
    },
    NonIncreasingDeltaUpperBound {
        previous: f64,
        current: f64,
    },
    EmptyChangeLog,
    InvalidChangeLogEntry {
        position: usize,
        source: ExponentialAffineTransformMethodComparisonPolicyChangeLogEntryError,
    },
    DuplicateChangeLogPolicyVersion {
        policy_version: String,
    },
    MissingCurrentPolicyVersionInChangeLog {
        policy_version: String,
    },
    DeltaOutsideToleranceBands {
        delta: f64,
        max_supported_delta: f64,
    },
}

impl Display for ExponentialAffineTransformMethodComparisonTolerancePolicyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSchemaVersion { expected, actual } => write!(
                formatter,
                "unsupported method-comparison tolerance policy schema_version: expected {expected}, found {actual}"
            ),
            Self::EmptyPolicyVersion => {
                write!(formatter, "policy_version must be a non-empty string")
            }
            Self::InvalidBaselineSolverSettings { source } => {
                write!(formatter, "baseline_solver_settings are invalid: {source}")
            }
            Self::EmptyAlternativeSolverSettings => write!(
                formatter,
                "alternative_solver_settings must include at least one method configuration"
            ),
            Self::InvalidAlternativeSolverSettings { position, source } => write!(
                formatter,
                "alternative_solver_settings[{position}] is invalid: {source}"
            ),
            Self::AlternativeMatchesBaselineSettings {
                integrator_method,
                step_count,
            } => write!(
                formatter,
                "alternative solver settings must differ from baseline settings, found method={integrator_method:?} step_count={step_count}"
            ),
            Self::DuplicateAlternativeSolverSettings {
                integrator_method,
                step_count,
            } => write!(
                formatter,
                "alternative solver settings must be unique, found duplicate method={integrator_method:?} step_count={step_count}"
            ),
            Self::EmptyIntervalToleranceBands => write!(
                formatter,
                "interval_tolerance_bands must include at least one tolerance band"
            ),
            Self::InvalidIntervalToleranceBand { position, source } => write!(
                formatter,
                "interval_tolerance_bands[{position}] is invalid: {source}"
            ),
            Self::NonIncreasingDeltaUpperBound { previous, current } => write!(
                formatter,
                "interval_tolerance_bands delta_upper_bound values must be strictly increasing, found previous={previous}, current={current}"
            ),
            Self::EmptyChangeLog => {
                write!(
                    formatter,
                    "change_log must include at least one policy history entry"
                )
            }
            Self::InvalidChangeLogEntry { position, source } => {
                write!(formatter, "change_log[{position}] is invalid: {source}")
            }
            Self::DuplicateChangeLogPolicyVersion { policy_version } => write!(
                formatter,
                "change_log contains duplicate policy_version: {policy_version}"
            ),
            Self::MissingCurrentPolicyVersionInChangeLog { policy_version } => write!(
                formatter,
                "change_log must include an entry for policy_version '{policy_version}'"
            ),
            Self::DeltaOutsideToleranceBands {
                delta,
                max_supported_delta,
            } => write!(
                formatter,
                "delta {delta} is outside configured tolerance bands, maximum supported delta is {max_supported_delta}"
            ),
        }
    }
}

impl Error for ExponentialAffineTransformMethodComparisonTolerancePolicyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidBaselineSolverSettings { source } => Some(source),
            Self::InvalidAlternativeSolverSettings { source, .. } => Some(source),
            Self::InvalidIntervalToleranceBand { source, .. } => Some(source),
            Self::InvalidChangeLogEntry { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Per-method run summary emitted in method-comparison reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ExponentialAffineTransformMethodComparisonRunSummary {
    pub integrator_method: ExponentialAffineTransformIntegratorMethod,
    pub step_count: usize,
    pub status: ExponentialAffineTransformIntegrationStatus,
    pub guard_failure_code: Option<ExponentialAffineTransformGuardFailureCode>,
}

/// Absolute output differences measured against baseline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ExponentialAffineTransformMethodComparisonOutputDifference {
    pub c_at_delta_absolute_difference: f64,
    pub d_component_absolute_difference: f64,
    pub transform_value_absolute_difference: f64,
}

/// Machine-readable reason codes for alternative-run method comparison failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExponentialAffineTransformMethodComparisonFailureCode {
    StatusMismatch,
    BaselineNotCompleted,
    AlternativeNotCompleted,
    BaselineOutputMissing,
    AlternativeOutputMissing,
    CAtDeltaToleranceExceeded,
    DComponentToleranceExceeded,
    TransformValueToleranceExceeded,
}

/// Comparison result for one alternative method configuration.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExponentialAffineTransformMethodComparisonAlternativeRunReport {
    pub run_summary: ExponentialAffineTransformMethodComparisonRunSummary,
    pub status_matches_baseline: bool,
    pub output_difference: Option<ExponentialAffineTransformMethodComparisonOutputDifference>,
    pub tolerance_window: ExponentialAffineTransformMethodComparisonToleranceWindow,
    pub passed: bool,
    pub failure_codes: Vec<ExponentialAffineTransformMethodComparisonFailureCode>,
}

/// Full method-comparison report for one transform input.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExponentialAffineTransformMethodComparisonReport {
    pub schema_version: &'static str,
    pub output_schema_metadata: ExponentialAffineTransformMethodComparisonOutputSchemaMetadata,
    pub policy_schema_version: String,
    pub policy_version: String,
    pub agreement_policy: &'static str,
    pub interval_selection_policy: &'static str,
    pub reference_fixture_policy: &'static str,
    pub input_delta: f64,
    pub tolerance_band: ExponentialAffineTransformMethodComparisonIntervalToleranceBand,
    pub baseline_run: ExponentialAffineTransformMethodComparisonRunSummary,
    pub alternative_runs: Vec<ExponentialAffineTransformMethodComparisonAlternativeRunReport>,
    pub all_checks_passed: bool,
}

/// Structured failure modes for method-comparison harness execution.
#[derive(Debug, Clone, PartialEq)]
pub enum ExponentialAffineTransformMethodComparisonError {
    InvalidTolerancePolicy {
        source: ExponentialAffineTransformMethodComparisonTolerancePolicyError,
    },
    BaselineSolverExecutionFailed {
        source: ExponentialAffineTransformSolverError,
    },
    AlternativeSolverExecutionFailed {
        alternative_position: usize,
        source: ExponentialAffineTransformSolverError,
    },
}

impl Display for ExponentialAffineTransformMethodComparisonError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidTolerancePolicy { source } => write!(
                formatter,
                "method-comparison tolerance policy is invalid: {source}"
            ),
            Self::BaselineSolverExecutionFailed { source } => {
                write!(formatter, "baseline solver execution failed: {source}")
            }
            Self::AlternativeSolverExecutionFailed {
                alternative_position,
                source,
            } => write!(
                formatter,
                "alternative solver execution failed for alternative_solver_settings[{alternative_position}]: {source}"
            ),
        }
    }
}

impl Error for ExponentialAffineTransformMethodComparisonError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidTolerancePolicy { source } => Some(source),
            Self::BaselineSolverExecutionFailed { source } => Some(source),
            Self::AlternativeSolverExecutionFailed { source, .. } => Some(source),
        }
    }
}

/// Compare solver outputs across configured integration methods under one tolerance policy.
///
/// This harness complements the deterministic reference fixture suite and does not replace it.
pub fn compare_exponential_affine_transform_integration_methods(
    transform_input: ExponentialAffineTransformInput,
    tolerance_policy: ExponentialAffineTransformMethodComparisonTolerancePolicy,
) -> Result<
    ExponentialAffineTransformMethodComparisonReport,
    ExponentialAffineTransformMethodComparisonError,
> {
    tolerance_policy.validate().map_err(|source| {
        ExponentialAffineTransformMethodComparisonError::InvalidTolerancePolicy { source }
    })?;
    let tolerance_band = tolerance_policy
        .select_tolerance_band_for_delta(transform_input.delta)
        .map_err(|source| {
            ExponentialAffineTransformMethodComparisonError::InvalidTolerancePolicy { source }
        })?;

    let baseline_solver_settings = tolerance_policy
        .baseline_solver_settings
        .to_solver_settings();
    let baseline_result =
        solve_exponential_affine_transform_with_settings(transform_input, baseline_solver_settings)
            .map_err(|source| {
                ExponentialAffineTransformMethodComparisonError::BaselineSolverExecutionFailed {
                    source,
                }
            })?;
    let baseline_run_summary =
        method_comparison_run_summary(tolerance_policy.baseline_solver_settings, baseline_result);

    let mut alternative_runs = Vec::new();
    for (alternative_position, alternative_solver_settings) in tolerance_policy
        .alternative_solver_settings
        .iter()
        .enumerate()
    {
        let alternative_result = solve_exponential_affine_transform_with_settings(
            transform_input,
            alternative_solver_settings.to_solver_settings(),
        )
        .map_err(|source| {
            ExponentialAffineTransformMethodComparisonError::AlternativeSolverExecutionFailed {
                alternative_position,
                source,
            }
        })?;

        alternative_runs.push(evaluate_method_comparison_alternative_run(
            baseline_result,
            *alternative_solver_settings,
            alternative_result,
            tolerance_band.tolerance_window,
        ));
    }

    let all_checks_passed = alternative_runs
        .iter()
        .all(|alternative_run| alternative_run.passed);

    Ok(ExponentialAffineTransformMethodComparisonReport {
        schema_version: EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REPORT_SCHEMA_VERSION,
        output_schema_metadata:
            EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_OUTPUT_SCHEMA_METADATA,
        policy_schema_version: tolerance_policy.schema_version,
        policy_version: tolerance_policy.policy_version,
        agreement_policy: EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_AGREEMENT_POLICY,
        interval_selection_policy:
            EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_INTERVAL_SELECTION_POLICY,
        reference_fixture_policy:
            EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REFERENCE_FIXTURE_POLICY,
        input_delta: transform_input.delta,
        tolerance_band,
        baseline_run: baseline_run_summary,
        alternative_runs,
        all_checks_passed,
    })
}

/// Solve one-interval affine transform ODE system using default stable settings.
pub fn solve_exponential_affine_transform(
    transform_input: ExponentialAffineTransformInput,
) -> Result<ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformSolverError> {
    solve_exponential_affine_transform_with_optional_settings(transform_input, None)
}

/// Solve one-interval affine transform ODE system with explicit solver settings.
pub fn solve_exponential_affine_transform_with_settings(
    transform_input: ExponentialAffineTransformInput,
    solver_settings: ExponentialAffineTransformSolverSettings,
) -> Result<ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformSolverError> {
    solve_exponential_affine_transform_with_optional_settings(
        transform_input,
        Some(solver_settings),
    )
}

/// Solve one-interval affine transform ODE system through one consumer-neutral entrypoint.
///
/// Pricing and risk adapters call this function so solver behavior stays unified.
///
/// Pass `None` for `solver_settings` to use the default stable method.
///
/// # Examples
///
/// Pricing consumer
/// ```ignore
/// use janiform_core::pricing::solve_exponential_affine_transform;
///
/// let pricing_input = build_pricing_transform_input();
/// let solve_result = solve_exponential_affine_transform(pricing_input)?;
/// # Ok::<(), janiform_core::affine_oracle::ExponentialAffineTransformSolverError>(())
/// ```
///
/// Risk-sampling consumer with explicit settings
/// ```ignore
/// use janiform_core::affine_oracle::ExponentialAffineTransformSolverSettings;
/// use janiform_core::risk::solve_exponential_affine_transform_for_risk_sampling_with_settings;
///
/// let risk_input = build_risk_sampling_transform_input();
/// let solver_settings = ExponentialAffineTransformSolverSettings::default();
/// let solve_result = solve_exponential_affine_transform_for_risk_sampling_with_settings(
///     risk_input,
///     solver_settings,
/// )?;
/// # Ok::<(), janiform_core::affine_oracle::ExponentialAffineTransformSolverError>(())
/// ```
pub fn solve_exponential_affine_transform_with_optional_settings(
    transform_input: ExponentialAffineTransformInput,
    solver_settings: Option<ExponentialAffineTransformSolverSettings>,
) -> Result<ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformSolverError> {
    let effective_solver_settings = solver_settings.unwrap_or_default();

    transform_input
        .validate()
        .map_err(|source| ExponentialAffineTransformSolverError::InvalidInput { source })?;
    effective_solver_settings.validate().map_err(|source| {
        ExponentialAffineTransformSolverError::InvalidSolverSettings { source }
    })?;

    match effective_solver_settings.integrator_method {
        ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta => {
            run_affine_transform_integration_with_integrator(
                transform_input,
                effective_solver_settings,
                FourthOrderRungeKuttaIntegrator,
            )
        }
        ExponentialAffineTransformIntegratorMethod::ForwardEuler => {
            run_affine_transform_integration_with_integrator(
                transform_input,
                effective_solver_settings,
                ForwardEulerIntegrator,
            )
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ExponentialAffineTransformIntegrationState {
    d_vector: ExponentialAffineTransformDVector,
    c_value: ComplexScalar,
}

impl ExponentialAffineTransformIntegrationState {
    fn initial() -> Self {
        Self {
            d_vector: ExponentialAffineTransformDVector::new(
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
            ),
            c_value: ComplexScalar::new(0.0, 0.0),
        }
    }

    fn add_scaled_rhs(self, rhs: ExponentialAffineTransformOdeRhs, scale: f64) -> Self {
        Self {
            d_vector: self.d_vector.add(rhs.d_derivative.scale(scale)),
            c_value: self.c_value.add(rhs.c_derivative.scale(scale)),
        }
    }
}

type ExponentialAffineTransformOdeRhsEvaluator = fn(
    ExponentialAffineTransformInput,
    ExponentialAffineTransformDVector,
) -> ExponentialAffineTransformOdeRhs;

trait ExponentialAffineTransformIntegrator {
    fn method(&self) -> ExponentialAffineTransformIntegratorMethod;
    fn rhs_evaluations_per_step(&self) -> usize;
    fn advance_one_step(
        &self,
        transform_input: ExponentialAffineTransformInput,
        step_size: f64,
        state: ExponentialAffineTransformIntegrationState,
        rhs_evaluator: ExponentialAffineTransformOdeRhsEvaluator,
    ) -> ExponentialAffineTransformIntegrationState;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ForwardEulerIntegrator;

impl ExponentialAffineTransformIntegrator for ForwardEulerIntegrator {
    fn method(&self) -> ExponentialAffineTransformIntegratorMethod {
        ExponentialAffineTransformIntegratorMethod::ForwardEuler
    }

    fn rhs_evaluations_per_step(&self) -> usize {
        1
    }

    fn advance_one_step(
        &self,
        transform_input: ExponentialAffineTransformInput,
        step_size: f64,
        state: ExponentialAffineTransformIntegrationState,
        rhs_evaluator: ExponentialAffineTransformOdeRhsEvaluator,
    ) -> ExponentialAffineTransformIntegrationState {
        let first_derivative = rhs_evaluator(transform_input, state.d_vector);
        state.add_scaled_rhs(first_derivative, step_size)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FourthOrderRungeKuttaIntegrator;

impl ExponentialAffineTransformIntegrator for FourthOrderRungeKuttaIntegrator {
    fn method(&self) -> ExponentialAffineTransformIntegratorMethod {
        ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta
    }

    fn rhs_evaluations_per_step(&self) -> usize {
        4
    }

    fn advance_one_step(
        &self,
        transform_input: ExponentialAffineTransformInput,
        step_size: f64,
        state: ExponentialAffineTransformIntegrationState,
        rhs_evaluator: ExponentialAffineTransformOdeRhsEvaluator,
    ) -> ExponentialAffineTransformIntegrationState {
        let k1 = rhs_evaluator(transform_input, state.d_vector);
        let k2 = rhs_evaluator(
            transform_input,
            state.add_scaled_rhs(k1, 0.5 * step_size).d_vector,
        );
        let k3 = rhs_evaluator(
            transform_input,
            state.add_scaled_rhs(k2, 0.5 * step_size).d_vector,
        );
        let k4 = rhs_evaluator(
            transform_input,
            state.add_scaled_rhs(k3, step_size).d_vector,
        );

        let d_increment = k1
            .d_derivative
            .add(k2.d_derivative.scale(2.0))
            .add(k3.d_derivative.scale(2.0))
            .add(k4.d_derivative)
            .scale(step_size / 6.0);
        let c_increment = k1
            .c_derivative
            .add(k2.c_derivative.scale(2.0))
            .add(k3.c_derivative.scale(2.0))
            .add(k4.c_derivative)
            .scale(step_size / 6.0);

        ExponentialAffineTransformIntegrationState {
            d_vector: state.d_vector.add(d_increment),
            c_value: state.c_value.add(c_increment),
        }
    }
}

fn run_affine_transform_integration_with_integrator<TIntegrator>(
    transform_input: ExponentialAffineTransformInput,
    solver_settings: ExponentialAffineTransformSolverSettings,
    integrator: TIntegrator,
) -> Result<ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformSolverError>
where
    TIntegrator: ExponentialAffineTransformIntegrator,
{
    let step_size = transform_input.delta / solver_settings.step_count as f64;
    let mut integration_state = ExponentialAffineTransformIntegrationState::initial();
    let mut completed_step_count = 0_usize;
    let mut rhs_evaluation_count = 0_usize;
    let mut telemetry_accumulator =
        ExponentialAffineTransformTelemetryAccumulator::enabled_or_none(solver_settings);

    let initial_checkpoint = evaluate_integration_guard_checkpoint(
        transform_input,
        integration_state,
        completed_step_count,
        step_size,
    );

    if let Some(telemetry_accumulator) = telemetry_accumulator.as_mut() {
        telemetry_accumulator.record_checkpoint(integration_state, initial_checkpoint);
    }

    if let Some(guard_failure) = initial_checkpoint.guard_failure {
        let telemetry =
            telemetry_accumulator.map(ExponentialAffineTransformTelemetryAccumulator::to_payload);
        return Ok(terminated_result_from_guard_failure(
            solver_settings,
            integrator,
            completed_step_count,
            rhs_evaluation_count,
            step_size,
            guard_failure,
            telemetry,
        ));
    }

    for _ in 0..solver_settings.step_count {
        integration_state = integrator.advance_one_step(
            transform_input,
            step_size,
            integration_state,
            evaluate_exponential_affine_transform_ode_rhs,
        );
        completed_step_count += 1;
        rhs_evaluation_count =
            rhs_evaluation_count.saturating_add(integrator.rhs_evaluations_per_step());

        let checkpoint = evaluate_integration_guard_checkpoint(
            transform_input,
            integration_state,
            completed_step_count,
            step_size,
        );

        if let Some(telemetry_accumulator) = telemetry_accumulator.as_mut() {
            telemetry_accumulator.record_checkpoint(integration_state, checkpoint);
        }

        if let Some(guard_failure) = checkpoint.guard_failure {
            let telemetry = telemetry_accumulator
                .map(ExponentialAffineTransformTelemetryAccumulator::to_payload);
            return Ok(terminated_result_from_guard_failure(
                solver_settings,
                integrator,
                completed_step_count,
                rhs_evaluation_count,
                step_size,
                guard_failure,
                telemetry,
            ));
        }
    }

    let c_at_delta = integration_state.c_value;
    let d_at_delta = integration_state.d_vector;
    let transform_exponent = c_at_delta.add(d_at_delta.dot_state(transform_input.initial_state));
    let transform_value = complex_exponential(transform_exponent);
    let output = ExponentialAffineTransformOutput::try_new(c_at_delta, d_at_delta, transform_value)
        .map_err(|source| ExponentialAffineTransformSolverError::InvalidOutput { source })?;
    let status = ExponentialAffineTransformIntegrationStatus::Completed;
    let telemetry =
        telemetry_accumulator.map(ExponentialAffineTransformTelemetryAccumulator::to_payload);

    Ok(ExponentialAffineTransformIntegrationResult {
        schema_version: EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_RESULT_SCHEMA_VERSION,
        output_schema_metadata: EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_OUTPUT_SCHEMA_METADATA,
        status,
        metadata: build_integration_metadata(
            solver_settings,
            integrator.method(),
            status,
            completed_step_count,
            step_size,
            rhs_evaluation_count,
            None,
        ),
        output: Some(output),
        telemetry,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ExponentialAffineTransformGuardCheckpointEvaluation {
    guard_failure: Option<ExponentialAffineTransformGuardFailure>,
    d_dot_r: ComplexScalar,
    d_dot_r_upper_bound: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ExponentialAffineTransformTelemetryAccumulator {
    checkpoint_count: usize,
    max_abs_c_value: Option<f64>,
    max_abs_d_component: Option<f64>,
    max_d_dot_r_real: Option<f64>,
    min_domain_margin: Option<f64>,
    guard_trigger: Option<ExponentialAffineTransformGuardFailure>,
}

impl ExponentialAffineTransformTelemetryAccumulator {
    fn enabled_or_none(solver_settings: ExponentialAffineTransformSolverSettings) -> Option<Self> {
        match solver_settings.telemetry_mode {
            ExponentialAffineTransformTelemetryMode::Disabled => None,
            ExponentialAffineTransformTelemetryMode::Enabled => Some(Self {
                checkpoint_count: 0,
                max_abs_c_value: None,
                max_abs_d_component: None,
                max_d_dot_r_real: None,
                min_domain_margin: None,
                guard_trigger: None,
            }),
        }
    }

    fn record_checkpoint(
        &mut self,
        integration_state: ExponentialAffineTransformIntegrationState,
        checkpoint: ExponentialAffineTransformGuardCheckpointEvaluation,
    ) {
        self.checkpoint_count = self.checkpoint_count.saturating_add(1);

        if integration_state.c_value.is_finite() {
            self.max_abs_c_value =
                update_optional_max(self.max_abs_c_value, integration_state.c_value.norm());
        }

        if let Some(max_abs_d_component) = evaluate_max_abs_d_component(integration_state.d_vector)
        {
            self.max_abs_d_component =
                update_optional_max(self.max_abs_d_component, max_abs_d_component);
        }

        if checkpoint.d_dot_r.is_finite() {
            self.max_d_dot_r_real =
                update_optional_max(self.max_d_dot_r_real, checkpoint.d_dot_r.real);
            self.min_domain_margin = update_optional_min(
                self.min_domain_margin,
                checkpoint.d_dot_r_upper_bound - checkpoint.d_dot_r.real,
            );
        }

        if self.guard_trigger.is_none() {
            self.guard_trigger = checkpoint.guard_failure;
        }
    }

    fn to_payload(self) -> ExponentialAffineTransformIntegrationTelemetry {
        ExponentialAffineTransformIntegrationTelemetry {
            schema_version: EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_TELEMETRY_SCHEMA_VERSION,
            checkpoint_count: self.checkpoint_count,
            max_abs_c_value: self.max_abs_c_value,
            max_abs_d_component: self.max_abs_d_component,
            max_d_dot_r_real: self.max_d_dot_r_real,
            min_domain_margin: self.min_domain_margin,
            guard_triggered: self.guard_trigger.is_some(),
            guard_trigger: self.guard_trigger,
        }
    }
}

fn evaluate_integration_guard_checkpoint(
    transform_input: ExponentialAffineTransformInput,
    integration_state: ExponentialAffineTransformIntegrationState,
    completed_step_count: usize,
    step_size: f64,
) -> ExponentialAffineTransformGuardCheckpointEvaluation {
    let d_dot_r_upper_bound = 1.0
        / transform_input
            .equation_specification
            .gamma_severity_mark_configuration
            .scale_theta;
    let checkpoint_time = completed_step_count as f64 * step_size;

    if !integration_state.c_value.is_finite() || integration_state.d_vector.validate().is_err() {
        return ExponentialAffineTransformGuardCheckpointEvaluation {
            guard_failure: Some(build_guard_failure(
                ExponentialAffineTransformGuardFailureCode::NonFiniteState,
                completed_step_count,
                checkpoint_time,
                ComplexScalar::new(f64::NAN, f64::NAN),
                d_dot_r_upper_bound,
            )),
            d_dot_r: ComplexScalar::new(f64::NAN, f64::NAN),
            d_dot_r_upper_bound,
        };
    }

    let d_dot_r = integration_state
        .d_vector
        .dot_loadings(transform_input.equation_specification.r());
    if !d_dot_r.is_finite() {
        return ExponentialAffineTransformGuardCheckpointEvaluation {
            guard_failure: Some(build_guard_failure(
                ExponentialAffineTransformGuardFailureCode::NonFiniteDomainMetric,
                completed_step_count,
                checkpoint_time,
                d_dot_r,
                d_dot_r_upper_bound,
            )),
            d_dot_r,
            d_dot_r_upper_bound,
        };
    }

    if d_dot_r.real >= d_dot_r_upper_bound {
        return ExponentialAffineTransformGuardCheckpointEvaluation {
            guard_failure: Some(build_guard_failure(
                ExponentialAffineTransformGuardFailureCode::DomainUpperBoundViolation,
                completed_step_count,
                checkpoint_time,
                d_dot_r,
                d_dot_r_upper_bound,
            )),
            d_dot_r,
            d_dot_r_upper_bound,
        };
    }

    ExponentialAffineTransformGuardCheckpointEvaluation {
        guard_failure: None,
        d_dot_r,
        d_dot_r_upper_bound,
    }
}

fn terminated_result_from_guard_failure<TIntegrator>(
    solver_settings: ExponentialAffineTransformSolverSettings,
    integrator: TIntegrator,
    completed_step_count: usize,
    rhs_evaluation_count: usize,
    step_size: f64,
    guard_failure: ExponentialAffineTransformGuardFailure,
    telemetry: Option<ExponentialAffineTransformIntegrationTelemetry>,
) -> ExponentialAffineTransformIntegrationResult
where
    TIntegrator: ExponentialAffineTransformIntegrator,
{
    let status = status_from_guard_failure_code(guard_failure.code);

    ExponentialAffineTransformIntegrationResult {
        schema_version: EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_RESULT_SCHEMA_VERSION,
        output_schema_metadata: EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_OUTPUT_SCHEMA_METADATA,
        status,
        metadata: build_integration_metadata(
            solver_settings,
            integrator.method(),
            status,
            completed_step_count,
            step_size,
            rhs_evaluation_count,
            Some(guard_failure),
        ),
        output: None,
        telemetry,
    }
}

fn build_guard_failure(
    code: ExponentialAffineTransformGuardFailureCode,
    step_index: usize,
    time: f64,
    d_dot_r: ComplexScalar,
    d_dot_r_upper_bound: f64,
) -> ExponentialAffineTransformGuardFailure {
    ExponentialAffineTransformGuardFailure {
        schema_version: EXPONENTIAL_AFFINE_TRANSFORM_GUARD_FAILURE_SCHEMA_VERSION,
        code,
        step_index,
        time,
        d_dot_r,
        d_dot_r_upper_bound,
    }
}

fn build_integration_metadata(
    solver_settings: ExponentialAffineTransformSolverSettings,
    integrator_method: ExponentialAffineTransformIntegratorMethod,
    status: ExponentialAffineTransformIntegrationStatus,
    completed_step_count: usize,
    step_size: f64,
    rhs_evaluation_count: usize,
    guard_failure: Option<ExponentialAffineTransformGuardFailure>,
) -> ExponentialAffineTransformIntegrationMetadata {
    ExponentialAffineTransformIntegrationMetadata {
        schema_version: EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_METADATA_SCHEMA_VERSION,
        integrator_method,
        telemetry_mode: solver_settings.telemetry_mode,
        status_flags: ExponentialAffineTransformIntegrationStatusFlags::from_status(status),
        configured_step_count: solver_settings.step_count,
        completed_step_count,
        step_size,
        rhs_evaluation_count,
        domain_guard_policy: EXPONENTIAL_AFFINE_TRANSFORM_DOMAIN_GUARD_POLICY,
        blowup_termination_policy: EXPONENTIAL_AFFINE_TRANSFORM_BLOWUP_TERMINATION_POLICY,
        guard_failure,
    }
}

fn status_from_guard_failure_code(
    guard_failure_code: ExponentialAffineTransformGuardFailureCode,
) -> ExponentialAffineTransformIntegrationStatus {
    match guard_failure_code {
        ExponentialAffineTransformGuardFailureCode::DomainUpperBoundViolation => {
            ExponentialAffineTransformIntegrationStatus::DomainGuardViolation
        }
        ExponentialAffineTransformGuardFailureCode::NonFiniteState
        | ExponentialAffineTransformGuardFailureCode::NonFiniteDomainMetric => {
            ExponentialAffineTransformIntegrationStatus::BlowupTerminated
        }
    }
}

fn update_optional_max(current_value: Option<f64>, candidate_value: f64) -> Option<f64> {
    match current_value {
        Some(current_value) if candidate_value <= current_value => Some(current_value),
        _ => Some(candidate_value),
    }
}

fn update_optional_min(current_value: Option<f64>, candidate_value: f64) -> Option<f64> {
    match current_value {
        Some(current_value) if candidate_value >= current_value => Some(current_value),
        _ => Some(candidate_value),
    }
}

fn validate_positive_finite_tolerance_value(
    field: &'static str,
    value: f64,
) -> Result<(), ExponentialAffineTransformMethodComparisonToleranceWindowError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(
            ExponentialAffineTransformMethodComparisonToleranceWindowError::InvalidToleranceValue {
                field,
                value,
            },
        );
    }

    Ok(())
}

fn method_comparison_solver_settings_identity(
    settings: ExponentialAffineTransformMethodComparisonSolverSettings,
) -> String {
    format!(
        "{integrator_method}:{step_count}",
        integrator_method = method_comparison_integrator_method_label(settings.integrator_method),
        step_count = settings.step_count
    )
}

fn method_comparison_integrator_method_label(
    integrator_method: ExponentialAffineTransformIntegratorMethod,
) -> &'static str {
    match integrator_method {
        ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta => {
            "fourth_order_runge_kutta"
        }
        ExponentialAffineTransformIntegratorMethod::ForwardEuler => "forward_euler",
    }
}

fn method_comparison_run_summary(
    solver_settings: ExponentialAffineTransformMethodComparisonSolverSettings,
    integration_result: ExponentialAffineTransformIntegrationResult,
) -> ExponentialAffineTransformMethodComparisonRunSummary {
    ExponentialAffineTransformMethodComparisonRunSummary {
        integrator_method: solver_settings.integrator_method,
        step_count: solver_settings.step_count,
        status: integration_result.status,
        guard_failure_code: integration_result
            .metadata
            .guard_failure
            .map(|guard_failure| guard_failure.code),
    }
}

fn evaluate_method_comparison_alternative_run(
    baseline_result: ExponentialAffineTransformIntegrationResult,
    alternative_solver_settings: ExponentialAffineTransformMethodComparisonSolverSettings,
    alternative_result: ExponentialAffineTransformIntegrationResult,
    tolerance_window: ExponentialAffineTransformMethodComparisonToleranceWindow,
) -> ExponentialAffineTransformMethodComparisonAlternativeRunReport {
    let run_summary =
        method_comparison_run_summary(alternative_solver_settings, alternative_result);
    let status_matches_baseline = baseline_result.status == alternative_result.status;
    let mut failure_codes = Vec::new();

    if !status_matches_baseline {
        failure_codes.push(ExponentialAffineTransformMethodComparisonFailureCode::StatusMismatch);
    }
    if baseline_result.status != ExponentialAffineTransformIntegrationStatus::Completed {
        failure_codes
            .push(ExponentialAffineTransformMethodComparisonFailureCode::BaselineNotCompleted);
    }
    if alternative_result.status != ExponentialAffineTransformIntegrationStatus::Completed {
        failure_codes
            .push(ExponentialAffineTransformMethodComparisonFailureCode::AlternativeNotCompleted);
    }

    let output_difference = if baseline_result.status
        == ExponentialAffineTransformIntegrationStatus::Completed
        && alternative_result.status == ExponentialAffineTransformIntegrationStatus::Completed
    {
        match (baseline_result.output, alternative_result.output) {
            (Some(baseline_output), Some(alternative_output)) => {
                let output_difference = evaluate_method_comparison_output_difference(
                    baseline_output,
                    alternative_output,
                );

                if output_difference.c_at_delta_absolute_difference
                    > tolerance_window.c_at_delta_absolute_tolerance
                {
                    failure_codes.push(
                        ExponentialAffineTransformMethodComparisonFailureCode::CAtDeltaToleranceExceeded,
                    );
                }

                if output_difference.d_component_absolute_difference
                    > tolerance_window.d_component_absolute_tolerance
                {
                    failure_codes.push(
                        ExponentialAffineTransformMethodComparisonFailureCode::DComponentToleranceExceeded,
                    );
                }

                if output_difference.transform_value_absolute_difference
                    > tolerance_window.transform_value_absolute_tolerance
                {
                    failure_codes.push(
                        ExponentialAffineTransformMethodComparisonFailureCode::TransformValueToleranceExceeded,
                    );
                }

                Some(output_difference)
            }
            (None, Some(_)) => {
                failure_codes.push(
                    ExponentialAffineTransformMethodComparisonFailureCode::BaselineOutputMissing,
                );
                None
            }
            (Some(_), None) => {
                failure_codes.push(
                    ExponentialAffineTransformMethodComparisonFailureCode::AlternativeOutputMissing,
                );
                None
            }
            (None, None) => {
                failure_codes.push(
                    ExponentialAffineTransformMethodComparisonFailureCode::BaselineOutputMissing,
                );
                failure_codes.push(
                    ExponentialAffineTransformMethodComparisonFailureCode::AlternativeOutputMissing,
                );
                None
            }
        }
    } else {
        None
    };

    let passed = failure_codes.is_empty();

    ExponentialAffineTransformMethodComparisonAlternativeRunReport {
        run_summary,
        status_matches_baseline,
        output_difference,
        tolerance_window,
        passed,
        failure_codes,
    }
}

fn evaluate_method_comparison_output_difference(
    baseline_output: ExponentialAffineTransformOutput,
    alternative_output: ExponentialAffineTransformOutput,
) -> ExponentialAffineTransformMethodComparisonOutputDifference {
    let d_component_absolute_difference = complex_componentwise_absolute_difference(
        baseline_output.d_at_delta.plus_plus,
        alternative_output.d_at_delta.plus_plus,
    )
    .max(complex_componentwise_absolute_difference(
        baseline_output.d_at_delta.plus_minus,
        alternative_output.d_at_delta.plus_minus,
    ))
    .max(complex_componentwise_absolute_difference(
        baseline_output.d_at_delta.minus_plus,
        alternative_output.d_at_delta.minus_plus,
    ))
    .max(complex_componentwise_absolute_difference(
        baseline_output.d_at_delta.minus_minus,
        alternative_output.d_at_delta.minus_minus,
    ));

    ExponentialAffineTransformMethodComparisonOutputDifference {
        c_at_delta_absolute_difference: complex_componentwise_absolute_difference(
            baseline_output.c_at_delta,
            alternative_output.c_at_delta,
        ),
        d_component_absolute_difference,
        transform_value_absolute_difference: complex_componentwise_absolute_difference(
            baseline_output.transform_value,
            alternative_output.transform_value,
        ),
    }
}

fn complex_componentwise_absolute_difference(left: ComplexScalar, right: ComplexScalar) -> f64 {
    (left.real - right.real)
        .abs()
        .max((left.imaginary - right.imaginary).abs())
}

fn evaluate_max_abs_d_component(d_vector: ExponentialAffineTransformDVector) -> Option<f64> {
    if d_vector.validate().is_err() {
        return None;
    }

    Some(
        d_vector
            .plus_plus
            .norm()
            .max(d_vector.plus_minus.norm())
            .max(d_vector.minus_plus.norm())
            .max(d_vector.minus_minus.norm()),
    )
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

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExponentialAffineTransformMethodComparisonTolerancePolicyPayload {
    schema_version: String,
    policy_version: String,
    baseline_solver_settings: ExponentialAffineTransformMethodComparisonSolverSettings,
    alternative_solver_settings: Vec<ExponentialAffineTransformMethodComparisonSolverSettings>,
    interval_tolerance_bands: Vec<ExponentialAffineTransformMethodComparisonIntervalToleranceBand>,
    change_log: Vec<ExponentialAffineTransformMethodComparisonPolicyChangeLogEntry>,
}

impl<'de> Deserialize<'de> for ExponentialAffineTransformMethodComparisonTolerancePolicy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload =
            ExponentialAffineTransformMethodComparisonTolerancePolicyPayload::deserialize(
                deserializer,
            )?;
        Self::try_new(
            payload.schema_version,
            payload.policy_version,
            payload.baseline_solver_settings,
            payload.alternative_solver_settings,
            payload.interval_tolerance_bands,
            payload.change_log,
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
    use serde_json::{Value, json};

    use super::{
        EXPONENTIAL_AFFINE_TRANSFORM_BLOWUP_TERMINATION_POLICY,
        EXPONENTIAL_AFFINE_TRANSFORM_DEFAULT_SOLVER_POLICY,
        EXPONENTIAL_AFFINE_TRANSFORM_DOMAIN_GUARD_POLICY,
        EXPONENTIAL_AFFINE_TRANSFORM_EQUATION_MAPPING_POLICY,
        EXPONENTIAL_AFFINE_TRANSFORM_GUARD_FAILURE_SCHEMA_VERSION,
        EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_METADATA_SCHEMA_VERSION,
        EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_OUTPUT_SCHEMA_METADATA_VERSION,
        EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_GUARD_FAILURE_FIELDS,
        EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_METADATA_FIELDS,
        EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_RESULT_FIELDS,
        EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_TELEMETRY_FIELDS,
        EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_RESULT_SCHEMA_VERSION,
        EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_TELEMETRY_SCHEMA_VERSION,
        ExponentialAffineTransformDVector, ExponentialAffineTransformEquationSpecification,
        ExponentialAffineTransformGuardFailureCode, ExponentialAffineTransformInput,
        ExponentialAffineTransformIntegrationStatus, ExponentialAffineTransformIntegratorMethod,
        ExponentialAffineTransformOutput, ExponentialAffineTransformSolverError,
        ExponentialAffineTransformSolverSettings, ExponentialAffineTransformTelemetryMode,
        evaluate_exponential_affine_transform_hamiltonians,
        evaluate_exponential_affine_transform_ode_rhs, solve_exponential_affine_transform,
        solve_exponential_affine_transform_with_settings,
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

    fn linear_closed_form_fixture_input() -> ExponentialAffineTransformInput {
        ExponentialAffineTransformInput::try_new(
            ExponentialAffineTransformEquationSpecification::try_new(
                HawkesKernelParameters::try_new(
                    0.8,
                    1.2,
                    HawkesKernelLoadings {
                        plus_plus: 0.0,
                        plus_minus: 0.0,
                        minus_plus: 0.0,
                        minus_minus: 0.0,
                    },
                    HawkesKernelLoadings {
                        plus_plus: 0.0,
                        plus_minus: 0.0,
                        minus_plus: 0.0,
                        minus_minus: 0.0,
                    },
                )
                .expect("closed-form fixture kernel payload should construct"),
                GammaSeverityMarkConfiguration::try_new(1.8, 0.9)
                    .expect("closed-form fixture gamma payload should construct"),
            )
            .expect("closed-form fixture equation specification should construct"),
            ComplexScalar::new(0.2, 0.0),
            ComplexScalar::new(0.3, 0.0),
            ComplexScalar::new(1.0, 0.0),
            ComplexScalar::new(1.0, 0.0),
            1.1,
            0.0,
            1.0,
            HawkesExcitationState {
                y_plus_plus: 0.4,
                y_plus_minus: 0.2,
                y_minus_plus: 0.3,
                y_minus_minus: 0.5,
            },
        )
        .expect("closed-form fixture input should construct")
    }

    fn domain_violation_fixture_input() -> ExponentialAffineTransformInput {
        ExponentialAffineTransformInput::try_new(
            ExponentialAffineTransformEquationSpecification::try_new(
                HawkesKernelParameters::try_new(
                    0.8,
                    1.2,
                    HawkesKernelLoadings {
                        plus_plus: 0.0,
                        plus_minus: 0.0,
                        minus_plus: 0.0,
                        minus_minus: 0.0,
                    },
                    HawkesKernelLoadings {
                        plus_plus: 1.0,
                        plus_minus: 1.0,
                        minus_plus: 1.0,
                        minus_minus: 1.0,
                    },
                )
                .expect("domain violation fixture kernel payload should construct"),
                GammaSeverityMarkConfiguration::try_new(1.8, 0.9)
                    .expect("domain violation fixture gamma payload should construct"),
            )
            .expect("domain violation fixture equation specification should construct"),
            ComplexScalar::new(-10.0, 0.0),
            ComplexScalar::new(-10.0, 0.0),
            ComplexScalar::new(1.0, 0.0),
            ComplexScalar::new(1.0, 0.0),
            2.0,
            0.0,
            0.5,
            HawkesExcitationState {
                y_plus_plus: 0.4,
                y_plus_minus: 0.2,
                y_minus_plus: 0.3,
                y_minus_minus: 0.5,
            },
        )
        .expect("domain violation fixture input should construct")
    }

    fn blowup_fixture_input() -> ExponentialAffineTransformInput {
        ExponentialAffineTransformInput::try_new(
            ExponentialAffineTransformEquationSpecification::try_new(
                HawkesKernelParameters::try_new(
                    0.8,
                    1.2,
                    HawkesKernelLoadings {
                        plus_plus: 0.0,
                        plus_minus: 0.0,
                        minus_plus: 0.0,
                        minus_minus: 0.0,
                    },
                    HawkesKernelLoadings {
                        plus_plus: 0.01,
                        plus_minus: 0.02,
                        minus_plus: 0.03,
                        minus_minus: 0.04,
                    },
                )
                .expect("blowup fixture kernel payload should construct"),
                GammaSeverityMarkConfiguration::try_new(1.8, 0.9)
                    .expect("blowup fixture gamma payload should construct"),
            )
            .expect("blowup fixture equation specification should construct"),
            ComplexScalar::new(0.0, 0.0),
            ComplexScalar::new(0.0, 0.0),
            ComplexScalar::new(1.0e308, 0.0),
            ComplexScalar::new(1.0e308, 0.0),
            1.0,
            0.0,
            2.0,
            HawkesExcitationState {
                y_plus_plus: 0.4,
                y_plus_minus: 0.2,
                y_minus_plus: 0.3,
                y_minus_minus: 0.5,
            },
        )
        .expect("blowup fixture input should construct")
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
        assert_eq!(
            EXPONENTIAL_AFFINE_TRANSFORM_DEFAULT_SOLVER_POLICY,
            "fourth_order_runge_kutta_fixed_step.v1"
        );
        assert_eq!(
            EXPONENTIAL_AFFINE_TRANSFORM_DOMAIN_GUARD_POLICY,
            "enforce_d_dot_r_real_strict_upper_bound_at_runtime_checkpoints.v1"
        );
        assert_eq!(
            EXPONENTIAL_AFFINE_TRANSFORM_BLOWUP_TERMINATION_POLICY,
            "terminate_on_non_finite_state_or_guard_metric_without_clamping.v1"
        );
        assert_eq!(
            EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_RESULT_SCHEMA_VERSION,
            "affine_oracle.integration_result.v1"
        );
        assert_eq!(
            EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_METADATA_SCHEMA_VERSION,
            "affine_oracle.integration_metadata.v1"
        );
        assert_eq!(
            EXPONENTIAL_AFFINE_TRANSFORM_GUARD_FAILURE_SCHEMA_VERSION,
            "affine_oracle.integration_guard_failure.v1"
        );
        assert_eq!(
            EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_TELEMETRY_SCHEMA_VERSION,
            "affine_oracle.integration_telemetry.v1"
        );
        assert_eq!(
            EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_OUTPUT_SCHEMA_METADATA_VERSION,
            "affine_oracle.integration_output_schema_metadata.v1"
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

    #[test]
    fn solver_settings_validation_rejects_zero_step_count() {
        let settings_error = ExponentialAffineTransformSolverSettings::try_new(
            ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta,
            0,
        )
        .expect_err("zero step_count must be rejected during settings construction");
        assert!(
            settings_error
                .to_string()
                .contains("step_count must be strictly positive"),
            "unexpected settings error: {settings_error}"
        );

        let solver_error = solve_exponential_affine_transform_with_settings(
            valid_input(),
            ExponentialAffineTransformSolverSettings::new(
                ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta,
                0,
            ),
        )
        .expect_err("solver execution must reject invalid settings before runtime");
        assert!(
            matches!(
                solver_error,
                ExponentialAffineTransformSolverError::InvalidSolverSettings { .. }
            ),
            "unexpected solver error variant: {solver_error}"
        );
    }

    #[test]
    fn solver_integrator_selection_wiring_uses_requested_method() {
        let transform_input = linear_closed_form_fixture_input();
        let euler_settings = ExponentialAffineTransformSolverSettings::try_new(
            ExponentialAffineTransformIntegratorMethod::ForwardEuler,
            1,
        )
        .expect("valid Euler settings should construct");
        let runge_kutta_settings = ExponentialAffineTransformSolverSettings::try_new(
            ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta,
            1,
        )
        .expect("valid Runge-Kutta settings should construct");

        let euler_result =
            solve_exponential_affine_transform_with_settings(transform_input, euler_settings)
                .expect("Euler integration should succeed");
        let runge_kutta_result =
            solve_exponential_affine_transform_with_settings(transform_input, runge_kutta_settings)
                .expect("Runge-Kutta integration should succeed");

        assert_eq!(
            euler_result.metadata.integrator_method,
            ExponentialAffineTransformIntegratorMethod::ForwardEuler
        );
        assert_eq!(
            euler_result.metadata.rhs_evaluation_count,
            euler_result.metadata.configured_step_count
        );
        assert_eq!(euler_result.metadata.guard_failure, None);
        assert_eq!(
            runge_kutta_result.metadata.integrator_method,
            ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta
        );
        assert_eq!(
            runge_kutta_result.metadata.rhs_evaluation_count,
            runge_kutta_result
                .metadata
                .configured_step_count
                .saturating_mul(4)
        );
        assert_eq!(runge_kutta_result.metadata.guard_failure, None);

        let method_difference = euler_result
            .output
            .expect("Euler integration should produce output when status is completed")
            .transform_value
            .subtract(
                runge_kutta_result
                    .output
                    .expect(
                        "Runge-Kutta integration should produce output when status is completed",
                    )
                    .transform_value,
            )
            .norm();
        assert!(
            method_difference > 1.0e-6,
            "integrator selection wiring regression detected because methods produced near-identical output under one-step fixture"
        );
    }

    #[test]
    fn default_solver_executes_deterministic_closed_form_fixture() {
        let transform_input = linear_closed_form_fixture_input();
        let solve_result = solve_exponential_affine_transform(transform_input)
            .expect("default solver should succeed");
        let output = solve_result
            .output
            .expect("default solver should emit output when status is completed");

        assert_eq!(
            solve_result.status,
            ExponentialAffineTransformIntegrationStatus::Completed
        );
        assert_eq!(
            solve_result.metadata.integrator_method,
            ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta
        );
        assert_eq!(
            solve_result.metadata.completed_step_count,
            solve_result.metadata.configured_step_count
        );
        assert!(
            solve_result.metadata.step_size > 0.0 && solve_result.metadata.step_size.is_finite(),
            "default solver step size must be finite and positive"
        );
        assert_eq!(solve_result.metadata.guard_failure, None);
        assert_eq!(
            solve_result.metadata.domain_guard_policy,
            EXPONENTIAL_AFFINE_TRANSFORM_DOMAIN_GUARD_POLICY
        );
        assert_eq!(
            solve_result.metadata.blowup_termination_policy,
            EXPONENTIAL_AFFINE_TRANSFORM_BLOWUP_TERMINATION_POLICY
        );
        assert!(solve_result.telemetry.is_none());

        let decay = (-transform_input
            .equation_specification
            .hawkes_kernel_parameters
            .beta
            * transform_input.delta)
            .exp();
        let h_plus = -transform_input.u_plus.real;
        let h_minus = -transform_input.u_minus.real;

        let expected_d_plus = transform_input.s_h * h_plus * (1.0 - decay)
            / transform_input
                .equation_specification
                .hawkes_kernel_parameters
                .beta;
        let expected_d_minus = transform_input.s_h * h_minus * (1.0 - decay)
            / transform_input
                .equation_specification
                .hawkes_kernel_parameters
                .beta;
        let expected_c = transform_input.s_h
            * transform_input
                .equation_specification
                .hawkes_kernel_parameters
                .lambda_bar
            * (h_plus + h_minus)
            * transform_input.delta;
        let expected_exponent = expected_c
            + expected_d_plus
                * (transform_input.initial_state.y_plus_plus
                    + transform_input.initial_state.y_plus_minus)
            + expected_d_minus
                * (transform_input.initial_state.y_minus_plus
                    + transform_input.initial_state.y_minus_minus);

        assert_complex_close(
            output.c_at_delta,
            ComplexScalar::new(expected_c, 0.0),
            1.0e-9,
            "C(delta)",
        );
        assert_complex_close(
            output.d_at_delta.plus_plus,
            ComplexScalar::new(expected_d_plus, 0.0),
            1.0e-9,
            "D_++(delta)",
        );
        assert_complex_close(
            output.d_at_delta.plus_minus,
            ComplexScalar::new(expected_d_plus, 0.0),
            1.0e-9,
            "D_+-(delta)",
        );
        assert_complex_close(
            output.d_at_delta.minus_plus,
            ComplexScalar::new(expected_d_minus, 0.0),
            1.0e-9,
            "D_-+(delta)",
        );
        assert_complex_close(
            output.d_at_delta.minus_minus,
            ComplexScalar::new(expected_d_minus, 0.0),
            1.0e-9,
            "D_--(delta)",
        );
        assert_complex_close(
            output.transform_value,
            ComplexScalar::new(expected_exponent.exp(), 0.0),
            1.0e-9,
            "F",
        );
    }

    #[test]
    fn solver_domain_guard_violation_terminates_with_deterministic_metadata() {
        let transform_input = domain_violation_fixture_input();
        let solver_settings = ExponentialAffineTransformSolverSettings::try_new(
            ExponentialAffineTransformIntegratorMethod::ForwardEuler,
            2,
        )
        .expect("domain-violation settings should construct");

        let solve_result =
            solve_exponential_affine_transform_with_settings(transform_input, solver_settings)
                .expect("domain-violation run should return guarded termination result");
        assert_eq!(
            solve_result.status,
            ExponentialAffineTransformIntegrationStatus::DomainGuardViolation
        );
        assert!(solve_result.output.is_none());

        let guard_failure = solve_result
            .metadata
            .guard_failure
            .expect("guarded run should include guard failure metadata");
        assert_eq!(
            guard_failure.code,
            ExponentialAffineTransformGuardFailureCode::DomainUpperBoundViolation
        );
        assert_eq!(
            guard_failure.schema_version,
            EXPONENTIAL_AFFINE_TRANSFORM_GUARD_FAILURE_SCHEMA_VERSION
        );
        assert_eq!(guard_failure.step_index, 1);
        assert!(
            (guard_failure.time - 0.25).abs() <= 1.0e-15,
            "unexpected guard failure time: {}",
            guard_failure.time
        );
        assert!(
            guard_failure.d_dot_r.real > guard_failure.d_dot_r_upper_bound,
            "domain guard trigger should require d_dot_r.real > upper bound"
        );
        assert_eq!(
            solve_result.metadata.completed_step_count,
            guard_failure.step_index
        );
        assert_eq!(solve_result.metadata.rhs_evaluation_count, 1);
    }

    #[test]
    fn solver_blowup_detection_terminates_without_output() {
        let transform_input = blowup_fixture_input();
        let solver_settings = ExponentialAffineTransformSolverSettings::try_new(
            ExponentialAffineTransformIntegratorMethod::ForwardEuler,
            1,
        )
        .expect("blowup settings should construct");

        let solve_result =
            solve_exponential_affine_transform_with_settings(transform_input, solver_settings)
                .expect("blowup run should return guarded termination result");
        assert_eq!(
            solve_result.status,
            ExponentialAffineTransformIntegrationStatus::BlowupTerminated
        );
        assert!(solve_result.output.is_none());

        let guard_failure = solve_result
            .metadata
            .guard_failure
            .expect("blowup run should include guard failure metadata");
        assert_eq!(
            guard_failure.code,
            ExponentialAffineTransformGuardFailureCode::NonFiniteState
        );
        assert_eq!(
            guard_failure.schema_version,
            EXPONENTIAL_AFFINE_TRANSFORM_GUARD_FAILURE_SCHEMA_VERSION
        );
        assert_eq!(guard_failure.step_index, 1);
        assert!(
            guard_failure.d_dot_r.real.is_nan() && guard_failure.d_dot_r.imaginary.is_nan(),
            "non-finite-state guard marker should use NaN d_dot_r sentinel"
        );
        assert_eq!(solve_result.metadata.rhs_evaluation_count, 1);
    }

    #[test]
    fn telemetry_toggle_preserves_solver_semantics() {
        let transform_input = linear_closed_form_fixture_input();

        let telemetry_disabled_settings = ExponentialAffineTransformSolverSettings::try_new(
            ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta,
            8,
        )
        .expect("telemetry-disabled settings should construct");

        let telemetry_enabled_settings = telemetry_disabled_settings
            .with_telemetry_mode(ExponentialAffineTransformTelemetryMode::Enabled);

        let telemetry_disabled_result = solve_exponential_affine_transform_with_settings(
            transform_input,
            telemetry_disabled_settings,
        )
        .expect("telemetry-disabled solve should succeed");

        let telemetry_enabled_result = solve_exponential_affine_transform_with_settings(
            transform_input,
            telemetry_enabled_settings,
        )
        .expect("telemetry-enabled solve should succeed");

        assert_eq!(
            telemetry_disabled_result.status,
            telemetry_enabled_result.status
        );
        assert_eq!(
            telemetry_disabled_result.output,
            telemetry_enabled_result.output
        );
        assert_eq!(
            telemetry_disabled_result.metadata.status_flags,
            telemetry_enabled_result.metadata.status_flags
        );
        assert_eq!(
            telemetry_disabled_result.metadata.telemetry_mode,
            ExponentialAffineTransformTelemetryMode::Disabled
        );
        assert_eq!(
            telemetry_enabled_result.metadata.telemetry_mode,
            ExponentialAffineTransformTelemetryMode::Enabled
        );
        assert!(telemetry_disabled_result.telemetry.is_none());

        let telemetry = telemetry_enabled_result
            .telemetry
            .expect("telemetry-enabled solve should emit telemetry payload");

        assert!(!telemetry.guard_triggered);
        assert_eq!(telemetry.guard_trigger, None);
        assert_eq!(telemetry.checkpoint_count, 9);
    }

    #[test]
    fn diagnostics_payload_schema_contains_required_fields() {
        let solve_result = solve_exponential_affine_transform(linear_closed_form_fixture_input())
            .expect("solve should succeed");

        let serialized_payload =
            serde_json::to_value(solve_result).expect("solver payload should serialize");
        let serialized_payload_object = serialized_payload
            .as_object()
            .expect("serialized payload should be an object");

        for required_result_field in EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_RESULT_FIELDS
        {
            assert!(
                serialized_payload_object.contains_key(required_result_field),
                "missing required result field '{required_result_field}'"
            );
        }

        assert_eq!(
            serialized_payload["schema_version"],
            Value::String(
                EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_RESULT_SCHEMA_VERSION.to_string()
            )
        );

        let output_schema_metadata = &serialized_payload["output_schema_metadata"];

        assert_eq!(
            output_schema_metadata["schema_version"],
            Value::String(
                EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_OUTPUT_SCHEMA_METADATA_VERSION.to_string()
            )
        );
        assert_eq!(
            output_schema_metadata["required_result_fields"],
            serde_json::to_value(EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_RESULT_FIELDS)
                .expect("required result fields should serialize")
        );
        assert_eq!(
            output_schema_metadata["required_metadata_fields"],
            serde_json::to_value(EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_METADATA_FIELDS)
                .expect("required metadata fields should serialize")
        );
        assert_eq!(
            output_schema_metadata["required_guard_failure_fields"],
            serde_json::to_value(
                EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_GUARD_FAILURE_FIELDS
            )
            .expect("required guard failure fields should serialize")
        );
        assert_eq!(
            output_schema_metadata["required_telemetry_fields"],
            serde_json::to_value(
                EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_TELEMETRY_FIELDS
            )
            .expect("required telemetry fields should serialize")
        );

        let metadata = &serialized_payload["metadata"];
        let metadata_object = metadata
            .as_object()
            .expect("serialized metadata should be an object");

        for required_metadata_field in
            EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_METADATA_FIELDS
        {
            assert!(
                metadata_object.contains_key(required_metadata_field),
                "missing required metadata field '{required_metadata_field}'"
            );
        }

        assert_eq!(
            metadata["schema_version"],
            Value::String(
                EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_METADATA_SCHEMA_VERSION.to_string()
            )
        );
    }

    #[test]
    fn telemetry_payload_reports_guarded_failure_context() {
        let transform_input = domain_violation_fixture_input();
        let solver_settings = ExponentialAffineTransformSolverSettings::try_new_with_telemetry(
            ExponentialAffineTransformIntegratorMethod::ForwardEuler,
            2,
            ExponentialAffineTransformTelemetryMode::Enabled,
        )
        .expect("guarded-failure telemetry settings should construct");

        let solve_result =
            solve_exponential_affine_transform_with_settings(transform_input, solver_settings)
                .expect("guarded-failure solve should return deterministic result");
        assert_eq!(
            solve_result.status,
            ExponentialAffineTransformIntegrationStatus::DomainGuardViolation
        );

        let guard_failure = solve_result
            .metadata
            .guard_failure
            .expect("guarded-failure solve should include guard metadata");
        let telemetry = solve_result
            .telemetry
            .expect("guarded-failure solve should emit telemetry when enabled");

        assert_eq!(
            telemetry.schema_version,
            EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_TELEMETRY_SCHEMA_VERSION
        );
        assert!(telemetry.guard_triggered);
        assert_eq!(telemetry.guard_trigger, Some(guard_failure));
        assert!(
            telemetry.checkpoint_count >= 2,
            "telemetry must include at least initial and first-step checkpoints"
        );
        assert!(
            telemetry
                .min_domain_margin
                .expect("domain-violation telemetry must include a domain margin")
                <= 0.0
        );

        let serialized_guard_failure =
            serde_json::to_value(guard_failure).expect("guard failure payload should serialize");
        for required_guard_failure_field in
            EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_GUARD_FAILURE_FIELDS
        {
            assert_ne!(
                serialized_guard_failure[required_guard_failure_field],
                Value::Null,
                "guard failure field '{required_guard_failure_field}' must be present"
            );
        }
    }
}
