use crate::error::WorkflowError;

use super::numeric::usize_to_f64;

/// Project finite spectral CDF samples onto the probability-CDF contract.
///
/// The target set is the closed convex set of non-decreasing sequences inside
/// `[0, 1]`. The pool-adjacent-violators pass gives the least-squares
/// non-decreasing projection, and the final bound projection enforces the
/// probability interval required by forecast artifacts.
pub(super) fn isotonic_non_decreasing_projection(
    values: &[f64],
) -> Result<Vec<f64>, WorkflowError> {
    let mut blocks = Vec::with_capacity(values.len());

    for value in values {
        blocks.push(IsotonicBlock {
            average: *value,
            weight: 1,
        });

        while blocks.len() >= 2 {
            let right_position = blocks.len() - 1;
            let left_position = right_position - 1;

            if blocks[left_position].average <= blocks[right_position].average {
                break;
            }

            let left = blocks.remove(left_position);
            let right = blocks.remove(left_position);

            blocks.insert(left_position, left.merge(right)?);
        }
    }

    let mut projected = Vec::with_capacity(values.len());

    for block in blocks {
        projected.extend(std::iter::repeat_n(
            block.average.clamp(0.0, 1.0),
            block.weight,
        ));
    }

    Ok(projected)
}

#[derive(Clone, Copy, Debug)]
struct IsotonicBlock {
    average: f64,
    weight: usize,
}

impl IsotonicBlock {
    fn merge(self, other: Self) -> Result<Self, WorkflowError> {
        let weight = self.weight + other.weight;
        let self_weight = usize_to_f64(self.weight, "isotonic_block_weight")?;
        let other_weight = usize_to_f64(other.weight, "isotonic_block_weight")?;
        let merged_weight = usize_to_f64(weight, "isotonic_block_weight")?;
        let average = (self.average * self_weight + other.average * other_weight) / merged_weight;

        Ok(Self { average, weight })
    }
}
