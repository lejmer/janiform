use janiform_foundations::numeric::arithmetic_mean_or_zero;

pub(crate) fn mean_vector(vectors: &[Vec<f64>]) -> Option<Vec<f64>> {
    let dimension = vectors.first()?.len();
    if dimension == 0 {
        return None;
    }

    let mut means = vec![0.0; dimension];

    for vector in vectors {
        if vector.len() != dimension || vector.iter().any(|value| !value.is_finite()) {
            return None;
        }

        for (mean, value) in means.iter_mut().zip(vector.iter()) {
            *mean += *value;
        }
    }

    for mean in &mut means {
        *mean /= vectors.len() as f64;
    }

    Some(means)
}

pub(crate) fn covariance_from_delete_block_vectors(
    full_sample_vector: &[f64],
    delete_block_vectors: &[Vec<f64>],
) -> Option<Vec<Vec<f64>>> {
    let block_count = delete_block_vectors.len();
    if block_count < 2 {
        return None;
    }

    let dimension = full_sample_vector.len();
    if dimension == 0 || full_sample_vector.iter().any(|value| !value.is_finite()) {
        return None;
    }

    let mut covariance = zero_covariance(dimension);

    let scale = (block_count - 1) as f64 / block_count as f64;

    for vector in delete_block_vectors {
        if vector.len() != dimension || vector.iter().any(|value| !value.is_finite()) {
            return None;
        }

        for row_position in 0..dimension {
            let left = vector[row_position] - full_sample_vector[row_position];

            for column_position in 0..dimension {
                let right = vector[column_position] - full_sample_vector[column_position];
                covariance[row_position][column_position] += scale * left * right;
            }
        }
    }

    covariance_is_finite(covariance.as_slice()).then_some(covariance)
}

pub(crate) fn covariance_from_centered_source_vectors(
    source_vectors: &[Vec<f64>],
) -> Option<Vec<Vec<f64>>> {
    let sample_count = source_vectors.len();
    if sample_count < 2 {
        return None;
    }

    let mean = mean_vector(source_vectors)?;
    let dimension = mean.len();

    let mut covariance = zero_covariance(dimension);

    for vector in source_vectors {
        for row_position in 0..dimension {
            let left = vector[row_position] - mean[row_position];

            for column_position in 0..dimension {
                let right = vector[column_position] - mean[column_position];
                covariance[row_position][column_position] += left * right;
            }
        }
    }

    let denominator = (sample_count - 1) as f64;
    for row in &mut covariance {
        for value in row {
            *value /= denominator;
        }
    }

    covariance_is_finite(covariance.as_slice()).then_some(covariance)
}

pub(crate) fn model_mean_covariance(model_vectors: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let mut covariance = covariance_from_centered_source_vectors(model_vectors)?;

    let scale = (model_vectors.len() as f64).recip();

    for row in &mut covariance {
        for value in row {
            *value *= scale;
        }
    }

    Some(covariance)
}

pub(crate) fn diagonal_covariance(variances: &[f64]) -> Option<Vec<Vec<f64>>> {
    if variances.is_empty()
        || variances
            .iter()
            .any(|variance| !variance.is_finite() || *variance < 0.0)
    {
        return None;
    }

    let mut covariance = zero_covariance(variances.len());

    for (position, variance) in variances.iter().enumerate() {
        covariance[position][position] = *variance;
    }

    Some(covariance)
}

pub(crate) fn block_diagonal_covariance(covariances: &[Vec<Vec<f64>>]) -> Option<Vec<Vec<f64>>> {
    if covariances.is_empty() {
        return None;
    }

    let total_dimension = covariances
        .iter()
        .map(|covariance| covariance.len())
        .try_fold(0usize, usize::checked_add)?;
    if total_dimension == 0 {
        return None;
    }

    let mut combined = zero_covariance(total_dimension);
    let mut row_offset = 0usize;

    for covariance in covariances {
        let dimension = covariance.len();
        if dimension == 0 || covariance.iter().any(|row| row.len() != dimension) {
            return None;
        }

        for row_position in 0..dimension {
            for column_position in 0..dimension {
                let value = covariance[row_position][column_position];
                if !value.is_finite() {
                    return None;
                }

                combined[row_offset + row_position][row_offset + column_position] = value;
            }
        }

        row_offset += dimension;
    }

    covariance_is_finite(combined.as_slice()).then_some(combined)
}

