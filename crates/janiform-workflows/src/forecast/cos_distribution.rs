use janiform_foundations::complex_math::ComplexScalar;

use crate::error::WorkflowError;

use super::cdf_projection::isotonic_non_decreasing_projection;
use super::numeric::usize_to_f64;
use super::request::validation_error;

pub(super) struct CosDistribution {
    pub(super) support_lower: f64,
    pub(super) support_upper: f64,
    coefficients: Vec<f64>,
}

impl CosDistribution {
    pub(super) fn from_law(
        law: &janiform_engine_pricing::AffineForecastTerminalLaw,
    ) -> Result<Self, WorkflowError> {
        let support_lower = law.terminal_log_return_lower_bound;
        let support_upper = law.terminal_log_return_upper_bound;
        let width = support_upper - support_lower;

        if !width.is_finite() || width <= 0.0 {
            return Err(validation_error(
                "distribution_support",
                "distribution support width must be finite and positive",
            ));
        }

        let coefficients = law
            .characteristic_function_values
            .iter()
            .enumerate()
            .map(|(frequency_index, characteristic_value)| {
                let frequency = usize_to_f64(frequency_index, "frequency_index")?
                    * std::f64::consts::PI
                    / width;
                let log_return_characteristic = phase_shift_characteristic(
                    *characteristic_value,
                    -frequency * law.observed_log_price,
                );
                let support_shifted = phase_shift_characteristic(
                    log_return_characteristic,
                    -frequency * support_lower,
                );

                Ok(2.0 * support_shifted.real / width)
            })
            .collect::<Result<Vec<_>, WorkflowError>>()?;

        Ok(Self {
            support_lower,
            support_upper,
            coefficients,
        })
    }

    pub(super) fn density(&self, log_return: f64) -> Result<f64, WorkflowError> {
        let width = self.support_upper - self.support_lower;
        let offset = log_return - self.support_lower;

        self.coefficients
            .iter()
            .enumerate()
            .map(|(frequency_index, coefficient)| {
                if frequency_index == 0 {
                    Ok(0.5 * coefficient)
                } else {
                    let frequency = usize_to_f64(frequency_index, "frequency_index")?
                        * std::f64::consts::PI
                        / width;

                    Ok(coefficient * (frequency * offset).cos())
                }
            })
            .sum()
    }

    pub(super) fn projected_cdf(&self, log_return: f64) -> Result<f64, WorkflowError> {
        let width = self.support_upper - self.support_lower;
        let bounded_log_return = self.support_bounded_log_return(log_return);
        let offset = bounded_log_return - self.support_lower;

        let mut cumulative = 0.0;

        for (frequency_index, coefficient) in self.coefficients.iter().enumerate() {
            if frequency_index == 0 {
                cumulative += 0.5 * coefficient * offset;
            } else {
                let frequency = usize_to_f64(frequency_index, "frequency_index")?
                    * std::f64::consts::PI
                    / width;

                cumulative += coefficient * (frequency * offset).sin() / frequency;
            }
        }

        if !cumulative.is_finite() {
            return Err(validation_error(
                "terminal_distribution",
                "distribution CDF produced a non-finite value",
            ));
        }

        Ok(cumulative)
    }

    pub(super) fn quantiles(&self, probabilities: &[f64]) -> Result<Vec<f64>, WorkflowError> {
        for probability in probabilities {
            validate_quantile_probability(*probability)?;
        }

        let grid_count = quantile_inversion_grid_count(self.coefficients.len())?;
        let width = self.support_upper - self.support_lower;
        let step = width / usize_to_f64(grid_count - 1, "quantile_inversion_grid_count")?;
        let cdf_grid = self.bounded_isotonic_cdf_grid(grid_count, step)?;

        probabilities
            .iter()
            .copied()
            .map(|probability| self.quantile_from_cdf_grid(probability, cdf_grid.as_slice(), step))
            .collect()
    }

