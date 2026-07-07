use crate::error::WorkflowError;

use super::numeric::usize_to_f64;
use super::output::ForecastWarning;
use super::request::validation_error;

pub(super) const AUTOMATIC_SUPPORT_PILOT_LOG_RETURN_LOWER: f64 = -8.0;
pub(super) const AUTOMATIC_SUPPORT_PILOT_LOG_RETURN_UPPER: f64 = 8.0;

const AUTOMATIC_SUPPORT_STANDARD_DEVIATION_MULTIPLIER: f64 = 16.0;
const AUTOMATIC_SUPPORT_POLICY: &str =
    "terminal_law_characteristic_median_scale_support_mean_plus_minus_16_stddev.v1";
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
        policy: AUTOMATIC_SUPPORT_POLICY,
        lower_probability: None,
        upper_probability: None,
        warnings: Vec::new(),
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

    use super::super::test_support::synthetic_terminal_law;
    use super::{
        AUTOMATIC_SUPPORT_POLICY, AUTOMATIC_SUPPORT_STANDARD_DEVIATION_MULTIPLIER,
        automatic_support_resolution, estimate_terminal_standard_deviation,
    };

    #[test]
    fn automatic_support_uses_characteristic_scale_support() {
        let law = synthetic_terminal_law(vec![
            ComplexScalar::new(1.0, 0.0),
            ComplexScalar::new(0.5, 0.0),
            ComplexScalar::new(0.25, 0.0),
        ]);

        let support = automatic_support_resolution(&law)
            .expect("finite characteristic modulus should resolve support");

        let standard_deviation =
            estimate_terminal_standard_deviation(&law).expect("synthetic law should have scale");

        let half_width = AUTOMATIC_SUPPORT_STANDARD_DEVIATION_MULTIPLIER * standard_deviation;

        assert_eq!(support.policy, AUTOMATIC_SUPPORT_POLICY);
        assert_eq!(
            (
                support.lower_probability,
                support.upper_probability,
                support.warnings.len()
            ),
            (None, None, 0)
        );
        assert_eq!(support.lower, law.expected_log_return - half_width);
        assert_eq!(support.upper, law.expected_log_return + half_width);
    }

    #[test]
    fn automatic_support_rejects_laws_without_positive_characteristic_scale() {
        let law = synthetic_terminal_law(vec![
            ComplexScalar::new(0.0, 0.0),
            ComplexScalar::new(0.0, 0.0),
            ComplexScalar::new(0.0, 0.0),
        ]);

        assert!(automatic_support_resolution(&law).is_err());
    }
}
