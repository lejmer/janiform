//! Canonical NIG driver parameter schema and domain validation.
//!
//! This module centralizes NIG parameter constraints so simulation and pricing
//! use one validation path.

use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Deserialize;
use serde::Serialize;

use crate::randomness::SimulationRandomnessStream;

pub const NIG_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION: &str =
    "validation.nig.parameter_report.v1";
pub const NIG_PARAMETER_VALIDATION_DETERMINISM_POLICY: &str =
    "stable_check_order_and_aggregated_failures.v1";
pub const NIG_INCREMENT_SAMPLING_POLICY: &str =
    "normal_inverse_gaussian_via_inverse_gaussian_subordinator_with_box_muller_standard_normal.v1";
pub const NIG_INCREMENT_NEAR_ZERO_SCALE_POLICY: &str =
    "zero_business_time_scale_returns_zero_without_random_draws.v1";
pub const NIG_SHARED_PLUS_SHIFT_CONFIGURATION_POLICY: &str =
    "buy_beta_equals_beta_0_and_sell_beta_equals_beta_0_plus_delta_beta.v1";
pub const NIG_TRADING_TIME_RETURN_SIGN_CONVENTION_POLICY: &str =
    "buy_side_uses_psi_plus_of_u_and_sell_side_uses_psi_minus_of_negative_u.v1";

/// Minimal deterministic complex scalar used by transform-side APIs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComplexScalar {
    pub real: f64,
    pub imaginary: f64,
}

impl ComplexScalar {
    pub fn new(real: f64, imaginary: f64) -> Self {
        Self { real, imaginary }
    }

    pub fn conjugate(self) -> Self {
        Self::new(self.real, -self.imaginary)
    }

    pub fn square(self) -> Self {
        Self::new(
            self.real
                .mul_add(self.real, -(self.imaginary * self.imaginary)),
            2.0 * self.real * self.imaginary,
        )
    }

    pub fn norm(self) -> f64 {
        self.real.hypot(self.imaginary)
    }

    pub fn add(self, other: Self) -> Self {
        Self::new(self.real + other.real, self.imaginary + other.imaginary)
    }

    pub fn subtract(self, other: Self) -> Self {
        Self::new(self.real - other.real, self.imaginary - other.imaginary)
    }

    pub fn scale(self, value: f64) -> Self {
        Self::new(self.real * value, self.imaginary * value)
    }
}

impl Display for ComplexScalar {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "({}+{}i)", self.real, self.imaginary)
    }
}

/// Unified pass or fail status for NIG parameter validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NigValidationStatus {
    Pass,
    Fail,
}

/// Machine-readable constraint labels for NIG parameter validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum NigParameterConstraint {
    Finite,
    FiniteAndStrictlyPositive,
    StrictlyGreaterThanAbsoluteComparisonField,
}

impl Display for NigParameterConstraint {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Finite => write!(formatter, "finite"),
            Self::FiniteAndStrictlyPositive => {
                write!(formatter, "finite_and_strictly_positive")
            }
            Self::StrictlyGreaterThanAbsoluteComparisonField => {
                write!(formatter, "strictly_greater_than_absolute_comparison_field")
            }
        }
    }
}

/// Structured validation violation payload for one failed NIG domain check.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct NigParameterViolation {
    pub field: &'static str,
    pub constraint: NigParameterConstraint,
    pub value: f64,
    pub comparison_field: Option<&'static str>,
    pub comparison_value: Option<f64>,
}

/// Full validation report for standalone NIG domain checks.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NigParameterValidationReport {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub status: NigValidationStatus,
    pub violations: Vec<NigParameterViolation>,
}

impl NigParameterValidationReport {
    pub fn is_valid(&self) -> bool {
        self.status == NigValidationStatus::Pass
    }

    pub fn first_violation(&self) -> Option<NigParameterViolation> {
        self.violations.first().copied()
    }
}

/// Canonical NIG parameter payload used by pricing and simulation drivers.
///
/// Domain constraints:
/// - `alpha > abs(beta)`
/// - `delta > 0`
/// - all fields are finite
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct NigParameters {
    pub alpha: f64,
    pub beta: f64,
    pub delta: f64,
    pub mu: f64,
}

impl NigParameters {
    /// Evaluate the NIG characteristic exponent for a complex argument.
    ///
    /// Parameter mapping follows the model notation \f$(\alpha,\beta,\delta,\mu)\f$:
    /// \f[
    /// \psi_{\mathrm{NIG}}(u)
    /// = i\mu u
    /// + \delta\left(
    /// \sqrt{\alpha^2-\beta^2}
    /// -
    /// \sqrt{\alpha^2-(\beta+i u)^2}
    /// \right).
    /// \f]
    ///
    /// The complex square root uses the principal branch with a positive
    /// imaginary value on the negative real axis to keep branch handling explicit.
    pub fn characteristic_exponent(self, argument: ComplexScalar) -> ComplexScalar {
        let beta_plus_imaginary_argument =
            ComplexScalar::new(self.beta - argument.imaginary, argument.real);

        let alpha_squared = self.alpha * self.alpha;
        let radicand =
            ComplexScalar::new(alpha_squared, 0.0).subtract(beta_plus_imaginary_argument.square());
        let square_root_term = principal_branch_square_root(radicand);

        let gamma = stable_gamma(self.alpha, self.beta);
        let location_term =
            ComplexScalar::new(-self.mu * argument.imaginary, self.mu * argument.real);
        let shape_term = ComplexScalar::new(gamma, 0.0)
            .subtract(square_root_term)
            .scale(self.delta);

        location_term.add(shape_term)
    }

