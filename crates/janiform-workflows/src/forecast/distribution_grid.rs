use crate::error::WorkflowError;

use super::cos_distribution::CosDistribution;
use super::numeric::usize_to_f64;
use super::request::validation_error;

pub(super) const TERMINAL_DISTRIBUTION_GRID_POLICY: &str =
    "terminal_distribution_projected_cdf_body_quantile_bands_sixth_power_tail_log_return_bands.v1";

enum GridNode {
    LogReturn(f64),
    Probability(f64),
}

pub(super) fn projected_cdf_quantile_band_grid(
    distribution: &CosDistribution,
    node_count: usize,
    quantile_probabilities: &[f64],
    quantile_log_returns: &[f64],
) -> Result<Vec<f64>, WorkflowError> {
    if node_count < 2 {
        return Err(validation_error(
            "distribution_node_count",
            "terminal distribution grid requires at least two nodes",
        ));
    }

    if quantile_probabilities.len() != quantile_log_returns.len() {
        return Err(validation_error(
            "terminal_distribution",
            "distribution grid quantile probabilities and log returns must have matching lengths",
        ));
    }

    let probability_anchors = probability_anchors(quantile_probabilities)?;
    let log_return_anchors = log_return_anchors(distribution, quantile_log_returns)?;
    let denominator = usize_to_f64(node_count - 1, "distribution_node_count")?;
    let band_count = probability_anchors.len() - 1;

    let grid_nodes = (0..node_count)
        .map(|node_position| {
            grid_node(
                node_position,
                node_count,
                band_count,
                probability_anchors.as_slice(),
                log_return_anchors.as_slice(),
                denominator,
            )
        })
        .collect::<Result<Vec<_>, WorkflowError>>()?;

    let body_probabilities = grid_nodes
        .iter()
        .filter_map(|grid_node| match grid_node {
            GridNode::LogReturn(_) => None,
            GridNode::Probability(probability) => Some(*probability),
        })
        .collect::<Vec<_>>();

    let body_log_returns = distribution.quantiles(body_probabilities.as_slice())?;

    let mut body_log_return_iter = body_log_returns.into_iter();
    let mut log_returns = Vec::with_capacity(node_count);

    for grid_node in grid_nodes {
        match grid_node {
            GridNode::LogReturn(log_return) => log_returns.push(log_return),
            GridNode::Probability(_) => {
                log_returns.push(next_body_log_return(&mut body_log_return_iter)?);
            }
        }
    }

    validate_ordered_log_returns(log_returns.as_slice())?;

    Ok(log_returns)
}

fn probability_anchors(quantile_probabilities: &[f64]) -> Result<Vec<f64>, WorkflowError> {
    let mut anchors = Vec::with_capacity(quantile_probabilities.len() + 2);

    anchors.push(0.0);
    anchors.extend(quantile_probabilities.iter().copied());
    anchors.push(1.0);

    validate_probability_anchors(anchors.as_slice())?;

    Ok(anchors)
}

fn log_return_anchors(
    distribution: &CosDistribution,
    quantile_log_returns: &[f64],
) -> Result<Vec<f64>, WorkflowError> {
    let mut anchors = Vec::with_capacity(quantile_log_returns.len() + 2);

    anchors.push(distribution.support_lower);
    anchors.extend(quantile_log_returns.iter().copied());
    anchors.push(distribution.support_upper);

    validate_ordered_log_returns(anchors.as_slice())?;

    Ok(anchors)
}

fn validate_probability_anchors(anchors: &[f64]) -> Result<(), WorkflowError> {
    let mut anchor_iter = anchors.iter().copied();

    let Some(mut previous_anchor) = anchor_iter.next() else {
        return Err(validation_error(
            "terminal_distribution",
            "distribution grid probability anchors must not be empty",
        ));
    };

    if previous_anchor != 0.0 {
        return Err(validation_error(
            "terminal_distribution",
            "distribution grid probability anchors must start at zero",
        ));
    }

    for anchor in anchor_iter {
        if !anchor.is_finite() || anchor <= previous_anchor || anchor > 1.0 {
            return Err(validation_error(
                "terminal_distribution",
                "distribution grid probability anchors must be finite and strictly increasing inside [0, 1]",
            ));
        }

        previous_anchor = anchor;
    }

    if previous_anchor != 1.0 {
        return Err(validation_error(
            "terminal_distribution",
            "distribution grid probability anchors must end at one",
        ));
    }

    Ok(())
}

fn grid_node(
    node_position: usize,
    node_count: usize,
    band_count: usize,
    probability_anchors: &[f64],
    log_return_anchors: &[f64],
    denominator: f64,
) -> Result<GridNode, WorkflowError> {
    let numerator = node_position.checked_mul(band_count).ok_or_else(|| {
        validation_error("distribution_node_count", "distribution grid overflowed")
    })?;

    let grid_denominator = node_count - 1;
    let band_position = numerator / grid_denominator;
    let band_offset = numerator % grid_denominator;

    if band_position >= band_count {
        return Ok(GridNode::LogReturn(*log_return_anchors.last().ok_or_else(
            || {
                validation_error(
                    "terminal_distribution",
                    "distribution grid log-return anchors must not be empty",
                )
            },
        )?));
    }

    let lower_log_return = log_return_anchor(log_return_anchors, band_position)?;
    let upper_log_return = log_return_anchor(log_return_anchors, band_position + 1)?;
    let band_fraction = usize_to_f64(band_offset, "distribution_node_position")? / denominator;

    if band_position == 0 {
        return Ok(GridNode::LogReturn(lower_tail_interpolation(
            lower_log_return,
            upper_log_return,
            band_fraction,
        )?));
    }

    if band_position + 1 == band_count {
        return Ok(GridNode::LogReturn(upper_tail_interpolation(
            lower_log_return,
            upper_log_return,
            band_fraction,
        )?));
    }

    let lower_probability = probability_anchor(probability_anchors, band_position)?;
    let upper_probability = probability_anchor(probability_anchors, band_position + 1)?;

    Ok(GridNode::Probability(linear_interpolation(
        lower_probability,
        upper_probability,
        band_fraction,
    )?))
}

