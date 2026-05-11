use std::sync::Arc;

use janiform_foundations::complex_math::ComplexScalar;
use rustdct::{DctPlanner, TransformType2And3};

use super::sensitivity::NodePriceSharedPlusShiftComplexSensitivity;
#[cfg(test)]
use super::sensitivity::NodePriceSharedPlusShiftSensitivity;

pub(crate) fn values_to_cosine_coefficients(
    values_by_frequency: &[Vec<ComplexScalar>],
) -> Vec<Vec<ComplexScalar>> {
    let Some(node_count) = values_by_frequency.first().map(Vec::len) else {
        return Vec::new();
    };

    if node_count == 0 {
        return values_by_frequency.iter().map(|_| Vec::new()).collect();
    }

    let mut transform = CosineTransform::new(node_count);
    values_by_frequency
        .iter()
        .map(|values| transform.complex_coefficients(values))
        .collect()
}

pub(crate) fn values_to_cosine_coefficients_sensitivities(
    values_by_frequency: &[Vec<NodePriceSharedPlusShiftComplexSensitivity>],
) -> Vec<Vec<NodePriceSharedPlusShiftComplexSensitivity>> {
    let Some(node_count) = values_by_frequency.first().map(Vec::len) else {
        return Vec::new();
    };

    if node_count == 0 {
        return values_by_frequency.iter().map(|_| Vec::new()).collect();
    }

    let mut transform = CosineTransform::new(node_count);
    values_by_frequency
        .iter()
        .map(|values| transform.sensitivity_coefficients(values))
        .collect()
}

#[cfg(test)]
pub(crate) fn cosine_series_values_from_coefficients(coefficients: &[f64]) -> Vec<f64> {
    if coefficients.is_empty() {
        return Vec::new();
    }

    let mut transform = CosineTransform::new(coefficients.len());
    transform.real_values_from_coefficients(coefficients)
}

#[cfg(test)]
pub(crate) fn cosine_series_sensitivity_values_from_coefficients(
    coefficients: &[NodePriceSharedPlusShiftSensitivity],
) -> Vec<NodePriceSharedPlusShiftSensitivity> {
    if coefficients.is_empty() {
        return Vec::new();
    }

    let mut transform = CosineTransform::new(coefficients.len());
    transform.sensitivity_values_from_coefficients(coefficients)
}

struct CosineTransform {
    node_count: usize,
    dct: Arc<dyn TransformType2And3<f64>>,
    scratch: Vec<f64>,
}

impl CosineTransform {
    fn new(node_count: usize) -> Self {
        let mut planner = DctPlanner::new();
        let dct = planner.plan_dct2(node_count);
        let scratch = vec![0.0; dct.get_scratch_len()];

        Self {
            node_count,
            dct,
            scratch,
        }
    }

    fn complex_coefficients(&mut self, values: &[ComplexScalar]) -> Vec<ComplexScalar> {
        let mut real_values = values.iter().map(|value| value.real).collect::<Vec<_>>();
        let mut imaginary_values = values
            .iter()
            .map(|value| value.imaginary)
            .collect::<Vec<_>>();

        self.apply_dct(&mut real_values);
        self.apply_dct(&mut imaginary_values);

        real_values
            .into_iter()
            .zip(imaginary_values)
            .enumerate()
            .map(|(basis_index, (real, imaginary))| {
                ComplexScalar::new(real, imaginary)
                    .scale(coefficient_scale(basis_index, self.node_count))
            })
            .collect()
    }

    #[cfg(test)]
    fn real_values_from_coefficients(&mut self, coefficients: &[f64]) -> Vec<f64> {
        let mut values = coefficients.to_vec();

        self.apply_dct3(&mut values);

        let scale = inverse_series_scale(self.node_count);
        values
            .into_iter()
            .map(|value| scale * value)
            .collect::<Vec<_>>()
    }

