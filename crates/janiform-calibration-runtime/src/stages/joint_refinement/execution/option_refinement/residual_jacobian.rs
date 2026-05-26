use std::collections::BTreeMap;

use super::objective_gradient::JointRefinementOptionNodeResidualSensitivity;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WeightedOptionResidualJacobianRow {
    pub(crate) residual: f64,
    pub(crate) gradient: Vec<f64>,
}

pub(crate) fn weighted_option_residual_jacobian_rows(
    node_sensitivities: &[JointRefinementOptionNodeResidualSensitivity],
) -> Result<Vec<WeightedOptionResidualJacobianRow>, String> {
    let Some(first_node) = node_sensitivities.first() else {
        return Err(
            "option residual Jacobian requires at least one priced option node".to_string(),
        );
    };

    let gradient_dimension = first_node.spread_normalized_residual_gradient.len();

    let mut quote_count_by_expiry = BTreeMap::<i64, usize>::new();
    let mut maturity_weight_by_expiry = BTreeMap::<i64, usize>::new();

    for node in node_sensitivities {
        if !node.spread_normalized_residual.is_finite()
            || node
                .spread_normalized_residual_gradient
                .iter()
                .any(|value| !value.is_finite())
        {
            return Err(
                "option residual Jacobian received non-finite node sensitivities".to_string(),
            );
        }

        if node.spread_normalized_residual_gradient.len() != gradient_dimension {
            return Err(
                "option residual Jacobian received inconsistent node gradient dimensions"
                    .to_string(),
            );
        }

        *quote_count_by_expiry
            .entry(node.expiry_timestamp_utc_nanos)
            .or_insert(0) += 1;

        match maturity_weight_by_expiry.get(&node.expiry_timestamp_utc_nanos) {
            Some(existing_weight) if *existing_weight != node.maturity_weight => {
                return Err(
                    "option residual Jacobian encountered inconsistent maturity weights"
                        .to_string(),
                );
            }
            Some(_) => {}
            None => {
                maturity_weight_by_expiry
                    .insert(node.expiry_timestamp_utc_nanos, node.maturity_weight);
            }
        }
    }

    let total_maturity_weight = maturity_weight_by_expiry.values().copied().sum::<usize>();
    if total_maturity_weight == 0 {
        return Err("option residual Jacobian requires positive total maturity weight".to_string());
    }

    node_sensitivities
        .iter()
        .map(|node| {
            let quote_count = quote_count_by_expiry
                .get(&node.expiry_timestamp_utc_nanos)
                .copied()
                .ok_or_else(|| {
                    "option residual Jacobian quote-count lookup failed for maturity".to_string()
                })?;

            let normalized_row_weight = ((node.maturity_weight as f64)
                / (quote_count as f64 * total_maturity_weight as f64))
                .sqrt();

            Ok(WeightedOptionResidualJacobianRow {
                residual: normalized_row_weight * node.spread_normalized_residual,
                gradient: node
                    .spread_normalized_residual_gradient
                    .iter()
                    .map(|value| normalized_row_weight * value)
                    .collect(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::objective_gradient::JointRefinementOptionNodeResidualSensitivity;
    use super::weighted_option_residual_jacobian_rows;

    #[test]
    fn weighted_rows_preserve_tier_score_norm() {
        let node_sensitivities = vec![
            JointRefinementOptionNodeResidualSensitivity {
                expiry_timestamp_utc_nanos: 10,
                maturity_weight: 2,
                spread_normalized_residual: 3.0,
                spread_normalized_residual_gradient: vec![1.0, 0.0],
            },
            JointRefinementOptionNodeResidualSensitivity {
                expiry_timestamp_utc_nanos: 10,
                maturity_weight: 2,
                spread_normalized_residual: -1.0,
                spread_normalized_residual_gradient: vec![0.0, 2.0],
            },
            JointRefinementOptionNodeResidualSensitivity {
                expiry_timestamp_utc_nanos: 20,
                maturity_weight: 1,
                spread_normalized_residual: 4.0,
                spread_normalized_residual_gradient: vec![3.0, 4.0],
            },
        ];

        let weighted_rows = weighted_option_residual_jacobian_rows(node_sensitivities.as_slice())
            .expect("weighted residual Jacobian should resolve");

        let weighted_squared_norm = weighted_rows
            .iter()
            .map(|row| row.residual * row.residual)
            .sum::<f64>();
        let expected_tier_score_squared = 26.0 / 3.0;

        assert_close(weighted_squared_norm, expected_tier_score_squared);
    }

    fn assert_close(actual: f64, expected: f64) {
        let tolerance = 1.0e-12f64.max(1.0e-12 * actual.abs().max(expected.abs()));
        assert!(
            (actual - expected).abs() <= tolerance,
            "actual={actual} expected={expected} tolerance={tolerance}"
        );
    }
}
