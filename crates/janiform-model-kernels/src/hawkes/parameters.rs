use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use super::{
    HawkesKernelLoadings, HawkesSourceComponents, HawkesStructuredAlphaParameterization,
    HawkesStructuredRhoParameterization, HawkesStructuredRhoSelfDominancePolicy,
};
/// Shared-kernel parameter payload for the 4D Hawkes excitation dynamics.
///
/// Mapping to equations:
/// - `lambda_bar` is the shared de-seasonalized baseline intensity for buy and sell
/// - `beta` is shared exponential decay in `dY_t^{ij} = -beta * Y_t^{ij} dt + ...`
/// - `alpha` stores endogenous excitation loadings `alpha_ij`
/// - `rho` stores exogenous loading terms `rho_ij`
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct HawkesKernelParameters {
    pub lambda_bar: f64,
    pub beta: f64,
    pub alpha: HawkesKernelLoadings,
    pub rho: HawkesKernelLoadings,
}

pub const HAWKES_STRICT_STATIONARITY_THRESHOLD: f64 = 1.0;
pub const HAWKES_DEFAULT_STABILITY_NUMERIC_TOLERANCE: f64 = 0.0;
pub const HAWKES_KERNEL_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION: &str =
    "validation.hawkes.kernel_parameter_report.v1";
pub const HAWKES_KERNEL_PARAMETER_VALIDATION_DETERMINISM_POLICY: &str =
    "stable_check_order_and_aggregated_failures.v1";

/// Unified pass or fail status used by report-level and check-level validation payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HawkesKernelValidationStatus {
    Pass,
    Fail,
}

impl HawkesKernelValidationStatus {
    const fn from_bool(is_valid: bool) -> Self {
        if is_valid { Self::Pass } else { Self::Fail }
    }
}

/// High-level validation category used to group check diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HawkesKernelParameterCheckCategory {
    Positivity,
    Structural,
    Stability,
}

/// Per-check validation diagnostic entry for optimizer and CLI consumers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HawkesKernelParameterCheck {
    pub check_id: &'static str,
    pub status: HawkesKernelValidationStatus,
    pub category: HawkesKernelParameterCheckCategory,
    pub field: &'static str,
    pub comparison_field: Option<&'static str>,
    pub constraint: HawkesKernelParameterConstraint,
    pub observed_value: f64,
    pub threshold_value: Option<f64>,
    pub expected_condition: &'static str,
    pub diagnostics: String,
    pub remediation_hint: &'static str,
}

/// Summary metadata for aggregated validation reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HawkesKernelParameterValidationSummary {
    pub total_check_count: usize,
    pub passed_check_count: usize,
    pub failed_check_count: usize,
}

/// Machine-readable stability output for the de-seasonalized `Alpha / beta` kernel.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct HawkesSpectralRadiusStabilityCheck {
    pub spectral_radius_alpha_over_beta: f64,
    pub strict_stationarity_threshold: f64,
    pub numeric_tolerance: f64,
    pub effective_stationarity_threshold: f64,
    pub is_stable: bool,
}

impl HawkesKernelParameters {
    /// Run pure domain validation without constructing a kernel payload.
    ///
    /// This helper is intended for pre-checking candidate vectors before
    /// expensive calibration loops.
    pub fn validate_domain(
        lambda_bar: f64,
        beta: f64,
        alpha: HawkesKernelLoadings,
        rho: HawkesKernelLoadings,
    ) -> HawkesKernelParameterValidationReport {
        Self::validate_domain_with_stability_tolerance(
            lambda_bar,
            beta,
            alpha,
            rho,
            HAWKES_DEFAULT_STABILITY_NUMERIC_TOLERANCE,
        )
    }

