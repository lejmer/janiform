use crate::error::WorkflowError;

use super::cos_distribution::CosDistribution;
use super::numeric::usize_to_f64;
use super::output::{ForecastWarning, ForecastWarningSeverity};
use super::request::validation_error;

pub(super) const AUTOMATIC_SUPPORT_PILOT_LOG_RETURN_LOWER: f64 = -8.0;
pub(super) const AUTOMATIC_SUPPORT_PILOT_LOG_RETURN_UPPER: f64 = 8.0;

const AUTOMATIC_SUPPORT_LOWER_PROBABILITY: f64 = 0.01;
const AUTOMATIC_SUPPORT_UPPER_PROBABILITY: f64 = 0.99;
const AUTOMATIC_SUPPORT_STANDARD_DEVIATION_MULTIPLIER: f64 = 16.0;
const AUTOMATIC_SUPPORT_QUANTILE_POLICY: &str =
    "terminal_law_projected_cdf_quantile_support_p01_p99.v1";
const AUTOMATIC_SUPPORT_SCALE_FALLBACK_POLICY: &str =
    "terminal_law_characteristic_median_scale_support_mean_plus_minus_16_stddev.v1";
const AUTOMATIC_SUPPORT_SCALE_POLICY: &str =
    "median_positive_variance_estimate_from_characteristic_modulus.v1";
const EXPLICIT_SUPPORT_POLICY: &str =
    "explicit_terminal_log_return_support_bounded_isotonic_cdf_projection.v1";

pub(super) struct SupportResolution {
    pub(super) lower: f64,
    pub(super) upper: f64,
    pub(super) policy: &'static str,
    pub(super) lower_probability: Option<f64>,
    pub(super) upper_probability: Option<f64>,
    pub(super) warnings: Vec<ForecastWarning>,
}

pub(super) fn automatic_support_resolution(
    pilot_law: &janiform_engine_pricing::AffineForecastTerminalLaw,
) -> Result<SupportResolution, WorkflowError> {
    match automatic_quantile_support_resolution(pilot_law) {
        Ok(support_resolution) => Ok(support_resolution),
        Err(quantile_failure) => {
            automatic_scale_support_resolution(pilot_law, quantile_failure.to_string())
        }
    }
}

fn automatic_quantile_support_resolution(
    pilot_law: &janiform_engine_pricing::AffineForecastTerminalLaw,
) -> Result<SupportResolution, WorkflowError> {
    let pilot_distribution = CosDistribution::from_law(pilot_law)?;
    let lower = pilot_distribution.quantile(AUTOMATIC_SUPPORT_LOWER_PROBABILITY)?;
    let upper = pilot_distribution.quantile(AUTOMATIC_SUPPORT_UPPER_PROBABILITY)?;

    if !(lower.is_finite() && upper.is_finite() && upper > lower) {
        return Err(validation_error(
            "distribution_support",
            "automatic terminal-law quantile support could not be resolved",
        ));
    }

    Ok(SupportResolution {
        lower,
        upper,
        policy: AUTOMATIC_SUPPORT_QUANTILE_POLICY,
        lower_probability: Some(AUTOMATIC_SUPPORT_LOWER_PROBABILITY),
        upper_probability: Some(AUTOMATIC_SUPPORT_UPPER_PROBABILITY),
        warnings: Vec::new(),
    })
}

fn automatic_scale_support_resolution(
    pilot_law: &janiform_engine_pricing::AffineForecastTerminalLaw,
    quantile_failure: String,
) -> Result<SupportResolution, WorkflowError> {
    let standard_deviation = estimate_terminal_standard_deviation(pilot_law)?;
    let half_width = AUTOMATIC_SUPPORT_STANDARD_DEVIATION_MULTIPLIER * standard_deviation;

    let center = pilot_law.expected_log_return;
    let lower = center - half_width;
    let upper = center + half_width;

    if !(lower.is_finite() && upper.is_finite() && upper > lower) {
        return Err(validation_error(
            "distribution_support",
            "automatic terminal-law scale support could not be resolved",
        ));
    }

    Ok(SupportResolution {
        lower,
        upper,
        policy: AUTOMATIC_SUPPORT_SCALE_FALLBACK_POLICY,
        lower_probability: None,
        upper_probability: None,
        warnings: vec![ForecastWarning {
            code: "distribution_support_resolved_from_characteristic_scale".to_string(),
            severity: ForecastWarningSeverity::Warning,
            message: "automatic forecast support quantile inversion was not usable, so support was resolved from terminal characteristic-function scale".to_string(),
            fields: serde_json::json!({
                "pilot_log_return_lower": AUTOMATIC_SUPPORT_PILOT_LOG_RETURN_LOWER,
                "pilot_log_return_upper": AUTOMATIC_SUPPORT_PILOT_LOG_RETURN_UPPER,
                "quantile_policy": AUTOMATIC_SUPPORT_QUANTILE_POLICY,
                "quantile_failure": quantile_failure,
                "scale_policy": AUTOMATIC_SUPPORT_SCALE_POLICY,
                "standard_deviation": standard_deviation,
                "standard_deviation_multiplier": AUTOMATIC_SUPPORT_STANDARD_DEVIATION_MULTIPLIER,
                "support_lower": lower,
                "support_upper": upper,
            }),
        }],
    })
}