    fn sensitivity_coefficients(
        &mut self,
        values: &[NodePriceSharedPlusShiftComplexSensitivity],
    ) -> Vec<NodePriceSharedPlusShiftComplexSensitivity> {
        let mut alpha_real = values
            .iter()
            .map(|value| value.alpha_derivative.real)
            .collect::<Vec<_>>();

        let mut alpha_imaginary = values
            .iter()
            .map(|value| value.alpha_derivative.imaginary)
            .collect::<Vec<_>>();

        let mut beta_0_real = values
            .iter()
            .map(|value| value.beta_0_derivative.real)
            .collect::<Vec<_>>();

        let mut beta_0_imaginary = values
            .iter()
            .map(|value| value.beta_0_derivative.imaginary)
            .collect::<Vec<_>>();

        let mut delta_real = values
            .iter()
            .map(|value| value.delta_derivative.real)
            .collect::<Vec<_>>();

        let mut delta_imaginary = values
            .iter()
            .map(|value| value.delta_derivative.imaginary)
            .collect::<Vec<_>>();

        let mut delta_beta_real = values
            .iter()
            .map(|value| value.delta_beta_derivative.real)
            .collect::<Vec<_>>();

        let mut delta_beta_imaginary = values
            .iter()
            .map(|value| value.delta_beta_derivative.imaginary)
            .collect::<Vec<_>>();

        self.apply_dct(&mut alpha_real);
        self.apply_dct(&mut alpha_imaginary);
        self.apply_dct(&mut beta_0_real);
        self.apply_dct(&mut beta_0_imaginary);
        self.apply_dct(&mut delta_real);
        self.apply_dct(&mut delta_imaginary);
        self.apply_dct(&mut delta_beta_real);
        self.apply_dct(&mut delta_beta_imaginary);

        alpha_real
            .into_iter()
            .zip(alpha_imaginary)
            .zip(beta_0_real.into_iter().zip(beta_0_imaginary))
            .zip(delta_real.into_iter().zip(delta_imaginary))
            .zip(delta_beta_real.into_iter().zip(delta_beta_imaginary))
            .enumerate()
            .map(
                |(
                    basis_index,
                    (
                        (
                            ((alpha_real, alpha_imaginary), (beta_0_real, beta_0_imaginary)),
                            (delta_real, delta_imaginary),
                        ),
                        (delta_beta_real, delta_beta_imaginary),
                    ),
                )| {
                    let scale = coefficient_scale(basis_index, self.node_count);

                    NodePriceSharedPlusShiftComplexSensitivity {
                        alpha_derivative: ComplexScalar::new(alpha_real, alpha_imaginary)
                            .scale(scale),
                        beta_0_derivative: ComplexScalar::new(beta_0_real, beta_0_imaginary)
                            .scale(scale),
                        delta_derivative: ComplexScalar::new(delta_real, delta_imaginary)
                            .scale(scale),
                        delta_beta_derivative: ComplexScalar::new(
                            delta_beta_real,
                            delta_beta_imaginary,
                        )
                        .scale(scale),
                    }
                },
            )
            .collect()
    }

    #[cfg(test)]
    fn sensitivity_values_from_coefficients(
        &mut self,
        coefficients: &[NodePriceSharedPlusShiftSensitivity],
    ) -> Vec<NodePriceSharedPlusShiftSensitivity> {
        let mut alpha = coefficients
            .iter()
            .map(|coefficient| coefficient.alpha_derivative)
            .collect::<Vec<_>>();

        let mut beta_0 = coefficients
            .iter()
            .map(|coefficient| coefficient.beta_0_derivative)
            .collect::<Vec<_>>();

        let mut delta = coefficients
            .iter()
            .map(|coefficient| coefficient.delta_derivative)
            .collect::<Vec<_>>();

        let mut delta_beta = coefficients
            .iter()
            .map(|coefficient| coefficient.delta_beta_derivative)
            .collect::<Vec<_>>();

        self.apply_dct3(&mut alpha);
        self.apply_dct3(&mut beta_0);
        self.apply_dct3(&mut delta);
        self.apply_dct3(&mut delta_beta);

        let scale = inverse_series_scale(self.node_count);

        alpha
            .into_iter()
            .zip(beta_0)
            .zip(delta)
            .zip(delta_beta)
            .map(
                |(
                    ((alpha_derivative, beta_0_derivative), delta_derivative),
                    delta_beta_derivative,
                )| {
                    NodePriceSharedPlusShiftSensitivity {
                        alpha_derivative: scale * alpha_derivative,
                        beta_0_derivative: scale * beta_0_derivative,
                        delta_derivative: scale * delta_derivative,
                        delta_beta_derivative: scale * delta_beta_derivative,
                    }
                },
            )
            .collect()
    }