    /// Run domain validation with an explicit numerical tolerance around
    /// the strict stationarity threshold for `rho(Alpha / beta) < 1`.
    pub fn validate_domain_with_stability_tolerance(
        lambda_bar: f64,
        beta: f64,
        alpha: HawkesKernelLoadings,
        rho: HawkesKernelLoadings,
        stability_numeric_tolerance: f64,
    ) -> HawkesKernelParameterValidationReport {
        let mut checks: Vec<HawkesKernelParameterCheck> = Vec::new();

        validate_strictly_positive_check(
            &mut checks,
            "positivity.lambda_bar",
            HawkesKernelParameterCheckCategory::Positivity,
            "lambda_bar",
            lambda_bar,
            "lambda_bar must be finite and strictly positive",
            "increase lambda_bar to a finite value greater than 0",
        );

        let beta_is_valid = validate_strictly_positive_check(
            &mut checks,
            "positivity.beta",
            HawkesKernelParameterCheckCategory::Positivity,
            "beta",
            beta,
            "beta must be finite and strictly positive",
            "set beta to a finite value greater than 0",
        );

        let alpha_plus_plus_is_valid = validate_non_negative_check(
            &mut checks,
            "positivity.alpha.plus_plus",
            HawkesKernelParameterCheckCategory::Positivity,
            "alpha.plus_plus",
            alpha.plus_plus,
            "alpha.plus_plus must be finite and non-negative",
            "set alpha.plus_plus to a finite value greater than or equal to 0",
        );
        let alpha_plus_minus_is_valid = validate_non_negative_check(
            &mut checks,
            "positivity.alpha.plus_minus",
            HawkesKernelParameterCheckCategory::Positivity,
            "alpha.plus_minus",
            alpha.plus_minus,
            "alpha.plus_minus must be finite and non-negative",
            "set alpha.plus_minus to a finite value greater than or equal to 0",
        );
        let alpha_minus_plus_is_valid = validate_non_negative_check(
            &mut checks,
            "positivity.alpha.minus_plus",
            HawkesKernelParameterCheckCategory::Positivity,
            "alpha.minus_plus",
            alpha.minus_plus,
            "alpha.minus_plus must be finite and non-negative",
            "set alpha.minus_plus to a finite value greater than or equal to 0",
        );
        let alpha_minus_minus_is_valid = validate_non_negative_check(
            &mut checks,
            "positivity.alpha.minus_minus",
            HawkesKernelParameterCheckCategory::Positivity,
            "alpha.minus_minus",
            alpha.minus_minus,
            "alpha.minus_minus must be finite and non-negative",
            "set alpha.minus_minus to a finite value greater than or equal to 0",
        );
        let alpha_is_valid = alpha_plus_plus_is_valid
            && alpha_plus_minus_is_valid
            && alpha_minus_plus_is_valid
            && alpha_minus_minus_is_valid;

        validate_non_negative_check(
            &mut checks,
            "positivity.rho.plus_plus",
            HawkesKernelParameterCheckCategory::Positivity,
            "rho.plus_plus",
            rho.plus_plus,
            "rho.plus_plus must be finite and non-negative",
            "set rho.plus_plus to a finite value greater than or equal to 0",
        );
        validate_non_negative_check(
            &mut checks,
            "positivity.rho.plus_minus",
            HawkesKernelParameterCheckCategory::Positivity,
            "rho.plus_minus",
            rho.plus_minus,
            "rho.plus_minus must be finite and non-negative",
            "set rho.plus_minus to a finite value greater than or equal to 0",
        );
        validate_non_negative_check(
            &mut checks,
            "positivity.rho.minus_plus",
            HawkesKernelParameterCheckCategory::Positivity,
            "rho.minus_plus",
            rho.minus_plus,
            "rho.minus_plus must be finite and non-negative",
            "set rho.minus_plus to a finite value greater than or equal to 0",
        );
        validate_non_negative_check(
            &mut checks,
            "positivity.rho.minus_minus",
            HawkesKernelParameterCheckCategory::Positivity,
            "rho.minus_minus",
            rho.minus_minus,
            "rho.minus_minus must be finite and non-negative",
            "set rho.minus_minus to a finite value greater than or equal to 0",
        );

        let stability_numeric_tolerance_is_valid = validate_non_negative_check(
            &mut checks,
            "stability.numeric_tolerance",
            HawkesKernelParameterCheckCategory::Stability,
            "stability.numeric_tolerance",
            stability_numeric_tolerance,
            "stability numeric tolerance must be finite and non-negative",
            "set stability.numeric_tolerance to a finite value greater than or equal to 0",
        );

        if beta_is_valid && alpha_is_valid && stability_numeric_tolerance_is_valid {
            let stability_check =
                compute_spectral_radius_stability_check(alpha, beta, stability_numeric_tolerance);

            push_validation_check(
                &mut checks,
                "stability.alpha_over_beta.spectral_radius",
                HawkesKernelValidationStatus::from_bool(stability_check.is_stable),
                HawkesKernelParameterCheckCategory::Stability,
                "alpha_over_beta.spectral_radius",
                Some("stability.effective_stationarity_threshold"),
                HawkesKernelParameterConstraint::SpectralRadiusStrictlyBelowOne,
                stability_check.spectral_radius_alpha_over_beta,
                Some(stability_check.effective_stationarity_threshold),
                "spectral radius must be strictly below the effective stationarity threshold",
                format!(
                    "spectral radius={} and effective_threshold={} (strict_threshold={}, numeric_tolerance={})",
                    stability_check.spectral_radius_alpha_over_beta,
                    stability_check.effective_stationarity_threshold,
                    stability_check.strict_stationarity_threshold,
                    stability_check.numeric_tolerance
                ),
                "reduce alpha magnitudes or increase beta so spectral radius(alpha / beta) is below 1",
            );
        }

        HawkesKernelParameterValidationReport::from_checks(checks)
    }

    /// Run structured base-plus-delta validation with default stability tolerance.
    pub fn validate_structured_domain(
        lambda_bar: f64,
        beta: f64,
        structured_alpha: HawkesStructuredAlphaParameterization,
        structured_rho: HawkesStructuredRhoParameterization,
        self_dominance_policy: Option<HawkesStructuredRhoSelfDominancePolicy>,
    ) -> HawkesKernelParameterValidationReport {
        Self::validate_structured_domain_with_stability_tolerance(
            lambda_bar,
            beta,
            structured_alpha,
            structured_rho,
            self_dominance_policy,
            HAWKES_DEFAULT_STABILITY_NUMERIC_TOLERANCE,
        )
    }