    /// Sample one business-time increment from the NIG Lévy driver.
    ///
    /// Input semantics:
    /// - `business_time_increment_scale` is the Lévy clock increment and must be finite and
    ///   non-negative.
    /// - The method consumes randomness from the shared simulation randomness stream so seeded
    ///   reproducibility follows the project-wide seed and substream contracts.
    ///
    /// Edge behavior:
    /// - When `business_time_increment_scale == 0`, this method returns `0.0` and consumes no
    ///   random draws.
    pub fn sample_increment<R>(
        self,
        business_time_increment_scale: f64,
        randomness_stream: &mut R,
    ) -> Result<f64, NigIncrementSamplingError>
    where
        R: SimulationRandomnessStream,
    {
        if !business_time_increment_scale.is_finite() || business_time_increment_scale < 0.0 {
            return Err(
                NigIncrementSamplingError::InvalidBusinessTimeIncrementScale {
                    value: business_time_increment_scale,
                },
            );
        }

        if business_time_increment_scale == 0.0 {
            return Ok(0.0);
        }

        let scaled_delta = self.delta * business_time_increment_scale;
        let scaled_mu = self.mu * business_time_increment_scale;
        let gamma = stable_gamma(self.alpha, self.beta);
        let inverse_gaussian_mean = scaled_delta / gamma;
        let inverse_gaussian_shape = scaled_delta * scaled_delta;

        let business_clock_increment = sample_inverse_gaussian_increment(
            inverse_gaussian_mean,
            inverse_gaussian_shape,
            randomness_stream,
        )
        .ok_or(NigIncrementSamplingError::NonFiniteSubordinatorDraw {
            business_time_increment_scale,
        })?;

        let standard_normal_draw = sample_standard_normal_draw(randomness_stream);
        if !standard_normal_draw.is_finite() {
            return Err(NigIncrementSamplingError::NonFiniteStandardNormalDraw {
                business_time_increment_scale,
            });
        }

        let sampled_increment = scaled_mu
            + self.beta * business_clock_increment
            + business_clock_increment.sqrt() * standard_normal_draw;
        if !sampled_increment.is_finite() {
            return Err(NigIncrementSamplingError::NonFiniteIncrementDraw {
                business_time_increment_scale,
            });
        }

        Ok(sampled_increment)
    }

    /// Sample a vector of NIG increments for one sequence of business-time scales.
    pub fn sample_increment_path<R>(
        self,
        business_time_increment_scale_path: &[f64],
        randomness_stream: &mut R,
    ) -> Result<Vec<f64>, NigIncrementSamplingError>
    where
        R: SimulationRandomnessStream,
    {
        let mut sampled_increments: Vec<f64> =
            Vec::with_capacity(business_time_increment_scale_path.len());

        for business_time_increment_scale in business_time_increment_scale_path {
            let sampled_increment =
                self.sample_increment(*business_time_increment_scale, randomness_stream)?;
            sampled_increments.push(sampled_increment);
        }

        Ok(sampled_increments)
    }

    /// Run pure domain validation without constructing a parameter payload.
    pub fn validate_domain(
        alpha: f64,
        beta: f64,
        delta: f64,
        mu: f64,
    ) -> NigParameterValidationReport {
        let mut violations: Vec<NigParameterViolation> = Vec::new();

        collect_finite_violation("alpha", alpha, &mut violations);
        collect_finite_violation("beta", beta, &mut violations);
        collect_positive_violation("delta", delta, &mut violations);
        collect_finite_violation("mu", mu, &mut violations);

        if alpha.is_finite() && beta.is_finite() {
            let beta_absolute_value = beta.abs();
            if alpha <= beta_absolute_value {
                violations.push(NigParameterViolation {
                    field: "alpha",
                    constraint: NigParameterConstraint::StrictlyGreaterThanAbsoluteComparisonField,
                    value: alpha,
                    comparison_field: Some("beta"),
                    comparison_value: Some(beta_absolute_value),
                });
            }
        }

        let status = if violations.is_empty() {
            NigValidationStatus::Pass
        } else {
            NigValidationStatus::Fail
        };

        NigParameterValidationReport {
            schema_version: NIG_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION,
            determinism_policy: NIG_PARAMETER_VALIDATION_DETERMINISM_POLICY,
            status,
            violations,
        }
    }

    /// Validate and construct one canonical NIG parameter payload.
    pub fn try_new(alpha: f64, beta: f64, delta: f64, mu: f64) -> Result<Self, NigParameterError> {
        let validation_report = Self::validate_domain(alpha, beta, delta, mu);
        if let Some(violation) = validation_report.first_violation() {
            return Err(NigParameterError::DomainViolation {
                field: violation.field,
                constraint: violation.constraint,
                value: violation.value,
                comparison_field: violation.comparison_field,
                comparison_value: violation.comparison_value,
            });
        }

        Ok(Self {
            alpha,
            beta,
            delta,
            mu,
        })
    }

    /// Re-run domain validation for an already-built payload.
    pub fn validate(self) -> NigParameterValidationReport {
        Self::validate_domain(self.alpha, self.beta, self.delta, self.mu)
    }
}

/// Shared-plus-shift NIG driver configuration for buy and sell trading-time components.
///
/// Parameter mapping:
/// - buy driver uses `(alpha, beta_0, delta, mu)`
/// - sell driver uses `(alpha, beta_0 + delta_beta, delta, mu)`
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct NigSharedPlusShiftConfiguration {
    pub alpha: f64,
    pub beta_0: f64,
    pub delta: f64,
    pub mu: f64,
    pub delta_beta: f64,
}