    fn apply_dct(&mut self, values: &mut [f64]) {
        self.dct
            .process_dct2_with_scratch(values, self.scratch.as_mut_slice());
    }

    #[cfg(test)]
    fn apply_dct3(&mut self, values: &mut [f64]) {
        self.dct
            .process_dct3_with_scratch(values, self.scratch.as_mut_slice());
    }
}

fn coefficient_scale(basis_index: usize, node_count: usize) -> f64 {
    if basis_index == 0 {
        1.0 / node_count as f64
    } else {
        2.0 / node_count as f64
    }
}

#[cfg(test)]
fn inverse_series_scale(node_count: usize) -> f64 {
    2.0 / node_count as f64
}

#[cfg(test)]
mod tests {
    use super::super::sensitivity::{
        NodePriceSharedPlusShiftComplexSensitivity, NodePriceSharedPlusShiftSensitivity,
    };
    use super::{
        coefficient_scale, cosine_series_sensitivity_values_from_coefficients,
        cosine_series_values_from_coefficients, values_to_cosine_coefficients,
        values_to_cosine_coefficients_sensitivities,
    };
    use janiform_foundations::complex_math::ComplexScalar;

    #[test]
    fn dct_coefficients_match_direct_midpoint_cosine_sum() {
        let values = vec![
            ComplexScalar::new(1.0, 0.5),
            ComplexScalar::new(-0.25, 1.25),
            ComplexScalar::new(0.75, -0.5),
            ComplexScalar::new(2.0, 0.0),
            ComplexScalar::new(-1.5, -0.75),
        ];

        let rows = vec![values];
        let coefficients = values_to_cosine_coefficients(rows.as_slice());
        let expected = direct_complex_coefficients(rows[0].as_slice());

        assert_complex_rows_close(coefficients[0].as_slice(), expected.as_slice());
    }

    #[test]
    fn dct_sensitivity_coefficients_match_direct_midpoint_cosine_sum() {
        let values = vec![
            NodePriceSharedPlusShiftComplexSensitivity {
                alpha_derivative: ComplexScalar::new(1.0, 0.5),
                beta_0_derivative: ComplexScalar::new(-1.0, 0.25),
                delta_derivative: ComplexScalar::new(0.25, -0.5),
                delta_beta_derivative: ComplexScalar::new(1.5, 0.75),
            },
            NodePriceSharedPlusShiftComplexSensitivity {
                alpha_derivative: ComplexScalar::new(-0.25, 1.25),
                beta_0_derivative: ComplexScalar::new(0.5, -1.25),
                delta_derivative: ComplexScalar::new(0.75, 0.5),
                delta_beta_derivative: ComplexScalar::new(-1.0, 1.0),
            },
            NodePriceSharedPlusShiftComplexSensitivity {
                alpha_derivative: ComplexScalar::new(0.75, -0.5),
                beta_0_derivative: ComplexScalar::new(1.25, 0.0),
                delta_derivative: ComplexScalar::new(-0.5, -0.25),
                delta_beta_derivative: ComplexScalar::new(0.0, 0.5),
            },
        ];

        let rows = vec![values];
        let coefficients = values_to_cosine_coefficients_sensitivities(rows.as_slice());
        let expected = direct_sensitivity_coefficients(rows[0].as_slice());

        assert_sensitivity_rows_close(coefficients[0].as_slice(), expected.as_slice());
    }

    #[test]
    fn dct3_values_match_direct_midpoint_cosine_series() {
        let coefficients = vec![0.75, -0.4, 1.25, 0.5, -0.2];

        let values = cosine_series_values_from_coefficients(coefficients.as_slice());
        let expected = direct_real_values(coefficients.as_slice());

        assert_real_rows_close(values.as_slice(), expected.as_slice());
    }