    /// Run structured base-plus-delta validation with explicit stability tolerance.
    pub fn validate_structured_domain_with_stability_tolerance(
        lambda_bar: f64,
        beta: f64,
        structured_alpha: HawkesStructuredAlphaParameterization,
        structured_rho: HawkesStructuredRhoParameterization,
        self_dominance_policy: Option<HawkesStructuredRhoSelfDominancePolicy>,
        stability_numeric_tolerance: f64,
    ) -> HawkesKernelParameterValidationReport {
        let mut checks: Vec<HawkesKernelParameterCheck> = Vec::new();

        validate_strictly_positive_check(
            &mut checks,
            "positivity.lambda_bar",
            HawkesKernelParameterCheckCategory::Positivity,
            "lambda_bar",
            lambda_bar,
            "lambda_bar must be finite and strictly positive",
            "increase lambda_bar to a finite value greater than 0",
        );

        let beta_is_valid = validate_strictly_positive_check(
            &mut checks,
            "positivity.beta",
            HawkesKernelParameterCheckCategory::Positivity,
            "beta",
            beta,
            "beta must be finite and strictly positive",
            "set beta to a finite value greater than 0",
        );

        validate_strictly_positive_check(
            &mut checks,
            "positivity.structured_alpha.a_self",
            HawkesKernelParameterCheckCategory::Positivity,
            "structured_alpha.a_self",
            structured_alpha.a_self,
            "structured_alpha.a_self must be finite and strictly positive",
            "set structured_alpha.a_self to a finite value greater than 0",
        );
        validate_non_negative_check(
            &mut checks,
            "positivity.structured_alpha.a_cross",
            HawkesKernelParameterCheckCategory::Positivity,
            "structured_alpha.a_cross",
            structured_alpha.a_cross,
            "structured_alpha.a_cross must be finite and non-negative",
            "set structured_alpha.a_cross to a finite value greater than or equal to 0",
        );
        validate_non_negative_check(
            &mut checks,
            "positivity.structured_alpha.delta_self",
            HawkesKernelParameterCheckCategory::Positivity,
            "structured_alpha.delta_self",
            structured_alpha.delta_self,
            "structured_alpha.delta_self must be finite and non-negative",
            "set structured_alpha.delta_self to a finite value greater than or equal to 0",
        );
        validate_non_negative_check(
            &mut checks,
            "positivity.structured_alpha.delta_cross",
            HawkesKernelParameterCheckCategory::Positivity,
            "structured_alpha.delta_cross",
            structured_alpha.delta_cross,
            "structured_alpha.delta_cross must be finite and non-negative",
            "set structured_alpha.delta_cross to a finite value greater than or equal to 0",
        );

        let derived_alpha = HawkesKernelLoadings {
            plus_plus: structured_alpha.a_self,
            plus_minus: structured_alpha.a_cross,
            minus_plus: structured_alpha.a_cross + structured_alpha.delta_cross,
            minus_minus: structured_alpha.a_self + structured_alpha.delta_self,
        };
        let derived_alpha_plus_plus_is_valid = validate_non_negative_check(
            &mut checks,
            "positivity.alpha.plus_plus",
            HawkesKernelParameterCheckCategory::Positivity,
            "alpha.plus_plus",
            derived_alpha.plus_plus,
            "derived alpha.plus_plus must be finite and non-negative",
            "adjust structured_alpha inputs to keep alpha.plus_plus finite and non-negative",
        );
        let derived_alpha_plus_minus_is_valid = validate_non_negative_check(
            &mut checks,
            "positivity.alpha.plus_minus",
            HawkesKernelParameterCheckCategory::Positivity,
            "alpha.plus_minus",
            derived_alpha.plus_minus,
            "derived alpha.plus_minus must be finite and non-negative",
            "adjust structured_alpha inputs to keep alpha.plus_minus finite and non-negative",
        );
        let derived_alpha_minus_plus_is_valid = validate_non_negative_check(
            &mut checks,
            "positivity.alpha.minus_plus",
            HawkesKernelParameterCheckCategory::Positivity,
            "alpha.minus_plus",
            derived_alpha.minus_plus,
            "derived alpha.minus_plus must be finite and non-negative",
            "adjust structured_alpha inputs to keep alpha.minus_plus finite and non-negative",
        );
        let derived_alpha_minus_minus_is_valid = validate_non_negative_check(
            &mut checks,
            "positivity.alpha.minus_minus",
            HawkesKernelParameterCheckCategory::Positivity,
            "alpha.minus_minus",
            derived_alpha.minus_minus,
            "derived alpha.minus_minus must be finite and non-negative",
            "adjust structured_alpha inputs to keep alpha.minus_minus finite and non-negative",
        );

        if derived_alpha_minus_minus_is_valid && derived_alpha_plus_plus_is_valid {
            validate_greater_or_equal_to_comparison_field_check(
                &mut checks,
                "structural.alpha.self_asymmetry",
                HawkesKernelParameterCheckCategory::Structural,
                "alpha.minus_minus",
                derived_alpha.minus_minus,
                "alpha.plus_plus",
                derived_alpha.plus_plus,
                "alpha self asymmetry requires alpha.minus_minus >= alpha.plus_plus",
                "increase structured_alpha.delta_self so alpha.minus_minus is at least alpha.plus_plus",
            );
        }

        if derived_alpha_minus_plus_is_valid && derived_alpha_plus_minus_is_valid {
            validate_greater_or_equal_to_comparison_field_check(
                &mut checks,
                "structural.alpha.cross_asymmetry",
                HawkesKernelParameterCheckCategory::Structural,
                "alpha.minus_plus",
                derived_alpha.minus_plus,
                "alpha.plus_minus",
                derived_alpha.plus_minus,
                "alpha cross asymmetry requires alpha.minus_plus >= alpha.plus_minus",
                "increase structured_alpha.delta_cross so alpha.minus_plus is at least alpha.plus_minus",
            );
        }

        validate_strictly_positive_check(
            &mut checks,
            "positivity.structured_rho.r_self",
            HawkesKernelParameterCheckCategory::Positivity,
            "structured_rho.r_self",
            structured_rho.r_self,
            "structured_rho.r_self must be finite and strictly positive",
            "set structured_rho.r_self to a finite value greater than 0",
        );
        validate_non_negative_check(
            &mut checks,
            "positivity.structured_rho.r_cross",
            HawkesKernelParameterCheckCategory::Positivity,
            "structured_rho.r_cross",
            structured_rho.r_cross,
            "structured_rho.r_cross must be finite and non-negative",
            "set structured_rho.r_cross to a finite value greater than or equal to 0",
        );
        validate_non_negative_check(
            &mut checks,
            "positivity.structured_rho.delta_self_r",
            HawkesKernelParameterCheckCategory::Positivity,
            "structured_rho.delta_self_r",
            structured_rho.delta_self_r,
            "structured_rho.delta_self_r must be finite and non-negative",
            "set structured_rho.delta_self_r to a finite value greater than or equal to 0",
        );
        validate_non_negative_check(
            &mut checks,
            "positivity.structured_rho.delta_cross_r",
            HawkesKernelParameterCheckCategory::Positivity,
            "structured_rho.delta_cross_r",
            structured_rho.delta_cross_r,
            "structured_rho.delta_cross_r must be finite and non-negative",
            "set structured_rho.delta_cross_r to a finite value greater than or equal to 0",
        );

        let derived_rho = HawkesKernelLoadings {
            plus_plus: structured_rho.r_self,
            plus_minus: structured_rho.r_cross,
            minus_plus: structured_rho.r_cross + structured_rho.delta_cross_r,
            minus_minus: structured_rho.r_self + structured_rho.delta_self_r,
        };
        let derived_rho_plus_plus_is_valid = validate_non_negative_check(
            &mut checks,
            "positivity.rho.plus_plus",
            HawkesKernelParameterCheckCategory::Positivity,
            "rho.plus_plus",
            derived_rho.plus_plus,
            "derived rho.plus_plus must be finite and non-negative",
            "adjust structured_rho inputs to keep rho.plus_plus finite and non-negative",
        );
        let derived_rho_plus_minus_is_valid = validate_non_negative_check(
            &mut checks,
            "positivity.rho.plus_minus",
            HawkesKernelParameterCheckCategory::Positivity,
            "rho.plus_minus",
            derived_rho.plus_minus,
            "derived rho.plus_minus must be finite and non-negative",
            "adjust structured_rho inputs to keep rho.plus_minus finite and non-negative",
        );
        let derived_rho_minus_plus_is_valid = validate_non_negative_check(
            &mut checks,
            "positivity.rho.minus_plus",
            HawkesKernelParameterCheckCategory::Positivity,
            "rho.minus_plus",
            derived_rho.minus_plus,
            "derived rho.minus_plus must be finite and non-negative",
            "adjust structured_rho inputs to keep rho.minus_plus finite and non-negative",
        );
        let derived_rho_minus_minus_is_valid = validate_non_negative_check(
            &mut checks,
            "positivity.rho.minus_minus",
            HawkesKernelParameterCheckCategory::Positivity,
            "rho.minus_minus",
            derived_rho.minus_minus,
            "derived rho.minus_minus must be finite and non-negative",
            "adjust structured_rho inputs to keep rho.minus_minus finite and non-negative",
        );

        if derived_rho_minus_minus_is_valid && derived_rho_plus_plus_is_valid {
            validate_greater_or_equal_to_comparison_field_check(
                &mut checks,
                "structural.rho.self_asymmetry",
                HawkesKernelParameterCheckCategory::Structural,
                "rho.minus_minus",
                derived_rho.minus_minus,
                "rho.plus_plus",
                derived_rho.plus_plus,
                "rho self asymmetry requires rho.minus_minus >= rho.plus_plus",
                "increase structured_rho.delta_self_r so rho.minus_minus is at least rho.plus_plus",
            );
        }

        if derived_rho_minus_plus_is_valid && derived_rho_plus_minus_is_valid {
            validate_greater_or_equal_to_comparison_field_check(
                &mut checks,
                "structural.rho.cross_asymmetry",
                HawkesKernelParameterCheckCategory::Structural,
                "rho.minus_plus",
                derived_rho.minus_plus,
                "rho.plus_minus",
                derived_rho.plus_minus,
                "rho cross asymmetry requires rho.minus_plus >= rho.plus_minus",
                "increase structured_rho.delta_cross_r so rho.minus_plus is at least rho.plus_minus",
            );
        }

        if let Some(policy) = self_dominance_policy {
            let minimum_ratio_is_valid = validate_greater_or_equal_threshold_check(
                &mut checks,
                "structural.rho.self_dominance.minimum_ratio",
                HawkesKernelParameterCheckCategory::Structural,
                "structured_rho.self_dominance_policy.minimum_self_to_cross_ratio",
                policy.minimum_self_to_cross_ratio,
                1.0,
                "self-dominance policy minimum ratio requires a threshold >= 1.0",
                "set self-dominance minimum ratio to a finite value greater than or equal to 1.0",
            );

            if minimum_ratio_is_valid
                && derived_rho_plus_plus_is_valid
                && derived_rho_plus_minus_is_valid
                && derived_rho_minus_minus_is_valid
                && derived_rho_minus_plus_is_valid
            {
                let target_plus_ratio =
                    derive_self_to_cross_ratio(derived_rho.plus_plus, derived_rho.plus_minus);
                let target_minus_ratio =
                    derive_self_to_cross_ratio(derived_rho.minus_minus, derived_rho.minus_plus);

                validate_greater_or_equal_threshold_check(
                    &mut checks,
                    "structural.rho.self_dominance.target_plus_ratio",
                    HawkesKernelParameterCheckCategory::Structural,
                    "rho.target_plus_self_to_cross_ratio",
                    target_plus_ratio,
                    policy.minimum_self_to_cross_ratio,
                    "target plus self-dominance ratio requires rho.plus_plus / rho.plus_minus >= configured minimum ratio",
                    "increase rho.plus_plus or decrease rho.plus_minus to satisfy the configured minimum self-dominance ratio",
                );
                validate_greater_or_equal_threshold_check(
                    &mut checks,
                    "structural.rho.self_dominance.target_minus_ratio",
                    HawkesKernelParameterCheckCategory::Structural,
                    "rho.target_minus_self_to_cross_ratio",
                    target_minus_ratio,
                    policy.minimum_self_to_cross_ratio,
                    "target minus self-dominance ratio requires rho.minus_minus / rho.minus_plus >= configured minimum ratio",
                    "increase rho.minus_minus or decrease rho.minus_plus to satisfy the configured minimum self-dominance ratio",
                );
            }
        }

        let stability_numeric_tolerance_is_valid = validate_non_negative_check(
            &mut checks,
            "stability.numeric_tolerance",
            HawkesKernelParameterCheckCategory::Stability,
            "stability.numeric_tolerance",
            stability_numeric_tolerance,
            "stability numeric tolerance must be finite and non-negative",
            "set stability.numeric_tolerance to a finite value greater than or equal to 0",
        );

        let derived_alpha_is_valid = derived_alpha_plus_plus_is_valid
            && derived_alpha_plus_minus_is_valid
            && derived_alpha_minus_plus_is_valid
            && derived_alpha_minus_minus_is_valid;

        if beta_is_valid && derived_alpha_is_valid && stability_numeric_tolerance_is_valid {
            let stability_check = compute_spectral_radius_stability_check(
                derived_alpha,
                beta,
                stability_numeric_tolerance,
            );

            push_validation_check(
                &mut checks,
                "stability.alpha_over_beta.spectral_radius",
                HawkesKernelValidationStatus::from_bool(stability_check.is_stable),
                HawkesKernelParameterCheckCategory::Stability,
                "alpha_over_beta.spectral_radius",
                Some("stability.effective_stationarity_threshold"),
                HawkesKernelParameterConstraint::SpectralRadiusStrictlyBelowOne,
                stability_check.spectral_radius_alpha_over_beta,
                Some(stability_check.effective_stationarity_threshold),
                "spectral radius must be strictly below the effective stationarity threshold",
                format!(
                    "spectral radius={} and effective_threshold={} (strict_threshold={}, numeric_tolerance={})",
                    stability_check.spectral_radius_alpha_over_beta,
                    stability_check.effective_stationarity_threshold,
                    stability_check.strict_stationarity_threshold,
                    stability_check.numeric_tolerance
                ),
                "reduce alpha magnitudes or increase beta so spectral radius(alpha / beta) is below 1",
            );
        }

        let derived_rho_is_valid = derived_rho_plus_plus_is_valid
            && derived_rho_plus_minus_is_valid
            && derived_rho_minus_plus_is_valid
            && derived_rho_minus_minus_is_valid;

        HawkesKernelParameterValidationReport::from_checks_with_derived_loadings(
            checks,
            if derived_alpha_is_valid {
                Some(derived_alpha)
            } else {
                None
            },
            if derived_rho_is_valid {
                Some(derived_rho)
            } else {
                None
            },
        )
    }

