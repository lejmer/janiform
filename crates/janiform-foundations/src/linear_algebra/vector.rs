//! Shared vector linear algebra helpers.

/// Compute the Euclidean dot product of two equally sized vectors.
///
/// Returns `None` when the input slices have different lengths.
pub fn dot_product(left: &[f64], right: &[f64]) -> Option<f64> {
    if left.len() != right.len() {
        return None;
    }

    Some(
        left.iter()
            .zip(right.iter())
            .map(|(left_value, right_value)| left_value * right_value)
            .sum(),
    )
}

/// Scale a vector to fit inside a Euclidean radius.
///
/// Vectors already inside the radius are returned unchanged.
pub fn scale_vector_to_euclidean_radius(vector: &[f64], radius: f64) -> Option<Vec<f64>> {
    if !radius.is_finite() || radius <= 0.0 {
        return None;
    }

    let vector_norm = dot_product(vector, vector)?.sqrt();
    if !vector_norm.is_finite() {
        return None;
    }

    if vector_norm == 0.0 || vector_norm <= radius {
        return Some(vector.to_vec());
    }

    let scale = radius / vector_norm;

    Some(vector.iter().map(|component| component * scale).collect())
}

#[cfg(test)]
mod tests {
    use super::{dot_product, scale_vector_to_euclidean_radius};

    #[test]
    fn dot_product_computes_componentwise_sum_for_matching_vectors() {
        assert_eq!(dot_product(&[1.0, 2.0, 3.0], &[4.0, -5.0, 6.0]), Some(12.0));
    }

    #[test]
    fn dot_product_rejects_mismatched_dimensions() {
        assert_eq!(dot_product(&[1.0, 2.0], &[3.0]), None);
    }

    #[test]
    fn scale_vector_to_euclidean_radius_leaves_inside_vector_unchanged() {
        assert_eq!(
            scale_vector_to_euclidean_radius(&[3.0, 4.0], 5.0),
            Some(vec![3.0, 4.0])
        );
    }

    #[test]
    fn scale_vector_to_euclidean_radius_projects_outside_vector_to_boundary() {
        assert_eq!(
            scale_vector_to_euclidean_radius(&[3.0, 4.0], 2.5),
            Some(vec![1.5, 2.0])
        );
    }
}