impl NigSharedPlusShiftConfiguration {
    /// Validate and build one canonical shared-plus-shift configuration payload.
    pub fn try_new(
        alpha: f64,
        beta_0: f64,
        delta: f64,
        mu: f64,
        delta_beta: f64,
    ) -> Result<Self, NigSharedPlusShiftConfigurationError> {
        let _buy_side_parameters =
            NigParameters::try_new(alpha, beta_0, delta, mu).map_err(|source| {
                NigSharedPlusShiftConfigurationError::BuySideParameterDomainViolation { source }
            })?;
        let derived_sell_beta = beta_0 + delta_beta;
        let _sell_side_parameters = NigParameters::try_new(alpha, derived_sell_beta, delta, mu)
            .map_err(|source| {
                NigSharedPlusShiftConfigurationError::SellSideParameterDomainViolation {
                    beta_0,
                    delta_beta,
                    derived_sell_beta,
                    source,
                }
            })?;

        Ok(Self {
            alpha,
            beta_0,
            delta,
            mu,
            delta_beta,
        })
    }

    /// Return the derived sell-side skew parameter `beta_0 + delta_beta`.
    pub fn derived_sell_beta(self) -> f64 {
        self.beta_0 + self.delta_beta
    }

    /// Build both buy and sell driver parameter payloads from one shared source.
    pub fn derive_driver_parameters(
        self,
    ) -> Result<NigBuySellDriverParameters, NigSharedPlusShiftConfigurationError> {
        let buy_side_parameters =
            NigParameters::try_new(self.alpha, self.beta_0, self.delta, self.mu).map_err(
                |source| NigSharedPlusShiftConfigurationError::BuySideParameterDomainViolation {
                    source,
                },
            )?;
        let derived_sell_beta = self.derived_sell_beta();
        let sell_side_parameters =
            NigParameters::try_new(self.alpha, derived_sell_beta, self.delta, self.mu).map_err(
                |source| NigSharedPlusShiftConfigurationError::SellSideParameterDomainViolation {
                    beta_0: self.beta_0,
                    delta_beta: self.delta_beta,
                    derived_sell_beta,
                    source,
                },
            )?;

        Ok(NigBuySellDriverParameters {
            buy_side_parameters,
            sell_side_parameters,
            policy: NIG_SHARED_PLUS_SHIFT_CONFIGURATION_POLICY,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct NigBuySellDriverParameters {
    pub buy_side_parameters: NigParameters,
    pub sell_side_parameters: NigParameters,
    pub policy: &'static str,
}

/// Trading-time characteristic exponent contributions for one return argument.
///
/// Equation reference:
/// - `docs/design/janiform_compact_model_spec.tex`, Section 5
/// - `docs/design/janiform_design_brainstorm_notes.md`, Section 2.1
///
/// The canonical return composition sign convention is:
/// \f[
/// A^+\psi_+(u) + A^-\psi_-(-u).
/// \f]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NigTradingTimeCharacteristicExponentContributions {
    pub buy_side_weighted_exponent: ComplexScalar,
    pub sell_side_weighted_exponent: ComplexScalar,
    pub combined_weighted_exponent: ComplexScalar,
    pub policy: &'static str,
}

impl NigBuySellDriverParameters {
    /// Evaluate buy-side NIG exponent with the direct argument \f$u\f$, i.e. \f$\psi_+(u)\f$.
    pub fn buy_side_characteristic_exponent(self, argument: ComplexScalar) -> ComplexScalar {
        self.buy_side_parameters.characteristic_exponent(argument)
    }

    /// Evaluate sell-side NIG exponent with the negated argument, i.e. \f$\psi_-(-u)\f$.
    ///
    /// This sign convention matches the model equation
    /// \f[
    /// X_t = L_{A_t^+}^+ - L_{A_t^-}^- + G_t,
    /// \f]
    /// which yields the characteristic-function term
    /// \f[
    /// \exp\!\left(A^+\psi_+(u) + A^-\psi_-(-u)\right).
    /// \f]
    pub fn sell_side_characteristic_exponent_for_return_composition(
        self,
        argument: ComplexScalar,
    ) -> ComplexScalar {
        self.sell_side_parameters
            .characteristic_exponent(argument.scale(-1.0))
    }

    /// Assemble trading-time weighted characteristic exponents with canonical sign handling.
    pub fn compose_trading_time_weighted_characteristic_exponent(
        self,
        argument: ComplexScalar,
        integrated_buy_intensity: f64,
        integrated_sell_intensity: f64,
    ) -> NigTradingTimeCharacteristicExponentContributions {
        let buy_side_weighted_exponent = self
            .buy_side_characteristic_exponent(argument)
            .scale(integrated_buy_intensity);
        let sell_side_weighted_exponent = self
            .sell_side_characteristic_exponent_for_return_composition(argument)
            .scale(integrated_sell_intensity);
        let combined_weighted_exponent =
            buy_side_weighted_exponent.add(sell_side_weighted_exponent);

        NigTradingTimeCharacteristicExponentContributions {
            buy_side_weighted_exponent,
            sell_side_weighted_exponent,
            combined_weighted_exponent,
            policy: NIG_TRADING_TIME_RETURN_SIGN_CONVENTION_POLICY,
        }
    }
}

/// Structured validation failure for `NigParameters::try_new`.
#[derive(Debug, Clone, PartialEq)]
pub enum NigParameterError {
    DomainViolation {
        field: &'static str,
        constraint: NigParameterConstraint,
        value: f64,
        comparison_field: Option<&'static str>,
        comparison_value: Option<f64>,
    },
}

impl Display for NigParameterError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DomainViolation {
                field,
                constraint,
                value,
                comparison_field,
                comparison_value,
            } => match constraint {
                NigParameterConstraint::Finite => {
                    write!(formatter, "{field} must be finite, found {value}")
                }
                NigParameterConstraint::FiniteAndStrictlyPositive => write!(
                    formatter,
                    "{field} must be finite and strictly positive, found {value}"
                ),
                NigParameterConstraint::StrictlyGreaterThanAbsoluteComparisonField => {
                    let comparison_field_name = comparison_field.unwrap_or("comparison_field");
                    let comparison_absolute_value = comparison_value.unwrap_or(f64::NAN);
                    write!(
                        formatter,
                        "{field} must be strictly greater than abs({comparison_field_name}), found {field}={value} and abs({comparison_field_name})={comparison_absolute_value}"
                    )
                }
            },
        }
    }
}

