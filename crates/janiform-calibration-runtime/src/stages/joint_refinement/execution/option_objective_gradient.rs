use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct JointRefinementOptionNodeResidualSensitivity {
    pub(crate) expiry_timestamp_utc_nanos: i64,
    pub(crate) maturity_weight: usize,
    pub(crate) spread_normalized_residual: f64,
    pub(crate) spread_normalized_residual_gradient: Vec<f64>,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct JointRefinementOptionMaturityScoreGradientArtifact {
    pub(crate) expiry_timestamp_utc_nanos: i64,
    pub(crate) maturity_weight: usize,
    pub(crate) quote_count: usize,
    pub(crate) root_mean_square_spread_normalized_residual: f64,
    pub(crate) gradient: Option<Vec<f64>>,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct JointRefinementOptionTierScoreGradientArtifact {
    pub(crate) root_mean_square_spread_normalized_residual: f64,
    pub(crate) gradient: Option<Vec<f64>>,
    pub(crate) maturity_gradients: Vec<JointRefinementOptionMaturityScoreGradientArtifact>,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone)]
struct OptionMaturityResidualGradientAccumulator {
    maturity_weight: usize,
    quote_count: usize,
    squared_residual_sum: f64,
    residual_gradient_inner_product_sum: Vec<f64>,
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn differentiate_option_tier_root_mean_square_score(
    node_sensitivities: &[JointRefinementOptionNodeResidualSensitivity],
) -> Result<JointRefinementOptionTierScoreGradientArtifact, String> {
    let Some(first_node) = node_sensitivities.first() else {
        return Err(
            "option objective gradient requires at least one priced option node".to_string(),
        );
    };

    let gradient_dimension = first_node.spread_normalized_residual_gradient.len();

    let mut residuals_by_maturity =
        BTreeMap::<i64, OptionMaturityResidualGradientAccumulator>::new();

    for node in node_sensitivities {
        if node.spread_normalized_residual_gradient.len() != gradient_dimension {
            return Err(
                "option objective gradient received inconsistent node gradient dimensions"
                    .to_string(),
            );
        }

        let accumulator = residuals_by_maturity
            .entry(node.expiry_timestamp_utc_nanos)
            .or_insert_with(|| OptionMaturityResidualGradientAccumulator {
                maturity_weight: node.maturity_weight,
                quote_count: 0,
                squared_residual_sum: 0.0,
                residual_gradient_inner_product_sum: vec![0.0; gradient_dimension],
            });

        if accumulator.maturity_weight != node.maturity_weight {
            return Err(
                "option objective gradient encountered inconsistent maturity weights".to_string(),
            );
        }

        accumulator.quote_count += 1;
        accumulator.squared_residual_sum +=
            node.spread_normalized_residual * node.spread_normalized_residual;

        for (gradient_sum, residual_gradient) in accumulator
            .residual_gradient_inner_product_sum
            .iter_mut()
            .zip(node.spread_normalized_residual_gradient.iter())
        {
            *gradient_sum += node.spread_normalized_residual * residual_gradient;
        }
    }

    let mut weighted_squared_maturity_fit_sum = 0.0;
    let mut total_maturity_weight = 0usize;
    let mut weighted_residual_gradient_inner_product_sum = vec![0.0; gradient_dimension];
    let mut maturity_gradients = Vec::with_capacity(residuals_by_maturity.len());

    for (expiry_timestamp_utc_nanos, accumulator) in residuals_by_maturity {
        let root_mean_square_spread_normalized_residual =
            (accumulator.squared_residual_sum / accumulator.quote_count as f64).sqrt();

        weighted_squared_maturity_fit_sum += accumulator.maturity_weight as f64
            * root_mean_square_spread_normalized_residual
            * root_mean_square_spread_normalized_residual;

        total_maturity_weight += accumulator.maturity_weight;

        for (weighted_sum, residual_gradient_inner_product_sum) in
            weighted_residual_gradient_inner_product_sum
                .iter_mut()
                .zip(accumulator.residual_gradient_inner_product_sum.iter())
        {
            *weighted_sum += accumulator.maturity_weight as f64
                * residual_gradient_inner_product_sum
                / accumulator.quote_count as f64;
        }

        let maturity_gradient = if root_mean_square_spread_normalized_residual > 0.0 {
            Some(
                accumulator
                    .residual_gradient_inner_product_sum
                    .iter()
                    .map(|residual_gradient_inner_product_sum| {
                        residual_gradient_inner_product_sum
                            / (accumulator.quote_count as f64
                                * root_mean_square_spread_normalized_residual)
                    })
                    .collect(),
            )
        } else {
            None
        };

        maturity_gradients.push(JointRefinementOptionMaturityScoreGradientArtifact {
            expiry_timestamp_utc_nanos,
            maturity_weight: accumulator.maturity_weight,
            quote_count: accumulator.quote_count,
            root_mean_square_spread_normalized_residual,
            gradient: maturity_gradient,
        });
    }

    if total_maturity_weight == 0 {
        return Err(
            "option objective gradient requires positive total maturity weight".to_string(),
        );
    }

    let root_mean_square_spread_normalized_residual =
        (weighted_squared_maturity_fit_sum / total_maturity_weight as f64).sqrt();

    let gradient = if root_mean_square_spread_normalized_residual > 0.0 {
        Some(
            weighted_residual_gradient_inner_product_sum
                .iter()
                .map(|weighted_sum| {
                    weighted_sum
                        / (total_maturity_weight as f64
                            * root_mean_square_spread_normalized_residual)
                })
                .collect(),
        )
    } else {
        None
    };

    Ok(JointRefinementOptionTierScoreGradientArtifact {
        root_mean_square_spread_normalized_residual,
        gradient,
        maturity_gradients,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        JointRefinementOptionNodeResidualSensitivity,
        differentiate_option_tier_root_mean_square_score,
    };

    #[test]
    fn option_tier_objective_gradient_matches_linearized_finite_difference() {
        let node_sensitivities = vec![
            JointRefinementOptionNodeResidualSensitivity {
                expiry_timestamp_utc_nanos: 10,
                maturity_weight: 2,
                spread_normalized_residual: 1.0,
                spread_normalized_residual_gradient: vec![0.5, -1.0],
            },
            JointRefinementOptionNodeResidualSensitivity {
                expiry_timestamp_utc_nanos: 10,
                maturity_weight: 2,
                spread_normalized_residual: -2.0,
                spread_normalized_residual_gradient: vec![1.5, 0.25],
            },
            JointRefinementOptionNodeResidualSensitivity {
                expiry_timestamp_utc_nanos: 20,
                maturity_weight: 1,
                spread_normalized_residual: 0.25,
                spread_normalized_residual_gradient: vec![-0.75, 0.5],
            },
        ];

        let gradient_artifact =
            differentiate_option_tier_root_mean_square_score(node_sensitivities.as_slice())
                .expect("gradient should resolve");

        let gradient = gradient_artifact
            .gradient
            .as_ref()
            .expect("tier score should be differentiable");

        let epsilon = 1.0e-6;

        for (coordinate_index, gradient_value) in gradient.iter().copied().enumerate() {
            let forward_score =
                perturbed_tier_score(node_sensitivities.as_slice(), coordinate_index, epsilon);
            let backward_score =
                perturbed_tier_score(node_sensitivities.as_slice(), coordinate_index, -epsilon);

            let finite_difference = (forward_score - backward_score) / (2.0 * epsilon);

            assert_close(gradient_value, finite_difference);
        }
    }

    #[test]
    fn zero_residual_maturity_is_not_reported_as_differentiable() {
        let node_sensitivities = vec![
            JointRefinementOptionNodeResidualSensitivity {
                expiry_timestamp_utc_nanos: 10,
                maturity_weight: 1,
                spread_normalized_residual: 0.0,
                spread_normalized_residual_gradient: vec![1.0],
            },
            JointRefinementOptionNodeResidualSensitivity {
                expiry_timestamp_utc_nanos: 10,
                maturity_weight: 1,
                spread_normalized_residual: 0.0,
                spread_normalized_residual_gradient: vec![-1.0],
            },
            JointRefinementOptionNodeResidualSensitivity {
                expiry_timestamp_utc_nanos: 20,
                maturity_weight: 2,
                spread_normalized_residual: 2.0,
                spread_normalized_residual_gradient: vec![0.5],
            },
        ];

        let gradient_artifact =
            differentiate_option_tier_root_mean_square_score(node_sensitivities.as_slice())
                .expect("gradient should resolve");

        assert_eq!(gradient_artifact.maturity_gradients.len(), 2);
        assert_eq!(gradient_artifact.maturity_gradients[0].gradient, None);
        assert_eq!(gradient_artifact.gradient, Some(vec![1.0 / 6.0f64.sqrt()]));
    }

    fn perturbed_tier_score(
        node_sensitivities: &[JointRefinementOptionNodeResidualSensitivity],
        coordinate_index: usize,
        coordinate_delta: f64,
    ) -> f64 {
        let perturbed_node_sensitivities = node_sensitivities
            .iter()
            .map(|node| JointRefinementOptionNodeResidualSensitivity {
                expiry_timestamp_utc_nanos: node.expiry_timestamp_utc_nanos,
                maturity_weight: node.maturity_weight,
                spread_normalized_residual: node.spread_normalized_residual
                    + coordinate_delta * node.spread_normalized_residual_gradient[coordinate_index],
                spread_normalized_residual_gradient: node
                    .spread_normalized_residual_gradient
                    .clone(),
            })
            .collect::<Vec<_>>();

        differentiate_option_tier_root_mean_square_score(perturbed_node_sensitivities.as_slice())
            .expect("perturbed score should resolve")
            .root_mean_square_spread_normalized_residual
    }

    fn assert_close(actual: f64, expected: f64) {
        let tolerance = 1.0e-8f64.max(1.0e-6 * actual.abs().max(expected.abs()));
        assert!(
            (actual - expected).abs() <= tolerance,
            "actual={actual} expected={expected} tolerance={tolerance}"
        );
    }
}
