use nalgebra::{DMatrix, DVector, SymmetricEigen};

/// Result of evaluating a residual vector in a symmetric positive
/// semidefinite covariance geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct WhitenedQuadraticForm {
    /// Squared norm in the identified covariance subspace.
    pub quadratic_form: f64,
    /// Number of covariance eigen-directions retained by the pseudoinverse.
    pub rank: usize,
    /// Residual vector after pseudoinverse square-root covariance normalization.
    pub whitened_coordinates: Vec<f64>,
}

/// Evaluate `r' Sigma^+ r` for a symmetric positive semidefinite covariance.
///
/// The pseudoinverse rank is determined from the covariance spectral scale and
/// floating-point precision. Eigenvalues at or below the backward-error scale
/// of the decomposition are treated as unidentified directions.
pub fn symmetric_psd_whitened_quadratic_form(
    covariance_rows: &[Vec<f64>],
    residual_vector: &[f64],
) -> Option<WhitenedQuadraticForm> {
    let dimension = residual_vector.len();
    if dimension == 0 || covariance_rows.len() != dimension {
        return None;
    }

    let mut covariance_matrix = DMatrix::<f64>::zeros(dimension, dimension);

    for (row_position, row) in covariance_rows.iter().enumerate() {
        if row.len() != dimension {
            return None;
        }

        for (column_position, value) in row.iter().enumerate() {
            if !value.is_finite() {
                return None;
            }

            covariance_matrix[(row_position, column_position)] = *value;
        }
    }

    let mut residual = DVector::<f64>::zeros(dimension);

    for (position, value) in residual_vector.iter().enumerate() {
        if !value.is_finite() {
            return None;
        }

        residual[position] = *value;
    }

    let symmetric_covariance = 0.5 * (&covariance_matrix + covariance_matrix.transpose());
    let eigen = SymmetricEigen::new(symmetric_covariance);

    let spectral_scale = eigen
        .eigenvalues
        .iter()
        .copied()
        .map(f64::abs)
        .fold(0.0_f64, f64::max);

    if !spectral_scale.is_finite() || spectral_scale == 0.0 {
        return Some(WhitenedQuadraticForm {
            quadratic_form: 0.0,
            rank: 0,
            whitened_coordinates: Vec::new(),
        });
    }

    let eigenvalue_floor = spectral_scale * dimension as f64 * f64::EPSILON;
    let rotated_residual = eigen.eigenvectors.transpose() * residual;

    let mut quadratic_form = 0.0;
    let mut whitened_residual = DVector::<f64>::zeros(dimension);

    for (eigen_position, (eigenvalue, coordinate)) in eigen
        .eigenvalues
        .iter()
        .zip(rotated_residual.iter())
        .enumerate()
    {
        if *eigenvalue > eigenvalue_floor {
            let whitened_coordinate = coordinate / eigenvalue.sqrt();
            if !whitened_coordinate.is_finite() {
                return None;
            }

            quadratic_form += whitened_coordinate * whitened_coordinate;

            for row_position in 0..dimension {
                whitened_residual[row_position] +=
                    eigen.eigenvectors[(row_position, eigen_position)] * whitened_coordinate;
            }
        } else if *eigenvalue < -eigenvalue_floor {
            return None;
        }
    }

    let whitened_coordinates = whitened_residual.iter().copied().collect::<Vec<_>>();

    quadratic_form.is_finite().then_some(WhitenedQuadraticForm {
        quadratic_form,
        rank: eigen
            .eigenvalues
            .iter()
            .filter(|eigenvalue| **eigenvalue > eigenvalue_floor)
            .count(),
        whitened_coordinates,
    })
}

#[cfg(test)]
mod tests {
    use super::symmetric_psd_whitened_quadratic_form;

    #[test]
    fn whitened_quadratic_form_uses_identified_covariance_directions() {
        let covariance = vec![vec![4.0, 0.0], vec![0.0, 9.0]];
        let residual = vec![2.0, 3.0];

        let form = symmetric_psd_whitened_quadratic_form(&covariance, &residual)
            .expect("positive diagonal covariance should be identified");

        assert_eq!(form.quadratic_form, 2.0);
        assert_eq!(form.rank, 2);
        assert_eq!(form.whitened_coordinates, vec![1.0, 1.0]);
    }

    #[test]
    fn whitened_quadratic_form_omits_singular_directions() {
        let covariance = vec![vec![1.0, 1.0], vec![1.0, 1.0]];
        let residual = vec![1.0, 1.0];

        let form = symmetric_psd_whitened_quadratic_form(&covariance, &residual)
            .expect("positive semidefinite covariance should be accepted");

        assert_eq!(form.rank, 1);
        assert!((form.quadratic_form - 1.0).abs() <= 8.0 * f64::EPSILON);
        assert!(
            (form
                .whitened_coordinates
                .iter()
                .map(|coordinate| coordinate * coordinate)
                .sum::<f64>()
                - form.quadratic_form)
                .abs()
                <= 8.0 * f64::EPSILON
        );
    }
}