impl Error for NigParameterError {}

/// Structured validation failure for `NigSharedPlusShiftConfiguration::try_new`.
#[derive(Debug, Clone, PartialEq)]
pub enum NigSharedPlusShiftConfigurationError {
    BuySideParameterDomainViolation {
        source: NigParameterError,
    },
    SellSideParameterDomainViolation {
        beta_0: f64,
        delta_beta: f64,
        derived_sell_beta: f64,
        source: NigParameterError,
    },
}

impl Display for NigSharedPlusShiftConfigurationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BuySideParameterDomainViolation { source } => {
                write!(formatter, "buy-side NIG parameters are invalid: {source}")
            }
            Self::SellSideParameterDomainViolation {
                beta_0,
                delta_beta,
                derived_sell_beta,
                source,
            } => write!(
                formatter,
                "sell-side NIG parameters are invalid for beta_0={beta_0}, delta_beta={delta_beta}, derived_sell_beta={derived_sell_beta}: {source}"
            ),
        }
    }
}

impl Error for NigSharedPlusShiftConfigurationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::BuySideParameterDomainViolation { source } => Some(source),
            Self::SellSideParameterDomainViolation { source, .. } => Some(source),
        }
    }
}

/// Structured failure modes for NIG increment sampling.
#[derive(Debug, Clone, PartialEq)]
pub enum NigIncrementSamplingError {
    InvalidBusinessTimeIncrementScale { value: f64 },
    NonFiniteSubordinatorDraw { business_time_increment_scale: f64 },
    NonFiniteStandardNormalDraw { business_time_increment_scale: f64 },
    NonFiniteIncrementDraw { business_time_increment_scale: f64 },
}

impl Display for NigIncrementSamplingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidBusinessTimeIncrementScale { value } => write!(
                formatter,
                "business_time_increment_scale must be finite and non-negative, found {value}"
            ),
            Self::NonFiniteSubordinatorDraw {
                business_time_increment_scale,
            } => write!(
                formatter,
                "inverse-gaussian subordinator sampling produced a non-finite value for business_time_increment_scale={business_time_increment_scale}"
            ),
            Self::NonFiniteStandardNormalDraw {
                business_time_increment_scale,
            } => write!(
                formatter,
                "standard-normal draw produced a non-finite value for business_time_increment_scale={business_time_increment_scale}"
            ),
            Self::NonFiniteIncrementDraw {
                business_time_increment_scale,
            } => write!(
                formatter,
                "NIG increment sampling produced a non-finite value for business_time_increment_scale={business_time_increment_scale}"
            ),
        }
    }
}

impl Error for NigIncrementSamplingError {}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct NigParametersPayload {
    alpha: f64,
    beta: f64,
    delta: f64,
    mu: f64,
}

impl<'de> Deserialize<'de> for NigParameters {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = NigParametersPayload::deserialize(deserializer)?;
        Self::try_new(payload.alpha, payload.beta, payload.delta, payload.mu)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct NigSharedPlusShiftConfigurationPayload {
    alpha: f64,
    beta_0: f64,
    delta: f64,
    mu: f64,
    delta_beta: f64,
}

impl<'de> Deserialize<'de> for NigSharedPlusShiftConfiguration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = NigSharedPlusShiftConfigurationPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.alpha,
            payload.beta_0,
            payload.delta,
            payload.mu,
            payload.delta_beta,
        )
        .map_err(serde::de::Error::custom)
    }
}

fn stable_gamma(alpha: f64, beta: f64) -> f64 {
    ((alpha - beta.abs()) * (alpha + beta.abs())).sqrt()
}

fn principal_branch_square_root(value: ComplexScalar) -> ComplexScalar {
    if value.imaginary == 0.0 {
        if value.real >= 0.0 {
            return ComplexScalar::new(value.real.sqrt(), 0.0);
        }

        return ComplexScalar::new(0.0, (-value.real).sqrt());
    }

    let magnitude = value.real.hypot(value.imaginary);
    let half_sum = (0.5 * (magnitude + value.real)).max(0.0);
    let half_difference = (0.5 * (magnitude - value.real)).max(0.0);

    let real_component = half_sum.sqrt();
    let imaginary_magnitude = half_difference.sqrt();
    let imaginary_component = if value.imaginary.is_sign_negative() {
        -imaginary_magnitude
    } else {
        imaginary_magnitude
    };

    ComplexScalar::new(real_component, imaginary_component)
}