pub(crate) fn sum_covariance_matrices(covariances: &[Vec<Vec<f64>>]) -> Option<Vec<Vec<f64>>> {
    let dimension = covariances.first()?.len();
    if dimension == 0 {
        return None;
    }

    let mut sum = zero_covariance(dimension);

    for covariance in covariances {
        if covariance.len() != dimension || covariance.iter().any(|row| row.len() != dimension) {
            return None;
        }

        for row_position in 0..dimension {
            for column_position in 0..dimension {
                let value = covariance[row_position][column_position];
                if !value.is_finite() {
                    return None;
                }

                sum[row_position][column_position] += value;
            }
        }
    }

    covariance_is_finite(sum.as_slice()).then_some(sum)
}

pub(crate) fn share_contrast_vector(values: &[f64]) -> Option<Vec<f64>> {
    if values.len() < 2 || values.iter().any(|value| !value.is_finite()) {
        return None;
    }

    let mean = arithmetic_mean_or_zero(values);

    Some(values.iter().map(|value| value - mean).collect())
}

pub(crate) fn share_contrast_covariance(covariance: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let dimension = covariance.len();
    if dimension < 2
        || covariance
            .iter()
            .any(|row| row.len() != dimension || row.iter().any(|value| !value.is_finite()))
    {
        return None;
    }

    let inverse_dimension = (dimension as f64).recip();
    let row_means = covariance
        .iter()
        .map(|row| row.iter().sum::<f64>() * inverse_dimension)
        .collect::<Vec<_>>();

    let column_means = (0..dimension)
        .map(|column_position| {
            covariance
                .iter()
                .map(|row| row[column_position])
                .sum::<f64>()
                * inverse_dimension
        })
        .collect::<Vec<_>>();

    let grand_mean = row_means.iter().sum::<f64>() * inverse_dimension;
    let mut projected = zero_covariance(dimension);

    for row_position in 0..dimension {
        for column_position in 0..dimension {
            projected[row_position][column_position] = covariance[row_position][column_position]
                - row_means[row_position]
                - column_means[column_position]
                + grand_mean;
        }
    }

    covariance_is_finite(projected.as_slice()).then_some(projected)
}

fn zero_covariance(dimension: usize) -> Vec<Vec<f64>> {
    vec![vec![0.0; dimension]; dimension]
}

fn covariance_is_finite(covariance: &[Vec<f64>]) -> bool {
    covariance
        .iter()
        .all(|row| row.iter().all(|value| value.is_finite()))
}

#[cfg(test)]
mod tests {
    use super::{
        block_diagonal_covariance, covariance_from_delete_block_vectors, model_mean_covariance,
        share_contrast_covariance, share_contrast_vector,
    };

    #[test]
    fn delete_block_covariance_uses_jackknife_scaling() {
        let covariance =
            covariance_from_delete_block_vectors(&[2.0], &[vec![1.0], vec![2.0], vec![3.0]])
                .expect("delete-block vectors should produce covariance");

        assert_eq!(covariance, vec![vec![4.0 / 3.0]]);
    }

    #[test]
    fn model_mean_covariance_scales_seed_covariance_by_seed_count() {
        let covariance = model_mean_covariance(&[vec![1.0], vec![3.0]])
            .expect("seed vectors should produce covariance");

        assert_eq!(covariance, vec![vec![1.0]]);
    }

    #[test]
    fn block_diagonal_covariance_preserves_source_blocks() {
        let covariance =
            block_diagonal_covariance(&[vec![vec![1.0, 0.5], vec![0.5, 2.0]], vec![vec![3.0]]])
                .expect("valid covariance blocks should compose");

        assert_eq!(
            covariance,
            vec![
                vec![1.0, 0.5, 0.0],
                vec![0.5, 2.0, 0.0],
                vec![0.0, 0.0, 3.0],
            ]
        );
    }

    #[test]
    fn share_contrast_vector_removes_sum_direction() {
        let contrast =
            share_contrast_vector(&[0.2, 0.3, 0.5]).expect("share vector should produce contrast");

        assert!((contrast.iter().sum::<f64>()).abs() <= f64::EPSILON);
    }

    #[test]
    fn share_contrast_covariance_removes_sum_direction() {
        let covariance = share_contrast_covariance(&[
            vec![0.2, -0.1, -0.1],
            vec![-0.1, 0.2, -0.1],
            vec![-0.1, -0.1, 0.2],
        ])
        .expect("covariance should project");

        for row in covariance {
            assert!(row.iter().sum::<f64>().abs() <= f64::EPSILON);
        }
    }
}