    fn quantile_from_cdf_grid(
        &self,
        probability: f64,
        cdf_grid: &[f64],
        step: f64,
    ) -> Result<f64, WorkflowError> {
        let mut previous_x = self.support_lower;

        let Some(mut previous_cdf) = cdf_grid.first().copied() else {
            return Err(validation_error(
                "quantiles",
                "quantile resolution requires at least two distribution nodes",
            ));
        };

        for (node_position, current_cdf) in cdf_grid.iter().copied().enumerate().skip(1) {
            let current_x =
                self.support_lower + usize_to_f64(node_position, "node_position")? * step;

            if probability >= previous_cdf && probability <= current_cdf {
                let probability_width = current_cdf - previous_cdf;
                if probability_width == 0.0 {
                    return Ok(current_x);
                }

                let weight = (probability - previous_cdf) / probability_width;

                return Ok(previous_x + weight * (current_x - previous_x));
            }

            previous_x = current_x;
            previous_cdf = current_cdf;
        }

        Err(validation_error(
            "distribution_support",
            format!(
                "automatic terminal-law quantile {probability} was not resolved inside the configured support"
            ),
        ))
    }

    fn support_bounded_log_return(&self, log_return: f64) -> f64 {
        if log_return < self.support_lower {
            return self.support_lower;
        }

        if log_return > self.support_upper {
            return self.support_upper;
        }

        log_return
    }

    fn bounded_isotonic_cdf_grid(
        &self,
        grid_count: usize,
        step: f64,
    ) -> Result<Vec<f64>, WorkflowError> {
        let mut cdf_values = Vec::with_capacity(grid_count);

        for node_position in 0..grid_count {
            let x = self.support_lower + usize_to_f64(node_position, "node_position")? * step;
            cdf_values.push(self.projected_cdf(x)?);
        }

        isotonic_non_decreasing_projection(cdf_values.as_slice())
    }
}

/// Use one inversion sub-interval for each ordered pair of COS intervals.
///
/// The CDF is evaluated analytically, so quantile lookup should not be limited
/// to the spectral node spacing used to represent the density.
fn quantile_inversion_grid_count(coefficient_count: usize) -> Result<usize, WorkflowError> {
    if coefficient_count < 2 {
        return Err(validation_error(
            "quantiles",
            "quantile resolution requires at least two distribution nodes",
        ));
    }

    let interval_count = coefficient_count - 1;

    let refined_interval_count = interval_count.checked_mul(interval_count).ok_or_else(|| {
        validation_error(
            "quantile_inversion_grid_count",
            "quantile inversion interval count overflowed",
        )
    })?;

    refined_interval_count.checked_add(1).ok_or_else(|| {
        validation_error(
            "quantile_inversion_grid_count",
            "quantile inversion grid count overflowed",
        )
    })
}

fn validate_quantile_probability(probability: f64) -> Result<(), WorkflowError> {
    if !(0.0..=1.0).contains(&probability) || !probability.is_finite() {
        return Err(validation_error(
            "quantiles",
            "quantile probability must be finite and inside [0, 1]",
        ));
    }

    Ok(())
}

fn phase_shift_characteristic(value: ComplexScalar, phase: f64) -> ComplexScalar {
    let (sin, cos) = phase.sin_cos();

    ComplexScalar::new(
        value.real * cos - value.imaginary * sin,
        value.real * sin + value.imaginary * cos,
    )
}

#[cfg(test)]
mod tests {
    use janiform_foundations::complex_math::ComplexScalar;

    use super::super::test_support::{synthetic_terminal_law, synthetic_terminal_law_on_support};
    use super::CosDistribution;

    #[test]
    fn quantile_resolves_uniform_law() {
        let law = synthetic_terminal_law(vec![
            ComplexScalar::new(1.0, 0.0),
            ComplexScalar::new(0.0, 0.0),
            ComplexScalar::new(0.0, 0.0),
        ]);

        let distribution =
            CosDistribution::from_law(&law).expect("synthetic uniform distribution should build");

        let quantiles = distribution
            .quantiles(&[0.25, 0.50, 0.75])
            .expect("uniform quantiles should resolve");

        assert_eq!(quantiles, vec![-0.5, 0.0, 0.5]);
    }

    #[test]
    fn quantile_inversion_uses_refined_cdf_nodes_between_cos_nodes() {
        let law = synthetic_terminal_law_on_support(
            vec![
                ComplexScalar::new(1.0, 0.0),
                ComplexScalar::new(0.25, 0.0),
                ComplexScalar::new(0.0, 0.0),
            ],
            0.0,
            1.0,
        );

        let distribution =
            CosDistribution::from_law(&law).expect("synthetic curved distribution should build");

        let target_log_return = 0.25;
        let target_probability = distribution.projected_cdf(target_log_return).unwrap();

        let quantiles = distribution
            .quantiles(&[target_probability])
            .expect("target quantile should resolve");

        assert_eq!(quantiles, vec![target_log_return]);
    }
}