    /// Evaluate de-seasonalized Hawkes stationarity via
    /// `spectral_radius(Alpha / beta) < 1 - numeric_tolerance`.
    ///
    /// The tolerance policy is intentionally strict and one-sided:
    /// larger tolerance values tighten acceptance near the threshold.
    pub fn check_spectral_radius_stability(
        beta: f64,
        alpha: HawkesKernelLoadings,
        numeric_tolerance: f64,
    ) -> Result<HawkesSpectralRadiusStabilityCheck, HawkesKernelParameterError> {
        if !beta.is_finite() || beta <= 0.0 {
            return Err(HawkesKernelParameterError::InvalidBeta { value: beta });
        }

        validate_non_negative_loadings(
            alpha,
            [
                "alpha.plus_plus",
                "alpha.plus_minus",
                "alpha.minus_plus",
                "alpha.minus_minus",
            ],
        )?;

        if !numeric_tolerance.is_finite() || numeric_tolerance < 0.0 {
            return Err(
                HawkesKernelParameterError::InvalidStabilityNumericTolerance {
                    value: numeric_tolerance,
                },
            );
        }

        Ok(compute_spectral_radius_stability_check(
            alpha,
            beta,
            numeric_tolerance,
        ))
    }

    /// Validate and construct kernel parameters.
    ///
    /// Domain checks:
    /// - `lambda_bar > 0` and finite
    /// - `beta > 0` and finite
    /// - all `alpha_ij >= 0` and finite
    /// - all `rho_ij >= 0` and finite
    /// - `spectral_radius(Alpha / beta) < 1`
    pub fn try_new(
        lambda_bar: f64,
        beta: f64,
        alpha: HawkesKernelLoadings,
        rho: HawkesKernelLoadings,
    ) -> Result<Self, HawkesKernelParameterError> {
        Self::try_new_with_stability_tolerance(
            lambda_bar,
            beta,
            alpha,
            rho,
            HAWKES_DEFAULT_STABILITY_NUMERIC_TOLERANCE,
        )
    }