    #[test]
    fn dct3_sensitivity_values_match_direct_midpoint_cosine_series() {
        let coefficients = vec![
            NodePriceSharedPlusShiftSensitivity {
                alpha_derivative: 1.0,
                beta_0_derivative: -1.0,
                delta_derivative: 0.25,
                delta_beta_derivative: 1.5,
            },
            NodePriceSharedPlusShiftSensitivity {
                alpha_derivative: -0.25,
                beta_0_derivative: 0.5,
                delta_derivative: 0.75,
                delta_beta_derivative: -1.0,
            },
            NodePriceSharedPlusShiftSensitivity {
                alpha_derivative: 0.75,
                beta_0_derivative: 1.25,
                delta_derivative: -0.5,
                delta_beta_derivative: 0.0,
            },
        ];

        let values = cosine_series_sensitivity_values_from_coefficients(coefficients.as_slice());
        let expected = direct_sensitivity_values(coefficients.as_slice());

        assert_real_rows_close(
            values
                .iter()
                .map(|value| value.alpha_derivative)
                .collect::<Vec<_>>()
                .as_slice(),
            expected
                .iter()
                .map(|value| value.alpha_derivative)
                .collect::<Vec<_>>()
                .as_slice(),
        );
        assert_real_rows_close(
            values
                .iter()
                .map(|value| value.beta_0_derivative)
                .collect::<Vec<_>>()
                .as_slice(),
            expected
                .iter()
                .map(|value| value.beta_0_derivative)
                .collect::<Vec<_>>()
                .as_slice(),
        );
        assert_real_rows_close(
            values
                .iter()
                .map(|value| value.delta_derivative)
                .collect::<Vec<_>>()
                .as_slice(),
            expected
                .iter()
                .map(|value| value.delta_derivative)
                .collect::<Vec<_>>()
                .as_slice(),
        );
        assert_real_rows_close(
            values
                .iter()
                .map(|value| value.delta_beta_derivative)
                .collect::<Vec<_>>()
                .as_slice(),
            expected
                .iter()
                .map(|value| value.delta_beta_derivative)
                .collect::<Vec<_>>()
                .as_slice(),
        );
    }

    fn direct_complex_coefficients(values: &[ComplexScalar]) -> Vec<ComplexScalar> {
        (0..values.len())
            .map(|basis_index| {
                let scale = coefficient_scale(basis_index, values.len());
                values
                    .iter()
                    .enumerate()
                    .fold(
                        ComplexScalar::new(0.0, 0.0),
                        |coefficient, (node_index, value)| {
                            let angle = std::f64::consts::PI
                                * basis_index as f64
                                * (node_index as f64 + 0.5)
                                / values.len() as f64;

                            coefficient.plus(value.scale(angle.cos()))
                        },
                    )
                    .scale(scale)
            })
            .collect()
    }

    fn direct_real_values(coefficients: &[f64]) -> Vec<f64> {
        (0..coefficients.len())
            .map(|node_index| {
                coefficients
                    .iter()
                    .enumerate()
                    .map(|(basis_index, coefficient)| {
                        let basis_value = if basis_index == 0 {
                            1.0 / coefficients.len() as f64
                        } else {
                            let angle = std::f64::consts::PI
                                * basis_index as f64
                                * (node_index as f64 + 0.5)
                                / coefficients.len() as f64;

                            2.0 * angle.cos() / coefficients.len() as f64
                        };

                        coefficient * basis_value
                    })
                    .sum()
            })
            .collect()
    }