fn sample_inverse_gaussian_increment<R>(
    mean: f64,
    shape: f64,
    randomness_stream: &mut R,
) -> Option<f64>
where
    R: SimulationRandomnessStream,
{
    if !mean.is_finite() || !shape.is_finite() || mean <= 0.0 || shape <= 0.0 {
        return Some(0.0);
    }

    let standard_normal_draw = sample_standard_normal_draw(randomness_stream);
    if !standard_normal_draw.is_finite() {
        return None;
    }

    let squared_standard_normal_draw = standard_normal_draw * standard_normal_draw;
    if squared_standard_normal_draw <= f64::MIN_POSITIVE {
        return Some(mean);
    }

    let four_shape_over_mean_draw = 4.0 * shape / (mean * squared_standard_normal_draw);
    if !four_shape_over_mean_draw.is_finite() {
        return Some(mean);
    }

    let square_root_term = (1.0 + four_shape_over_mean_draw).sqrt();
    if !square_root_term.is_finite() || square_root_term <= 0.0 {
        return None;
    }

    let candidate_draw = mean * (square_root_term - 1.0) / (square_root_term + 1.0);
    if !candidate_draw.is_finite() || candidate_draw <= 0.0 {
        return Some(mean);
    }

    let acceptance_threshold = mean / (mean + candidate_draw);
    if !acceptance_threshold.is_finite() || acceptance_threshold <= 0.0 {
        return None;
    }

    let acceptance_draw = randomness_stream.next_unit_interval_f64().clamp(0.0, 1.0);
    if acceptance_draw <= acceptance_threshold {
        return Some(candidate_draw);
    }

    let reflected_draw = mean * mean / candidate_draw;
    if reflected_draw.is_finite() && reflected_draw > 0.0 {
        Some(reflected_draw)
    } else {
        None
    }
}

fn sample_standard_normal_draw<R>(randomness_stream: &mut R) -> f64
where
    R: SimulationRandomnessStream,
{
    let first_uniform_draw = randomness_stream
        .next_unit_interval_f64()
        .clamp(f64::MIN_POSITIVE, 1.0 - f64::EPSILON);
    let second_uniform_draw = randomness_stream
        .next_unit_interval_f64()
        .clamp(0.0, 1.0 - f64::EPSILON);

    let radius = (-2.0 * first_uniform_draw.ln()).sqrt();
    let angle = 2.0 * std::f64::consts::PI * second_uniform_draw;

    radius * angle.cos()
}

fn collect_finite_violation(
    field: &'static str,
    value: f64,
    violations: &mut Vec<NigParameterViolation>,
) {
    if !value.is_finite() {
        violations.push(NigParameterViolation {
            field,
            constraint: NigParameterConstraint::Finite,
            value,
            comparison_field: None,
            comparison_value: None,
        });
    }
}