    /// Validate and construct kernel parameters using an explicit stationarity
    /// tolerance around the strict threshold.
    pub fn try_new_with_stability_tolerance(
        lambda_bar: f64,
        beta: f64,
        alpha: HawkesKernelLoadings,
        rho: HawkesKernelLoadings,
        stability_numeric_tolerance: f64,
    ) -> Result<Self, HawkesKernelParameterError> {
        let validation_report = Self::validate_domain_with_stability_tolerance(
            lambda_bar,
            beta,
            alpha,
            rho,
            stability_numeric_tolerance,
        );
        if let Some(violation) = validation_report.first_violation() {
            return Err(map_parameter_violation_to_error_with_stability_tolerance(
                violation,
                stability_numeric_tolerance,
            ));
        }

        Ok(Self {
            lambda_bar,
            beta,
            alpha,
            rho,
        })
    }

    /// Validate and construct parameters from typed target/source component groups.
    ///
    /// This builder removes 4-value positional ambiguity when constructing `alpha`
    /// and `rho`.
    pub fn try_new_from_target_components(
        lambda_bar: f64,
        beta: f64,
        alpha_target_plus: HawkesSourceComponents,
        alpha_target_minus: HawkesSourceComponents,
        rho_target_plus: HawkesSourceComponents,
        rho_target_minus: HawkesSourceComponents,
    ) -> Result<Self, HawkesKernelParameterError> {
        Self::try_new(
            lambda_bar,
            beta,
            HawkesKernelLoadings::from_target_components(alpha_target_plus, alpha_target_minus),
            HawkesKernelLoadings::from_target_components(rho_target_plus, rho_target_minus),
        )
    }
}