    fn direct_sensitivity_values(
        coefficients: &[NodePriceSharedPlusShiftSensitivity],
    ) -> Vec<NodePriceSharedPlusShiftSensitivity> {
        let alpha = direct_real_values(
            coefficients
                .iter()
                .map(|coefficient| coefficient.alpha_derivative)
                .collect::<Vec<_>>()
                .as_slice(),
        );
        let beta_0 = direct_real_values(
            coefficients
                .iter()
                .map(|coefficient| coefficient.beta_0_derivative)
                .collect::<Vec<_>>()
                .as_slice(),
        );
        let delta = direct_real_values(
            coefficients
                .iter()
                .map(|coefficient| coefficient.delta_derivative)
                .collect::<Vec<_>>()
                .as_slice(),
        );
        let delta_beta = direct_real_values(
            coefficients
                .iter()
                .map(|coefficient| coefficient.delta_beta_derivative)
                .collect::<Vec<_>>()
                .as_slice(),
        );

        alpha
            .into_iter()
            .zip(beta_0)
            .zip(delta)
            .zip(delta_beta)
            .map(
                |(
                    ((alpha_derivative, beta_0_derivative), delta_derivative),
                    delta_beta_derivative,
                )| {
                    NodePriceSharedPlusShiftSensitivity {
                        alpha_derivative,
                        beta_0_derivative,
                        delta_derivative,
                        delta_beta_derivative,
                    }
                },
            )
            .collect()
    }

    fn direct_sensitivity_coefficients(
        values: &[NodePriceSharedPlusShiftComplexSensitivity],
    ) -> Vec<NodePriceSharedPlusShiftComplexSensitivity> {
        (0..values.len())
            .map(|basis_index| {
                let scale = coefficient_scale(basis_index, values.len());
                values
                    .iter()
                    .enumerate()
                    .fold(
                        NodePriceSharedPlusShiftComplexSensitivity::zero(),
                        |coefficient, (node_index, value)| {
                            let angle = std::f64::consts::PI
                                * basis_index as f64
                                * (node_index as f64 + 0.5)
                                / values.len() as f64;

                            coefficient.plus(value.scale(angle.cos()))
                        },
                    )
                    .scale(scale)
            })
            .collect()
    }

    fn assert_complex_rows_close(actual: &[ComplexScalar], expected: &[ComplexScalar]) {
        for (actual_value, expected_value) in actual.iter().zip(expected.iter()) {
            assert!(
                (actual_value.real - expected_value.real).abs() <= f64::EPSILON.sqrt(),
                "actual real {} should match expected real {}",
                actual_value.real,
                expected_value.real
            );
            assert!(
                (actual_value.imaginary - expected_value.imaginary).abs() <= f64::EPSILON.sqrt(),
                "actual imaginary {} should match expected imaginary {}",
                actual_value.imaginary,
                expected_value.imaginary
            );
        }
    }

    fn assert_real_rows_close(actual: &[f64], expected: &[f64]) {
        for (actual_value, expected_value) in actual.iter().zip(expected.iter()) {
            assert!(
                (actual_value - expected_value).abs() <= f64::EPSILON.sqrt(),
                "actual value {actual_value} should match expected value {expected_value}",
            );
        }
    }

    fn assert_sensitivity_rows_close(
        actual: &[NodePriceSharedPlusShiftComplexSensitivity],
        expected: &[NodePriceSharedPlusShiftComplexSensitivity],
    ) {
        assert_complex_rows_close(
            actual
                .iter()
                .map(|value| value.alpha_derivative)
                .collect::<Vec<_>>()
                .as_slice(),
            expected
                .iter()
                .map(|value| value.alpha_derivative)
                .collect::<Vec<_>>()
                .as_slice(),
        );
        assert_complex_rows_close(
            actual
                .iter()
                .map(|value| value.beta_0_derivative)
                .collect::<Vec<_>>()
                .as_slice(),
            expected
                .iter()
                .map(|value| value.beta_0_derivative)
                .collect::<Vec<_>>()
                .as_slice(),
        );
        assert_complex_rows_close(
            actual
                .iter()
                .map(|value| value.delta_derivative)
                .collect::<Vec<_>>()
                .as_slice(),
            expected
                .iter()
                .map(|value| value.delta_derivative)
                .collect::<Vec<_>>()
                .as_slice(),
        );
        assert_complex_rows_close(
            actual
                .iter()
                .map(|value| value.delta_beta_derivative)
                .collect::<Vec<_>>()
                .as_slice(),
            expected
                .iter()
                .map(|value| value.delta_beta_derivative)
                .collect::<Vec<_>>()
                .as_slice(),
        );
    }
}