fn collect_positive_violation(
    field: &'static str,
    value: f64,
    violations: &mut Vec<NigParameterViolation>,
) {
    if !value.is_finite() || value <= 0.0 {
        violations.push(NigParameterViolation {
            field,
            constraint: NigParameterConstraint::FiniteAndStrictlyPositive,
            value,
            comparison_field: None,
            comparison_value: Some(0.0),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ComplexScalar, NIG_INCREMENT_NEAR_ZERO_SCALE_POLICY, NIG_INCREMENT_SAMPLING_POLICY,
        NIG_SHARED_PLUS_SHIFT_CONFIGURATION_POLICY, NIG_TRADING_TIME_RETURN_SIGN_CONVENTION_POLICY,
        NigIncrementSamplingError, NigParameterConstraint, NigParameterError, NigParameters,
        NigSharedPlusShiftConfiguration, NigSharedPlusShiftConfigurationError, NigValidationStatus,
        stable_gamma,
    };
    use crate::randomness::{SimulationRandomnessStream, SplitMix64SimulationRandomnessStream};

    #[test]
    fn valid_interior_parameter_payload_constructs_and_validates() {
        let parameters = NigParameters::try_new(2.5, -0.9, 0.35, -0.05)
            .expect("interior NIG parameters must construct successfully");

        assert_eq!(parameters.alpha, 2.5);
        assert_eq!(parameters.beta, -0.9);
        assert_eq!(parameters.delta, 0.35);
        assert_eq!(parameters.mu, -0.05);

        let report = parameters.validate();
        assert_eq!(report.status, NigValidationStatus::Pass);
        assert!(report.violations.is_empty());
    }

    #[test]
    fn parameter_payload_serialization_round_trip_preserves_values() {
        let parameters = NigParameters::try_new(3.2, 0.6, 0.8, 0.01)
            .expect("valid parameters must be serializable");

        let serialized =
            serde_json::to_string(&parameters).expect("canonical payload should serialize");
        let parsed: NigParameters =
            serde_json::from_str(&serialized).expect("serialized payload should deserialize");

        assert_eq!(parsed, parameters);
    }

    #[test]
    fn shared_plus_shift_configuration_derives_expected_buy_and_sell_parameters() {
        let configuration = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("valid shared-plus-shift payload should construct");

        let derived_driver_parameters = configuration
            .derive_driver_parameters()
            .expect("valid shared-plus-shift payload should derive drivers");

        assert_eq!(
            derived_driver_parameters.buy_side_parameters,
            NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
                .expect("buy-side expected payload should be valid")
        );
        assert_eq!(
            derived_driver_parameters.sell_side_parameters,
            NigParameters::try_new(2.3, -1.0, 0.7, 0.1)
                .expect("sell-side expected payload should be valid")
        );
        assert_eq!(configuration.derived_sell_beta(), -1.0);
        assert_eq!(
            derived_driver_parameters.policy,
            NIG_SHARED_PLUS_SHIFT_CONFIGURATION_POLICY
        );
    }

    #[test]
    fn shared_plus_shift_configuration_rejects_invalid_derived_sell_side_domain() {
        let error = NigSharedPlusShiftConfiguration::try_new(1.0, 0.2, 0.3, 0.0, 0.9)
            .expect_err("derived sell-side beta at alpha boundary should fail");

        match error {
            NigSharedPlusShiftConfigurationError::SellSideParameterDomainViolation {
                beta_0,
                delta_beta,
                derived_sell_beta,
                source,
            } => {
                assert_eq!(beta_0, 0.2);
                assert_eq!(delta_beta, 0.9);
                assert_eq!(derived_sell_beta, 1.1);
                assert_eq!(
                    source,
                    NigParameterError::DomainViolation {
                        field: "alpha",
                        constraint:
                            NigParameterConstraint::StrictlyGreaterThanAbsoluteComparisonField,
                        value: 1.0,
                        comparison_field: Some("beta"),
                        comparison_value: Some(1.1),
                    }
                );
            }
            _ => panic!("unexpected error variant: {error}"),
        }
    }

    #[test]
    fn shared_plus_shift_configuration_rejects_invalid_buy_side_domain() {
        let error = NigSharedPlusShiftConfiguration::try_new(0.6, 0.8, 0.3, 0.0, -0.4)
            .expect_err("invalid buy-side domain should fail before sell-side checks");

        match error {
            NigSharedPlusShiftConfigurationError::BuySideParameterDomainViolation { source } => {
                assert_eq!(
                    source,
                    NigParameterError::DomainViolation {
                        field: "alpha",
                        constraint:
                            NigParameterConstraint::StrictlyGreaterThanAbsoluteComparisonField,
                        value: 0.6,
                        comparison_field: Some("beta"),
                        comparison_value: Some(0.8),
                    }
                );
            }
            _ => panic!("unexpected error variant: {error}"),
        }
    }

    #[test]
    fn shared_plus_shift_configuration_output_is_deterministic() {
        let configuration =
            NigSharedPlusShiftConfiguration::try_new(2.4, -0.65, 0.55, -0.03, -0.12)
                .expect("valid shared-plus-shift payload should construct");

        let first_derived_driver_parameters = configuration
            .derive_driver_parameters()
            .expect("first derivation should succeed");
        let second_derived_driver_parameters = configuration
            .derive_driver_parameters()
            .expect("second derivation should succeed");

        assert_eq!(
            first_derived_driver_parameters,
            second_derived_driver_parameters
        );

        let first_serialized_payload = serde_json::to_string(&first_derived_driver_parameters)
            .expect("first derived payload should serialize");
        let second_serialized_payload = serde_json::to_string(&second_derived_driver_parameters)
            .expect("second derived payload should serialize");
        assert_eq!(first_serialized_payload, second_serialized_payload);
    }

    #[test]
    fn boundary_and_out_of_domain_inputs_report_field_and_constraint() {
        let alpha_beta_boundary_error =
            NigParameters::try_new(1.2, -1.2, 0.5, 0.0).expect_err("alpha == abs(beta) must fail");
        assert_eq!(
            alpha_beta_boundary_error,
            NigParameterError::DomainViolation {
                field: "alpha",
                constraint: NigParameterConstraint::StrictlyGreaterThanAbsoluteComparisonField,
                value: 1.2,
                comparison_field: Some("beta"),
                comparison_value: Some(1.2),
            }
        );

        let delta_boundary_error =
            NigParameters::try_new(2.0, 0.5, 0.0, 0.0).expect_err("delta == 0 must fail");
        assert_eq!(
            delta_boundary_error,
            NigParameterError::DomainViolation {
                field: "delta",
                constraint: NigParameterConstraint::FiniteAndStrictlyPositive,
                value: 0.0,
                comparison_field: None,
                comparison_value: Some(0.0),
            }
        );

        let report = NigParameters::validate_domain(f64::NAN, f64::NEG_INFINITY, -1.0, f64::NAN);
        assert_eq!(report.status, NigValidationStatus::Fail);
        assert_eq!(
            report
                .violations
                .iter()
                .map(|violation| (violation.field, violation.constraint))
                .collect::<Vec<_>>(),
            vec![
                ("alpha", NigParameterConstraint::Finite),
                ("beta", NigParameterConstraint::Finite),
                ("delta", NigParameterConstraint::FiniteAndStrictlyPositive),
                ("mu", NigParameterConstraint::Finite),
            ],
        );
    }

    #[test]
    fn deserialization_rejects_invalid_domain_payload() {
        let payload = serde_json::json!({
            "alpha": 1.0,
            "beta": -1.0,
            "delta": 0.25,
            "mu": 0.0
        });

        let error = serde_json::from_value::<NigParameters>(payload)
            .expect_err("invalid payload must fail deserialization");
        let error_message = error.to_string();
        assert!(error_message.contains("alpha"));
        assert!(error_message.contains("strictly greater than abs(beta)"));
    }

    #[test]
    fn characteristic_exponent_matches_reference_values_for_selected_complex_arguments() {
        let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
            .expect("interior NIG parameters should construct");

        let reference_fixture = [
            (ComplexScalar::new(0.0, 0.0), ComplexScalar::new(0.0, 0.0)),
            (
                ComplexScalar::new(0.75, 0.0),
                ComplexScalar::new(-0.09910996139538936, -0.10776988880696167),
            ),
            (
                ComplexScalar::new(-1.25, 0.5),
                ComplexScalar::new(-0.20167135219211368, 0.3543390690583961),
            ),
            (
                ComplexScalar::new(2.0, -1.75),
                ComplexScalar::new(-0.39180925343099193, 0.6483982349995301),
            ),
            (
                ComplexScalar::new(-3.4, 1.2),
                ComplexScalar::new(-1.3912349251792828, 0.8582572631758696),
            ),
        ];

        for (argument, expected) in reference_fixture {
            let observed = parameters.characteristic_exponent(argument);
            assert_complex_close(observed, expected, 1.0e-13);
        }
    }

    #[test]
    fn characteristic_exponent_respects_conjugate_symmetry_for_real_arguments() {
        let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
            .expect("interior NIG parameters should construct");

        let argument_grid = [0.2, 0.75, 2.5, 8.0];
        for argument in argument_grid {
            let positive = parameters.characteristic_exponent(ComplexScalar::new(argument, 0.0));
            let negative = parameters.characteristic_exponent(ComplexScalar::new(-argument, 0.0));
            assert_complex_close(negative, positive.conjugate(), 1.0e-13);
        }
    }

    #[test]
    fn characteristic_exponent_is_deterministic_for_repeated_calls() {
        let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
            .expect("interior NIG parameters should construct");
        let argument = ComplexScalar::new(-1.7, 0.35);
        let expected = parameters.characteristic_exponent(argument);

        for _ in 0..128 {
            let observed = parameters.characteristic_exponent(argument);
            assert_eq!(observed, expected);
        }
    }

    #[test]
    fn characteristic_exponent_is_finite_for_representative_pricing_grid_arguments() {
        let parameters = NigParameters::try_new(1.000001, 1.0, 0.2, -0.03)
            .expect("near-boundary but valid parameters should construct");

        let real_argument_grid = [-150.0, -50.0, -10.0, 0.0, 10.0, 50.0, 150.0];
        let imaginary_argument_grid = [-2.0, -0.5, 0.0, 0.5, 2.0];

        for real_argument in real_argument_grid {
            for imaginary_argument in imaginary_argument_grid {
                let value = parameters
                    .characteristic_exponent(ComplexScalar::new(real_argument, imaginary_argument));
                assert!(value.real.is_finite());
                assert!(value.imaginary.is_finite());
            }
        }
    }

    #[test]
    fn buy_and_sell_sign_convention_helpers_match_equation_mapping() {
        let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("valid shared-plus-shift payload should construct")
            .derive_driver_parameters()
            .expect("valid shared-plus-shift payload should derive drivers");
        let argument = ComplexScalar::new(0.9, -0.15);

        let observed_buy_side = driver_parameters.buy_side_characteristic_exponent(argument);
        let expected_buy_side = driver_parameters
            .buy_side_parameters
            .characteristic_exponent(argument);
        assert_complex_close(observed_buy_side, expected_buy_side, 1.0e-13);

        let observed_sell_side =
            driver_parameters.sell_side_characteristic_exponent_for_return_composition(argument);
        let expected_sell_side = driver_parameters
            .sell_side_parameters
            .characteristic_exponent(argument.scale(-1.0));
        assert_complex_close(observed_sell_side, expected_sell_side, 1.0e-13);
    }

    #[test]
    fn sell_side_sign_convention_regression_guard_rejects_positive_u_wiring() {
        let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("valid shared-plus-shift payload should construct")
            .derive_driver_parameters()
            .expect("valid shared-plus-shift payload should derive drivers");
        let argument = ComplexScalar::new(1.1, 0.0);

        let correct_sell_side =
            driver_parameters.sell_side_characteristic_exponent_for_return_composition(argument);
        let incorrect_sell_side = driver_parameters
            .sell_side_parameters
            .characteristic_exponent(argument);

        let regression_distance = correct_sell_side.subtract(incorrect_sell_side).norm();
        assert!(
            regression_distance > 1.0e-6,
            "sell-side sign regression guard requires psi_minus(-u) to differ from psi_minus(u), observed distance={regression_distance}"
        );
    }

    #[test]
    fn trading_time_weighted_exponent_composition_matches_manual_assembly() {
        let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("valid shared-plus-shift payload should construct")
            .derive_driver_parameters()
            .expect("valid shared-plus-shift payload should derive drivers");
        let argument = ComplexScalar::new(-0.6, 0.35);
        let integrated_buy_intensity = 1.7;
        let integrated_sell_intensity = 0.9;

        let observed = driver_parameters.compose_trading_time_weighted_characteristic_exponent(
            argument,
            integrated_buy_intensity,
            integrated_sell_intensity,
        );

        let expected_buy_side = driver_parameters
            .buy_side_parameters
            .characteristic_exponent(argument)
            .scale(integrated_buy_intensity);
        let expected_sell_side = driver_parameters
            .sell_side_parameters
            .characteristic_exponent(argument.scale(-1.0))
            .scale(integrated_sell_intensity);
        let expected_combined = expected_buy_side.add(expected_sell_side);

        assert_complex_close(
            observed.buy_side_weighted_exponent,
            expected_buy_side,
            1.0e-13,
        );
        assert_complex_close(
            observed.sell_side_weighted_exponent,
            expected_sell_side,
            1.0e-13,
        );
        assert_complex_close(
            observed.combined_weighted_exponent,
            expected_combined,
            1.0e-13,
        );
        assert_eq!(
            observed.policy,
            NIG_TRADING_TIME_RETURN_SIGN_CONVENTION_POLICY
        );
    }

    #[test]
    fn increment_sampling_policies_are_stable() {
        assert_eq!(
            NIG_INCREMENT_SAMPLING_POLICY,
            "normal_inverse_gaussian_via_inverse_gaussian_subordinator_with_box_muller_standard_normal.v1"
        );
        assert_eq!(
            NIG_INCREMENT_NEAR_ZERO_SCALE_POLICY,
            "zero_business_time_scale_returns_zero_without_random_draws.v1"
        );
    }

    #[test]
    fn increment_sampling_rejects_invalid_business_time_increment_scale() {
        let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
            .expect("interior NIG parameters should construct");
        let mut randomness_stream = DrawCountRandomnessStream::default();

        for invalid_scale in [f64::NAN, f64::INFINITY, -0.5] {
            let error = parameters
                .sample_increment(invalid_scale, &mut randomness_stream)
                .expect_err("invalid business-time scale should fail sampling");

            match error {
                NigIncrementSamplingError::InvalidBusinessTimeIncrementScale { value } => {
                    if invalid_scale.is_nan() {
                        assert!(value.is_nan());
                    } else {
                        assert_eq!(value, invalid_scale);
                    }
                }
                _ => panic!("unexpected error variant: {error}"),
            }
        }
    }

    #[test]
    fn increment_sampling_zero_business_time_returns_zero_without_random_draws() {
        let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
            .expect("interior NIG parameters should construct");
        let mut randomness_stream = DrawCountRandomnessStream::default();

        let sampled_increment = parameters
            .sample_increment(0.0, &mut randomness_stream)
            .expect("zero business-time increment should sample");

        assert_eq!(sampled_increment, 0.0);
        assert_eq!(randomness_stream.draw_count, 0);
    }

    #[test]
    fn increment_sampling_small_and_moderate_business_time_scales_remain_finite() {
        let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
            .expect("interior NIG parameters should construct");
        let mut small_scale_stream = SplitMix64SimulationRandomnessStream::from_seed(11_009);
        let mut small_scale_maximum_absolute_sample = 0.0_f64;

        for _sample_number in 0..2_000 {
            let sampled_increment = parameters
                .sample_increment(1.0e-12, &mut small_scale_stream)
                .expect("small business-time increment should sample");
            assert!(sampled_increment.is_finite());
            small_scale_maximum_absolute_sample =
                small_scale_maximum_absolute_sample.max(sampled_increment.abs());
        }

        assert!(
            small_scale_maximum_absolute_sample < 1.0e-3,
            "very small business-time increments should stay numerically near zero, observed max abs sample={small_scale_maximum_absolute_sample}"
        );

        let mut moderate_scale_stream = SplitMix64SimulationRandomnessStream::from_seed(11_010);
        let moderate_scale_path = [0.25, 0.5, 1.0, 1.5];
        let sampled_path = parameters
            .sample_increment_path(&moderate_scale_path, &mut moderate_scale_stream)
            .expect("moderate business-time increment path should sample");

        assert_eq!(sampled_path.len(), moderate_scale_path.len());
        assert!(sampled_path.iter().all(|sample| sample.is_finite()));

        let path_span = sampled_path.iter().copied().fold(
            (f64::INFINITY, f64::NEG_INFINITY),
            |(minimum, maximum), value| (minimum.min(value), maximum.max(value)),
        );
        assert!(
            path_span.1 - path_span.0 > 1.0e-10,
            "moderate-scale path should contain non-identical sampled increments"
        );
    }

    #[test]
    fn increment_sampling_is_seed_reproducible_for_splitmix_streams() {
        let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
            .expect("interior NIG parameters should construct");
        let business_time_scale_path = [0.4, 0.2, 1.0e-8, 0.0, 0.75, 1.25, 0.05];

        let mut first_stream = SplitMix64SimulationRandomnessStream::from_seed(4_711);
        let mut second_stream = SplitMix64SimulationRandomnessStream::from_seed(4_711);
        let mut third_stream = SplitMix64SimulationRandomnessStream::from_seed(4_712);

        let first_samples = parameters
            .sample_increment_path(&business_time_scale_path, &mut first_stream)
            .expect("first seeded path should sample");
        let second_samples = parameters
            .sample_increment_path(&business_time_scale_path, &mut second_stream)
            .expect("second seeded path should sample");
        let third_samples = parameters
            .sample_increment_path(&business_time_scale_path, &mut third_stream)
            .expect("different-seed path should sample");

        assert_eq!(first_samples, second_samples);
        assert_ne!(first_samples, third_samples);
    }

    #[test]
    fn increment_sampling_distribution_sanity_matches_expected_moments() {
        let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
            .expect("interior NIG parameters should construct");
        let business_time_increment_scale = 0.75;
        let sample_count = 40_000_usize;
        let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(91_177);

        let gamma = stable_gamma(parameters.alpha, parameters.beta);
        let expected_mean = business_time_increment_scale
            * (parameters.mu + parameters.delta * parameters.beta / gamma);
        let expected_variance = business_time_increment_scale
            * (parameters.delta * parameters.alpha * parameters.alpha / gamma.powi(3));

        let mut running_mean = 0.0;
        let mut running_squared_error = 0.0;

        for sample_number in 1..=sample_count {
            let sampled_increment = parameters
                .sample_increment(business_time_increment_scale, &mut randomness_stream)
                .expect("distribution sanity sampling should succeed");
            let sample_number_as_float = sample_number as f64;

            let delta_from_running_mean = sampled_increment - running_mean;
            running_mean += delta_from_running_mean / sample_number_as_float;

            let updated_delta = sampled_increment - running_mean;

            running_squared_error += delta_from_running_mean * updated_delta;
        }

        let sample_variance = running_squared_error / (sample_count as f64 - 1.0);
        let mean_tolerance = 8.0 * (expected_variance / sample_count as f64).sqrt() + 5.0e-3;
        let variance_tolerance = expected_variance * 0.30 + 5.0e-3;

        assert!(
            (running_mean - expected_mean).abs() <= mean_tolerance,
            "sample mean deviated from expected mean beyond tolerance, expected={expected_mean}, observed={running_mean}, tolerance={mean_tolerance}"
        );
        assert!(
            (sample_variance - expected_variance).abs() <= variance_tolerance,
            "sample variance deviated from expected variance beyond tolerance, expected={expected_variance}, observed={sample_variance}, tolerance={variance_tolerance}"
        );
    }

    fn assert_complex_close(observed: ComplexScalar, expected: ComplexScalar, tolerance: f64) {
        let difference = observed.subtract(expected).norm();
        assert!(
            difference <= tolerance,
            "difference {difference} exceeded tolerance {tolerance} for observed {observed} and expected {expected}"
        );
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    struct DrawCountRandomnessStream {
        draw_count: u64,
    }

    impl SimulationRandomnessStream for DrawCountRandomnessStream {
        fn next_u64(&mut self) -> u64 {
            self.draw_count = self.draw_count.saturating_add(1);
            self.draw_count
        }
    }
}