/// Machine-readable constraint labels for kernel validation checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum HawkesKernelParameterConstraint {
    FiniteAndStrictlyPositive,
    FiniteAndNonNegative,
    GreaterThanOrEqualToComparisonField,
    GreaterThanOrEqualToThreshold,
    SpectralRadiusStrictlyBelowOne,
}

impl Display for HawkesKernelParameterConstraint {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FiniteAndStrictlyPositive => {
                write!(formatter, "finite_and_strictly_positive")
            }
            Self::FiniteAndNonNegative => write!(formatter, "finite_and_non_negative"),
            Self::GreaterThanOrEqualToComparisonField => {
                write!(formatter, "greater_than_or_equal_to_comparison_field")
            }
            Self::GreaterThanOrEqualToThreshold => {
                write!(formatter, "greater_than_or_equal_to_threshold")
            }
            Self::SpectralRadiusStrictlyBelowOne => {
                write!(formatter, "spectral_radius_strictly_below_one")
            }
        }
    }
}

/// Structured machine-readable violation payload for failed validation checks.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct HawkesKernelParameterViolation {
    pub field: &'static str,
    pub constraint: HawkesKernelParameterConstraint,
    pub value: f64,
}

/// Full report for standalone kernel validation calls.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HawkesKernelParameterValidationReport {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub status: HawkesKernelValidationStatus,
    pub summary: HawkesKernelParameterValidationSummary,
    pub derived_alpha: Option<HawkesKernelLoadings>,
    pub derived_rho: Option<HawkesKernelLoadings>,
    pub checks: Vec<HawkesKernelParameterCheck>,
    pub violations: Vec<HawkesKernelParameterViolation>,
}

impl HawkesKernelParameterValidationReport {
    pub fn is_valid(&self) -> bool {
        self.status == HawkesKernelValidationStatus::Pass
    }

    pub fn first_violation(&self) -> Option<HawkesKernelParameterViolation> {
        self.violations.first().copied()
    }

    fn from_checks(checks: Vec<HawkesKernelParameterCheck>) -> Self {
        Self::from_checks_with_derived_loadings(checks, None, None)
    }

    fn from_checks_with_derived_loadings(
        checks: Vec<HawkesKernelParameterCheck>,
        derived_alpha: Option<HawkesKernelLoadings>,
        derived_rho: Option<HawkesKernelLoadings>,
    ) -> Self {
        let total_check_count = checks.len();
        let failed_check_count = checks
            .iter()
            .filter(|check| check.status == HawkesKernelValidationStatus::Fail)
            .count();

        let passed_check_count = total_check_count - failed_check_count;

        let status = if failed_check_count == 0 {
            HawkesKernelValidationStatus::Pass
        } else {
            HawkesKernelValidationStatus::Fail
        };

        let violations = checks
            .iter()
            .filter(|check| check.status == HawkesKernelValidationStatus::Fail)
            .map(|check| HawkesKernelParameterViolation {
                field: check.field,
                constraint: check.constraint,
                value: check.observed_value,
            })
            .collect::<Vec<_>>();

        Self {
            schema_version: HAWKES_KERNEL_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION,
            determinism_policy: HAWKES_KERNEL_PARAMETER_VALIDATION_DETERMINISM_POLICY,
            status,
            summary: HawkesKernelParameterValidationSummary {
                total_check_count,
                passed_check_count,
                failed_check_count,
            },
            derived_alpha,
            derived_rho,
            checks,
            violations,
        }
    }
}

/// Structured validation failures for `HawkesKernelParameters::try_new`.
#[derive(Debug, Clone, PartialEq)]
pub enum HawkesKernelParameterError {
    InvalidLambdaBar {
        value: f64,
    },
    InvalidBeta {
        value: f64,
    },
    InvalidLoadingComponent {
        field: &'static str,
        value: f64,
    },
    InvalidStabilityNumericTolerance {
        value: f64,
    },
    UnstableSpectralRadius {
        spectral_radius_alpha_over_beta: f64,
        strict_stationarity_threshold: f64,
        numeric_tolerance: f64,
        effective_stationarity_threshold: f64,
    },
}

