use serde::Serialize;

use janiform_foundations::randomness::SimulationRandomnessStream;

use super::constants::{
    NIG_CANCELLATION_WARNING_THRESHOLD, NIG_CHARACTERISTIC_EXPONENT_GUARDRAIL_DETERMINISM_POLICY,
    NIG_CHARACTERISTIC_EXPONENT_GUARDRAIL_REPORT_SCHEMA_VERSION,
    NIG_CHARACTERISTIC_EXPONENT_STABILITY_POLICY,
};
use super::errors::{NigIncrementSamplingError, NigParameterError};
use super::guardrails::{
    NigCharacteristicExponentEvaluationReport, NigCharacteristicExponentGuardrailCode,
    NigCharacteristicExponentGuardrailEvent, NigNumericalGuardrailSeverity,
    NigNumericalGuardrailStatus, alpha_beta_margin_warning_threshold,
    collect_non_finite_guardrail_event, compute_radicand_warning_threshold, push_guardrail_event,
};
use super::math::{
    principal_branch_square_root, sample_inverse_gaussian_increment, sample_standard_normal_draw,
    stable_gamma,
};
use super::validation::{
    NigParameterConstraint, NigParameterValidationReport, NigParameterViolation,
    collect_finite_violation, collect_positive_violation,
};
use janiform_foundations::complex_math::ComplexScalar;

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
    /// Evaluate the first moment of one business-time increment.
    ///
    /// For increment scale `a`, the mean is:
    /// `a * (mu + delta * beta / sqrt(alpha^2 - beta^2))`.
    pub fn expected_increment(
        self,
        business_time_increment_scale: f64,
    ) -> Result<f64, NigIncrementSamplingError> {
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

        let gamma = stable_gamma(self.alpha, self.beta);

        let expected_increment =
            business_time_increment_scale * (self.mu + self.delta * self.beta / gamma);
        if !expected_increment.is_finite() {
            return Err(NigIncrementSamplingError::NonFiniteIncrementDraw {
                business_time_increment_scale,
            });
        }

        Ok(expected_increment)
    }

    /// Evaluate the NIG characteristic exponent for a complex argument.
    ///
    /// Parameter mapping follows the model notation \f$(\alpha,\beta,\delta,\mu)\f$:
    /// \f[
    /// \psi_{\mathrm{NIG}}(u)
    /// = i\mu u
    /// + \delta\left(
    ///   \sqrt{\alpha^2-\beta^2}
    /// -
    /// \sqrt{\alpha^2-(\beta+i u)^2}
    /// \right).
    /// \f]
    ///
    /// The complex square root uses the principal branch with a positive
    /// imaginary value on the negative real axis to keep branch handling explicit.
    pub fn characteristic_exponent(self, argument: ComplexScalar) -> ComplexScalar {
        self.characteristic_exponent_with_guardrails(argument)
            .value
            .unwrap_or(ComplexScalar::new(f64::NAN, f64::NAN))
    }

    /// Evaluate the characteristic exponent and emit deterministic numerical diagnostics.
    pub fn characteristic_exponent_with_guardrails(
        self,
        argument: ComplexScalar,
    ) -> NigCharacteristicExponentEvaluationReport {
        let mut status = NigNumericalGuardrailStatus::Pass;
        let mut events: Vec<NigCharacteristicExponentGuardrailEvent> = Vec::new();

        let alpha_beta_margin = self.alpha - self.beta.abs();
        let alpha_beta_margin_warning_threshold =
            alpha_beta_margin_warning_threshold(self.alpha, self.beta);

        let mut radicand_norm = f64::NAN;
        let mut radicand_warning_threshold = f64::NAN;
        let mut cancellation_ratio = f64::NAN;
        let mut used_rationalized_difference = false;

        collect_non_finite_guardrail_event(
            &mut status,
            "alpha",
            self.alpha,
            &mut events,
            "alpha must be finite for characteristic exponent evaluation",
        );

        collect_non_finite_guardrail_event(
            &mut status,
            "beta",
            self.beta,
            &mut events,
            "beta must be finite for characteristic exponent evaluation",
        );

        collect_non_finite_guardrail_event(
            &mut status,
            "delta",
            self.delta,
            &mut events,
            "delta must be finite for characteristic exponent evaluation",
        );

        collect_non_finite_guardrail_event(
            &mut status,
            "mu",
            self.mu,
            &mut events,
            "mu must be finite for characteristic exponent evaluation",
        );

        collect_non_finite_guardrail_event(
            &mut status,
            "argument_real",
            argument.real,
            &mut events,
            "argument.real must be finite for characteristic exponent evaluation",
        );

        collect_non_finite_guardrail_event(
            &mut status,
            "argument_imaginary",
            argument.imaginary,
            &mut events,
            "argument.imaginary must be finite for characteristic exponent evaluation",
        );

        if status == NigNumericalGuardrailStatus::Fail {
            return NigCharacteristicExponentEvaluationReport {
                schema_version: NIG_CHARACTERISTIC_EXPONENT_GUARDRAIL_REPORT_SCHEMA_VERSION,
                determinism_policy: NIG_CHARACTERISTIC_EXPONENT_GUARDRAIL_DETERMINISM_POLICY,
                stability_policy: NIG_CHARACTERISTIC_EXPONENT_STABILITY_POLICY,
                status,
                argument,
                alpha_beta_margin,
                alpha_beta_margin_warning_threshold,
                radicand_norm,
                radicand_warning_threshold,
                cancellation_ratio,
                cancellation_warning_threshold: NIG_CANCELLATION_WARNING_THRESHOLD,
                used_rationalized_difference,
                events,
                value: None,
            };
        }

        if alpha_beta_margin <= 0.0 {
            push_guardrail_event(
                &mut status,
                &mut events,
                NigCharacteristicExponentGuardrailEvent {
                    code: NigCharacteristicExponentGuardrailCode::AlphaBetaDomainViolation,
                    severity: NigNumericalGuardrailSeverity::Error,
                    metric: "alpha_minus_abs_beta",
                    observed_value: Some(alpha_beta_margin),
                    threshold_value: Some(0.0),
                    detail: "alpha must be strictly greater than abs(beta)",
                },
            );

            return NigCharacteristicExponentEvaluationReport {
                schema_version: NIG_CHARACTERISTIC_EXPONENT_GUARDRAIL_REPORT_SCHEMA_VERSION,
                determinism_policy: NIG_CHARACTERISTIC_EXPONENT_GUARDRAIL_DETERMINISM_POLICY,
                stability_policy: NIG_CHARACTERISTIC_EXPONENT_STABILITY_POLICY,
                status,
                argument,
                alpha_beta_margin,
                alpha_beta_margin_warning_threshold,
                radicand_norm,
                radicand_warning_threshold,
                cancellation_ratio,
                cancellation_warning_threshold: NIG_CANCELLATION_WARNING_THRESHOLD,
                used_rationalized_difference,
                events,
                value: None,
            };
        }

        if alpha_beta_margin <= alpha_beta_margin_warning_threshold {
            push_guardrail_event(
                &mut status,
                &mut events,
                NigCharacteristicExponentGuardrailEvent {
                    code: NigCharacteristicExponentGuardrailCode::AlphaBetaBoundaryProximity,
                    severity: NigNumericalGuardrailSeverity::Warning,
                    metric: "alpha_minus_abs_beta",
                    observed_value: Some(alpha_beta_margin),
                    threshold_value: Some(alpha_beta_margin_warning_threshold),
                    detail: "alpha is near the abs(beta) boundary and may amplify floating-point sensitivity",
                },
            );
        }

        if argument == ComplexScalar::new(0.0, 0.0) {
            return NigCharacteristicExponentEvaluationReport {
                schema_version: NIG_CHARACTERISTIC_EXPONENT_GUARDRAIL_REPORT_SCHEMA_VERSION,
                determinism_policy: NIG_CHARACTERISTIC_EXPONENT_GUARDRAIL_DETERMINISM_POLICY,
                stability_policy: NIG_CHARACTERISTIC_EXPONENT_STABILITY_POLICY,
                status,
                argument,
                alpha_beta_margin,
                alpha_beta_margin_warning_threshold,
                radicand_norm,
                radicand_warning_threshold,
                cancellation_ratio,
                cancellation_warning_threshold: NIG_CANCELLATION_WARNING_THRESHOLD,
                used_rationalized_difference,
                events,
                value: Some(ComplexScalar::new(0.0, 0.0)),
            };
        }

        let beta_plus_imaginary_argument =
            ComplexScalar::new(self.beta - argument.imaginary, argument.real);
        let alpha_squared = self.alpha * self.alpha;
        let radicand =
            ComplexScalar::new(alpha_squared, 0.0).subtract(beta_plus_imaginary_argument.square());

        radicand_norm = radicand.norm();

        radicand_warning_threshold =
            compute_radicand_warning_threshold(self.alpha, beta_plus_imaginary_argument);

        if radicand_norm <= radicand_warning_threshold {
            push_guardrail_event(
                &mut status,
                &mut events,
                NigCharacteristicExponentGuardrailEvent {
                    code: NigCharacteristicExponentGuardrailCode::TransformRadicandProximity,
                    severity: NigNumericalGuardrailSeverity::Warning,
                    metric: "radicand_norm",
                    observed_value: Some(radicand_norm),
                    threshold_value: Some(radicand_warning_threshold),
                    detail: "transform radicand is near zero and branch sensitivity may increase",
                },
            );
        }

        let square_root_term = principal_branch_square_root(radicand);

        let gamma = stable_gamma(self.alpha, self.beta);
        if !gamma.is_finite() {
            push_guardrail_event(
                &mut status,
                &mut events,
                NigCharacteristicExponentGuardrailEvent {
                    code: NigCharacteristicExponentGuardrailCode::NonFiniteEvaluationOutput,
                    severity: NigNumericalGuardrailSeverity::Error,
                    metric: "gamma",
                    observed_value: Some(gamma),
                    threshold_value: None,
                    detail: "gamma term became non-finite during characteristic exponent evaluation",
                },
            );

            return NigCharacteristicExponentEvaluationReport {
                schema_version: NIG_CHARACTERISTIC_EXPONENT_GUARDRAIL_REPORT_SCHEMA_VERSION,
                determinism_policy: NIG_CHARACTERISTIC_EXPONENT_GUARDRAIL_DETERMINISM_POLICY,
                stability_policy: NIG_CHARACTERISTIC_EXPONENT_STABILITY_POLICY,
                status,
                argument,
                alpha_beta_margin,
                alpha_beta_margin_warning_threshold,
                radicand_norm,
                radicand_warning_threshold,
                cancellation_ratio,
                cancellation_warning_threshold: NIG_CANCELLATION_WARNING_THRESHOLD,
                used_rationalized_difference,
                events,
                value: None,
            };
        }

        let direct_difference = ComplexScalar::new(gamma, 0.0).subtract(square_root_term);

        let cancellation_scale = (gamma.abs() + square_root_term.norm()).max(f64::MIN_POSITIVE);
        cancellation_ratio = direct_difference.norm() / cancellation_scale;

        let mut gamma_minus_square_root = direct_difference;

        if cancellation_ratio <= NIG_CANCELLATION_WARNING_THRESHOLD {
            push_guardrail_event(
                &mut status,
                &mut events,
                NigCharacteristicExponentGuardrailEvent {
                    code: NigCharacteristicExponentGuardrailCode::CancellationSensitiveDifference,
                    severity: NigNumericalGuardrailSeverity::Warning,
                    metric: "cancellation_ratio",
                    observed_value: Some(cancellation_ratio),
                    threshold_value: Some(NIG_CANCELLATION_WARNING_THRESHOLD),
                    detail: "gamma minus square-root term is cancellation-sensitive and uses stable rationalization",
                },
            );

            let numerator = ComplexScalar::new(gamma * gamma, 0.0).subtract(radicand);
            let denominator = ComplexScalar::new(gamma, 0.0).plus(square_root_term);

            if let Some(rationalized_difference) = numerator.divide(denominator) {
                gamma_minus_square_root = rationalized_difference;
                used_rationalized_difference = true;
            } else {
                push_guardrail_event(
                    &mut status,
                    &mut events,
                    NigCharacteristicExponentGuardrailEvent {
                        code:
                            NigCharacteristicExponentGuardrailCode::RationalizedDifferenceFallback,
                        severity: NigNumericalGuardrailSeverity::Warning,
                        metric: "rationalized_denominator_norm",
                        observed_value: Some(denominator.norm()),
                        threshold_value: Some(
                            super::constants::NIG_COMPLEX_DIVISION_MIN_DENOMINATOR_NORM,
                        ),
                        detail: "rationalized difference was unstable and evaluation used direct subtraction",
                    },
                );
            }
        }

        let location_term =
            ComplexScalar::new(-self.mu * argument.imaginary, self.mu * argument.real);
        let shape_term = gamma_minus_square_root.scale(self.delta);

        let value = location_term.plus(shape_term);

        if !value.is_finite() {
            push_guardrail_event(
                &mut status,
                &mut events,
                NigCharacteristicExponentGuardrailEvent {
                    code: NigCharacteristicExponentGuardrailCode::NonFiniteEvaluationOutput,
                    severity: NigNumericalGuardrailSeverity::Error,
                    metric: "characteristic_exponent_norm",
                    observed_value: Some(value.norm()),
                    threshold_value: None,
                    detail: "characteristic exponent result became non-finite",
                },
            );
        }

        NigCharacteristicExponentEvaluationReport {
            schema_version: NIG_CHARACTERISTIC_EXPONENT_GUARDRAIL_REPORT_SCHEMA_VERSION,
            determinism_policy: NIG_CHARACTERISTIC_EXPONENT_GUARDRAIL_DETERMINISM_POLICY,
            stability_policy: NIG_CHARACTERISTIC_EXPONENT_STABILITY_POLICY,
            status,
            argument,
            alpha_beta_margin,
            alpha_beta_margin_warning_threshold,
            radicand_norm,
            radicand_warning_threshold,
            cancellation_ratio,
            cancellation_warning_threshold: NIG_CANCELLATION_WARNING_THRESHOLD,
            used_rationalized_difference,
            events,
            value: if value.is_finite() { Some(value) } else { None },
        }
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

        NigParameterValidationReport::from_violations(violations)
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