fn probability_anchor(anchors: &[f64], position: usize) -> Result<f64, WorkflowError> {
    anchors.get(position).copied().ok_or_else(|| {
        validation_error(
            "terminal_distribution",
            "distribution grid probability anchor index exceeded anchor count",
        )
    })
}

fn log_return_anchor(anchors: &[f64], position: usize) -> Result<f64, WorkflowError> {
    anchors.get(position).copied().ok_or_else(|| {
        validation_error(
            "terminal_distribution",
            "distribution grid log-return anchor index exceeded anchor count",
        )
    })
}

fn lower_tail_interpolation(lower: f64, upper: f64, weight: f64) -> Result<f64, WorkflowError> {
    let complement = 1.0 - validated_unit_weight(weight)?;
    let squared_complement = complement * complement;
    let quartic_complement = squared_complement * squared_complement;

    linear_interpolation(lower, upper, 1.0 - quartic_complement * squared_complement)
}

fn upper_tail_interpolation(lower: f64, upper: f64, weight: f64) -> Result<f64, WorkflowError> {
    let unit_weight = validated_unit_weight(weight)?;
    let squared_weight = unit_weight * unit_weight;
    let quartic_weight = squared_weight * squared_weight;

    linear_interpolation(lower, upper, quartic_weight * squared_weight)
}

fn validated_unit_weight(weight: f64) -> Result<f64, WorkflowError> {
    if !weight.is_finite() || !(0.0..=1.0).contains(&weight) {
        return Err(validation_error(
            "terminal_distribution",
            "distribution grid interpolation weight must be finite and inside [0, 1]",
        ));
    }

    Ok(weight)
}

fn linear_interpolation(lower: f64, upper: f64, weight: f64) -> Result<f64, WorkflowError> {
    if !lower.is_finite()
        || !upper.is_finite()
        || !weight.is_finite()
        || upper < lower
        || !(0.0..=1.0).contains(&weight)
    {
        return Err(validation_error(
            "terminal_distribution",
            "distribution grid interpolation inputs must be finite and ordered",
        ));
    }

    Ok(lower + weight * (upper - lower))
}

fn next_body_log_return(
    body_log_return_iter: &mut impl Iterator<Item = f64>,
) -> Result<f64, WorkflowError> {
    body_log_return_iter.next().ok_or_else(|| {
        validation_error(
            "terminal_distribution",
            "distribution body quantile grid produced fewer log-return nodes than requested",
        )
    })
}

fn validate_ordered_log_returns(log_returns: &[f64]) -> Result<(), WorkflowError> {
    let mut log_return_iter = log_returns.iter().copied();

    let Some(mut lower) = log_return_iter.next() else {
        return Ok(());
    };

    for upper in log_return_iter {
        if !lower.is_finite() || !upper.is_finite() || upper < lower {
            return Err(validation_error(
                "terminal_distribution",
                "adaptive distribution grid produced non-finite or decreasing log-return nodes",
            ));
        }

        lower = upper;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use janiform_foundations::complex_math::ComplexScalar;

    use super::super::cos_distribution::CosDistribution;
    use super::super::test_support::synthetic_terminal_law;
    use super::projected_cdf_quantile_band_grid;

    #[test]
    fn projected_cdf_quantile_band_grid_uses_forecast_quantile_bands() {
        let probabilities = [0.01, 0.05, 0.25, 0.50, 0.75, 0.95, 0.99];
        let grid = uniform_quantile_band_grid(probabilities.as_slice(), 9);

        let expected = [0.0, 0.01, 0.05, 0.25, 0.50, 0.75, 0.95, 0.99, 1.0]
            .iter()
            .copied()
            .map(|probability| -1.0 + 2.0 * probability)
            .collect::<Vec<_>>();

        assert_eq!(grid, expected);
    }

    #[test]
    fn projected_cdf_quantile_band_grid_clusters_tail_nodes_near_reported_quantiles() {
        let probabilities = [0.25, 0.50, 0.75];
        let grid = uniform_quantile_band_grid(probabilities.as_slice(), 17);

        assert_eq!(grid[1], -0.5889892578125);
        assert_eq!(grid[2], -0.5078125);
        assert_eq!(grid[3], -0.5001220703125);
        assert_eq!(grid[13], 0.5001220703125);
        assert_eq!(grid[14], 0.5078125);
        assert_eq!(grid[15], 0.5889892578125);
    }

    fn uniform_quantile_band_grid(probabilities: &[f64], node_count: usize) -> Vec<f64> {
        let law = synthetic_terminal_law(vec![
            ComplexScalar::new(1.0, 0.0),
            ComplexScalar::new(0.0, 0.0),
            ComplexScalar::new(0.0, 0.0),
            ComplexScalar::new(0.0, 0.0),
            ComplexScalar::new(0.0, 0.0),
        ]);

        let distribution =
            CosDistribution::from_law(&law).expect("synthetic uniform distribution should build");

        let quantile_log_returns = distribution
            .quantiles(probabilities)
            .expect("uniform quantiles should resolve");

        let grid = projected_cdf_quantile_band_grid(
            &distribution,
            node_count,
            probabilities,
            quantile_log_returns.as_slice(),
        )
        .expect("uniform distribution should produce a quantile-band grid");

        grid
    }
}