impl Display for HawkesKernelParameterError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLambdaBar { value } => {
                write!(
                    formatter,
                    "lambda_bar must be finite and strictly positive, found {value}"
                )
            }
            Self::InvalidBeta { value } => {
                write!(
                    formatter,
                    "beta must be finite and strictly positive, found {value}"
                )
            }
            Self::InvalidLoadingComponent { field, value } => write!(
                formatter,
                "{field} must be finite and non-negative, found {value}"
            ),
            Self::InvalidStabilityNumericTolerance { value } => write!(
                formatter,
                "stability numeric tolerance must be finite and non-negative, found {value}"
            ),
            Self::UnstableSpectralRadius {
                spectral_radius_alpha_over_beta,
                strict_stationarity_threshold,
                numeric_tolerance,
                effective_stationarity_threshold,
            } => write!(
                formatter,
                "spectral radius stability failed with spectral_radius_alpha_over_beta={spectral_radius_alpha_over_beta}, strict_stationarity_threshold={strict_stationarity_threshold}, numeric_tolerance={numeric_tolerance}, effective_stationarity_threshold={effective_stationarity_threshold}"
            ),
        }
    }
}

impl Error for HawkesKernelParameterError {}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct HawkesKernelParametersPayload {
    lambda_bar: f64,
    beta: f64,
    alpha: HawkesKernelLoadings,
    rho: HawkesKernelLoadings,
}

impl<'de> Deserialize<'de> for HawkesKernelParameters {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = HawkesKernelParametersPayload::deserialize(deserializer)?;
        Self::try_new(payload.lambda_bar, payload.beta, payload.alpha, payload.rho)
            .map_err(serde::de::Error::custom)
    }
}

fn push_validation_check(
    checks: &mut Vec<HawkesKernelParameterCheck>,
    check_id: &'static str,
    status: HawkesKernelValidationStatus,
    category: HawkesKernelParameterCheckCategory,
    field: &'static str,
    comparison_field: Option<&'static str>,
    constraint: HawkesKernelParameterConstraint,
    observed_value: f64,
    threshold_value: Option<f64>,
    expected_condition: &'static str,
    diagnostics: String,
    remediation_hint: &'static str,
) {
    checks.push(HawkesKernelParameterCheck {
        check_id,
        status,
        category,
        field,
        comparison_field,
        constraint,
        observed_value,
        threshold_value,
        expected_condition,
        diagnostics,
        remediation_hint,
    });
}

fn validate_strictly_positive_check(
    checks: &mut Vec<HawkesKernelParameterCheck>,
    check_id: &'static str,
    category: HawkesKernelParameterCheckCategory,
    field: &'static str,
    value: f64,
    diagnostics_label: &'static str,
    remediation_hint: &'static str,
) -> bool {
    let is_valid = value.is_finite() && value > 0.0;
    let status = HawkesKernelValidationStatus::from_bool(is_valid);

    let diagnostics = if is_valid {
        format!("{diagnostics_label}: observed value {value} satisfies > 0")
    } else {
        format!("{diagnostics_label}: observed value {value} violates > 0")
    };

    push_validation_check(
        checks,
        check_id,
        status,
        category,
        field,
        None,
        HawkesKernelParameterConstraint::FiniteAndStrictlyPositive,
        value,
        Some(0.0),
        "value is finite and strictly greater than 0",
        diagnostics,
        remediation_hint,
    );

    is_valid
}

fn validate_non_negative_check(
    checks: &mut Vec<HawkesKernelParameterCheck>,
    check_id: &'static str,
    category: HawkesKernelParameterCheckCategory,
    field: &'static str,
    value: f64,
    diagnostics_label: &'static str,
    remediation_hint: &'static str,
) -> bool {
    let is_valid = value.is_finite() && value >= 0.0;
    let status = HawkesKernelValidationStatus::from_bool(is_valid);

    let diagnostics = if is_valid {
        format!("{diagnostics_label}: observed value {value} satisfies >= 0")
    } else {
        format!("{diagnostics_label}: observed value {value} violates >= 0")
    };

    push_validation_check(
        checks,
        check_id,
        status,
        category,
        field,
        None,
        HawkesKernelParameterConstraint::FiniteAndNonNegative,
        value,
        Some(0.0),
        "value is finite and greater than or equal to 0",
        diagnostics,
        remediation_hint,
    );

    is_valid
}

fn validate_greater_or_equal_to_comparison_field_check(
    checks: &mut Vec<HawkesKernelParameterCheck>,
    check_id: &'static str,
    category: HawkesKernelParameterCheckCategory,
    field: &'static str,
    value: f64,
    comparison_field: &'static str,
    comparison_value: f64,
    diagnostics_label: &'static str,
    remediation_hint: &'static str,
) -> bool {
    let is_valid = value.is_finite() && comparison_value.is_finite() && value >= comparison_value;
    let status = HawkesKernelValidationStatus::from_bool(is_valid);

    let diagnostics = if is_valid {
        format!(
            "{diagnostics_label}: observed value {value} satisfies >= comparison value {comparison_value}"
        )
    } else {
        format!(
            "{diagnostics_label}: observed value {value} violates >= comparison value {comparison_value}"
        )
    };

    push_validation_check(
        checks,
        check_id,
        status,
        category,
        field,
        Some(comparison_field),
        HawkesKernelParameterConstraint::GreaterThanOrEqualToComparisonField,
        value,
        Some(comparison_value),
        "value is finite and greater than or equal to comparison field value",
        diagnostics,
        remediation_hint,
    );

    is_valid
}