pub(super) fn explicit_support_resolution(
    lower: f64,
    upper: f64,
) -> Result<SupportResolution, WorkflowError> {
    if !(lower.is_finite() && upper.is_finite() && upper > lower) {
        return Err(validation_error(
            "distribution_log_return_bounds",
            "explicit support bounds must be finite with upper greater than lower",
        ));
    }

    Ok(SupportResolution {
        lower,
        upper,
        policy: EXPLICIT_SUPPORT_POLICY,
        lower_probability: None,
        upper_probability: None,
        warnings: Vec::new(),
    })
}

fn estimate_terminal_standard_deviation(
    law: &janiform_engine_pricing::AffineForecastTerminalLaw,
) -> Result<f64, WorkflowError> {
    let support_width = law.terminal_log_return_upper_bound - law.terminal_log_return_lower_bound;
    if !support_width.is_finite() || support_width <= 0.0 {
        return Err(validation_error(
            "distribution_support",
            "pilot support width must be finite and positive",
        ));
    }

    let mut estimates = Vec::new();

    for (frequency_index, characteristic_value) in law
        .characteristic_function_values
        .iter()
        .enumerate()
        .skip(1)
    {
        let frequency = usize_to_f64(frequency_index, "frequency_index")? * std::f64::consts::PI
            / support_width;

        let modulus = (characteristic_value.real * characteristic_value.real
            + characteristic_value.imaginary * characteristic_value.imaginary)
            .sqrt();

        if !(modulus.is_finite() && modulus > 0.0 && modulus < 1.0) {
            continue;
        }

        let log_decay = -modulus.ln();
        let variance = 2.0 * log_decay / (frequency * frequency);

        if variance.is_finite() && variance > 0.0 {
            estimates.push(variance.sqrt());
        }
    }

    median_positive_scale_estimate(estimates.as_mut_slice())
}

fn median_positive_scale_estimate(estimates: &mut [f64]) -> Result<f64, WorkflowError> {
    if estimates.is_empty() {
        return Err(finite_positive_scale_error());
    }

    estimates.sort_by(|left, right| left.total_cmp(right));

    let middle_position = estimates.len() / 2;

    let estimate = match estimates.len() % 2 {
        0 => 0.5 * (estimates[middle_position - 1] + estimates[middle_position]),
        _ => estimates[middle_position],
    };

    if !estimate.is_finite() || estimate <= 0.0 {
        return Err(finite_positive_scale_error());
    }

    Ok(estimate)
}

fn finite_positive_scale_error() -> WorkflowError {
    validation_error(
        "distribution_support",
        "terminal characteristic function did not yield a finite positive scale estimate",
    )
}

#[cfg(test)]
mod tests {
    use janiform_foundations::complex_math::ComplexScalar;

    use super::{
        AUTOMATIC_SUPPORT_LOWER_PROBABILITY, AUTOMATIC_SUPPORT_QUANTILE_POLICY,
        AUTOMATIC_SUPPORT_SCALE_FALLBACK_POLICY, AUTOMATIC_SUPPORT_UPPER_PROBABILITY,
        automatic_support_resolution,
    };

    fn synthetic_terminal_law(
        characteristic_function_values: Vec<ComplexScalar>,
    ) -> janiform_engine_pricing::AffineForecastTerminalLaw {
        janiform_engine_pricing::AffineForecastTerminalLaw {
            maturity_interval: 1.0,
            observed_log_price: 0.0,
            expected_log_return: 0.0,
            log_expected_price_ratio: 0.0,
            terminal_log_return_lower_bound: -1.0,
            terminal_log_return_upper_bound: 1.0,
            terminal_log_price_lower_bound: -1.0,
            terminal_log_price_upper_bound: 1.0,
            node_count: characteristic_function_values.len(),
            characteristic_function_values,
        }
    }

    #[test]
    fn automatic_support_uses_projected_quantile_support_when_available() {
        let law = synthetic_terminal_law(vec![
            ComplexScalar::new(1.0, 0.0),
            ComplexScalar::new(0.0, 0.0),
            ComplexScalar::new(0.0, 0.0),
        ]);

        let support = automatic_support_resolution(&law)
            .expect("uniform projected CDF support should resolve");

        assert_eq!(support.policy, AUTOMATIC_SUPPORT_QUANTILE_POLICY);
        assert_eq!(
            (
                support.lower_probability,
                support.upper_probability,
                support.warnings.len()
            ),
            (
                Some(AUTOMATIC_SUPPORT_LOWER_PROBABILITY),
                Some(AUTOMATIC_SUPPORT_UPPER_PROBABILITY),
                0
            )
        );
        assert_eq!(
            support.lower,
            -1.0 + 2.0 * AUTOMATIC_SUPPORT_LOWER_PROBABILITY
        );
        assert_eq!(
            support.upper,
            -1.0 + 2.0 * AUTOMATIC_SUPPORT_UPPER_PROBABILITY
        );
    }

    #[test]
    fn automatic_support_falls_back_to_characteristic_scale_with_warning() {
        let law = synthetic_terminal_law(vec![
            ComplexScalar::new(0.0, 0.0),
            ComplexScalar::new(0.5, 0.0),
            ComplexScalar::new(0.25, 0.0),
        ]);

        let support = automatic_support_resolution(&law)
            .expect("scale fallback should resolve from finite characteristic modulus");

        assert_eq!(support.policy, AUTOMATIC_SUPPORT_SCALE_FALLBACK_POLICY);
        assert_eq!(
            (support.lower_probability, support.upper_probability),
            (None, None)
        );
        assert_eq!(support.warnings.len(), 1);
        assert!(
            support.warnings[0]
                .message
                .contains("quantile inversion was not usable")
        );
    }
}