fn validate_greater_or_equal_threshold_check(
    checks: &mut Vec<HawkesKernelParameterCheck>,
    check_id: &'static str,
    category: HawkesKernelParameterCheckCategory,
    field: &'static str,
    value: f64,
    threshold_value: f64,
    diagnostics_label: &'static str,
    remediation_hint: &'static str,
) -> bool {
    let is_valid = value.is_finite() && value >= threshold_value;
    let status = HawkesKernelValidationStatus::from_bool(is_valid);

    let diagnostics = if is_valid {
        format!(
            "{diagnostics_label}: observed value {value} satisfies >= threshold {threshold_value}"
        )
    } else {
        format!(
            "{diagnostics_label}: observed value {value} violates >= threshold {threshold_value}"
        )
    };

    push_validation_check(
        checks,
        check_id,
        status,
        category,
        field,
        None,
        HawkesKernelParameterConstraint::GreaterThanOrEqualToThreshold,
        value,
        Some(threshold_value),
        "value is finite and greater than or equal to configured threshold",
        diagnostics,
        remediation_hint,
    );

    is_valid
}

pub(crate) fn validate_non_negative_loadings(
    loadings: HawkesKernelLoadings,
    component_fields: [&'static str; 4],
) -> Result<(), HawkesKernelParameterError> {
    let mut violations: Vec<HawkesKernelParameterViolation> = Vec::new();
    collect_non_negative_loading_violations(loadings, component_fields, &mut violations);

    if let Some(violation) = violations.first().copied() {
        return Err(map_parameter_violation_to_error(violation));
    }

    Ok(())
}

fn collect_non_negative_loading_violations(
    loadings: HawkesKernelLoadings,
    component_fields: [&'static str; 4],
    violations: &mut Vec<HawkesKernelParameterViolation>,
) {
    for (component_field_name, component_value) in component_fields
        .into_iter()
        .zip(loadings.as_model_ordered_array())
    {
        if !component_value.is_finite() || component_value < 0.0 {
            violations.push(HawkesKernelParameterViolation {
                field: component_field_name,
                constraint: HawkesKernelParameterConstraint::FiniteAndNonNegative,
                value: component_value,
            });
        }
    }
}

fn map_parameter_violation_to_error(
    violation: HawkesKernelParameterViolation,
) -> HawkesKernelParameterError {
    map_parameter_violation_to_error_with_stability_tolerance(
        violation,
        HAWKES_DEFAULT_STABILITY_NUMERIC_TOLERANCE,
    )
}

fn map_parameter_violation_to_error_with_stability_tolerance(
    violation: HawkesKernelParameterViolation,
    stability_numeric_tolerance: f64,
) -> HawkesKernelParameterError {
    match violation.field {
        "lambda_bar" => HawkesKernelParameterError::InvalidLambdaBar {
            value: violation.value,
        },
        "beta" => HawkesKernelParameterError::InvalidBeta {
            value: violation.value,
        },
        "stability.numeric_tolerance" => {
            HawkesKernelParameterError::InvalidStabilityNumericTolerance {
                value: violation.value,
            }
        }
        "alpha_over_beta.spectral_radius" => HawkesKernelParameterError::UnstableSpectralRadius {
            spectral_radius_alpha_over_beta: violation.value,
            strict_stationarity_threshold: HAWKES_STRICT_STATIONARITY_THRESHOLD,
            numeric_tolerance: stability_numeric_tolerance,
            effective_stationarity_threshold: HAWKES_STRICT_STATIONARITY_THRESHOLD
                - stability_numeric_tolerance,
        },
        _ => HawkesKernelParameterError::InvalidLoadingComponent {
            field: violation.field,
            value: violation.value,
        },
    }
}

fn compute_spectral_radius_stability_check(
    alpha: HawkesKernelLoadings,
    beta: f64,
    numeric_tolerance: f64,
) -> HawkesSpectralRadiusStabilityCheck {
    let spectral_radius_alpha_over_beta = compute_spectral_radius_alpha_over_beta(alpha, beta);
    let effective_stationarity_threshold = HAWKES_STRICT_STATIONARITY_THRESHOLD - numeric_tolerance;

    HawkesSpectralRadiusStabilityCheck {
        spectral_radius_alpha_over_beta,
        strict_stationarity_threshold: HAWKES_STRICT_STATIONARITY_THRESHOLD,
        numeric_tolerance,
        effective_stationarity_threshold,
        is_stable: spectral_radius_alpha_over_beta < effective_stationarity_threshold,
    }
}

fn compute_spectral_radius_alpha_over_beta(alpha: HawkesKernelLoadings, beta: f64) -> f64 {
    let scaled_plus_plus = alpha.plus_plus / beta;
    let scaled_plus_minus = alpha.plus_minus / beta;
    let scaled_minus_plus = alpha.minus_plus / beta;
    let scaled_minus_minus = alpha.minus_minus / beta;

    let trace = scaled_plus_plus + scaled_minus_minus;
    let discriminant = ((scaled_plus_plus - scaled_minus_minus).powi(2)
        + 4.0 * scaled_plus_minus * scaled_minus_plus)
        .max(0.0);
    let square_root_discriminant = discriminant.sqrt();

    let first_eigenvalue = 0.5 * (trace + square_root_discriminant);
    let second_eigenvalue = 0.5 * (trace - square_root_discriminant);

    first_eigenvalue.abs().max(second_eigenvalue.abs())
}

pub(crate) fn derive_self_to_cross_ratio(self_loading: f64, cross_loading: f64) -> f64 {
    if cross_loading == 0.0 {
        return if self_loading > 0.0 { f64::MAX } else { 0.0 };
    }

    self_loading / cross_loading
}
