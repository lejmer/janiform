use nalgebra::{DMatrix, DVector, SMatrix, SVector};

/// Computes a damped least-squares step for a residual linearization.
///
/// Given residual vector `r` and sensitivity matrix `J`, this solves
/// `(J'J + lambda I) step = -J'r`, where `lambda` is
/// `damping_mean_diagonal_multiplier * mean(diag(J'J))`.
pub fn mean_diagonal_damped_least_squares_step(
    sensitivity_matrix: &[Vec<f64>],
    residual_vector: &[f64],
    damping_mean_diagonal_multiplier: f64,
) -> Option<Vec<f64>> {
    if !damping_mean_diagonal_multiplier.is_finite() || damping_mean_diagonal_multiplier < 0.0 {
        return None;
    }

    let parameter_count = sensitivity_matrix.first()?.len();
    if parameter_count == 0 || residual_vector.len() != sensitivity_matrix.len() {
        return None;
    }

    let observation_count = residual_vector.len();

    let mut jacobian = DMatrix::<f64>::zeros(observation_count, parameter_count);
    let mut residuals = DVector::<f64>::zeros(observation_count);

    for (row_position, (row, residual)) in sensitivity_matrix
        .iter()
        .zip(residual_vector.iter())
        .enumerate()
    {
        if row.len() != parameter_count || !residual.is_finite() {
            return None;
        }

        residuals[row_position] = *residual;

        for (column_position, sensitivity) in row.iter().enumerate() {
            if !sensitivity.is_finite() {
                return None;
            }

            jacobian[(row_position, column_position)] = *sensitivity;
        }
    }

    let mut normal_matrix = jacobian.transpose() * &jacobian;

    let diagonal_mean = mean_matrix_diagonal(&normal_matrix);
    if !diagonal_mean.is_finite() || diagonal_mean <= 0.0 {
        return None;
    }

    let damping = damping_mean_diagonal_multiplier * diagonal_mean;
    for diagonal_position in 0..parameter_count {
        normal_matrix[(diagonal_position, diagonal_position)] += damping;
    }

    let normal_rhs = -(jacobian.transpose() * residuals);
    let solution = normal_matrix.cholesky()?.solve(&normal_rhs);

    solution
        .iter()
        .all(|value| value.is_finite())
        .then(|| solution.iter().copied().collect())
}

/// Computes a damped least-squares step for a four-parameter residual linearization.
pub fn mean_diagonal_damped_least_squares_step_4(
    sensitivity_matrix: &[[f64; 4]],
    residual_vector: &[f64],
    damping_mean_diagonal_multiplier: f64,
) -> Option<[f64; 4]> {
    if !damping_mean_diagonal_multiplier.is_finite()
        || damping_mean_diagonal_multiplier < 0.0
        || residual_vector.len() != sensitivity_matrix.len()
    {
        return None;
    }

    let mut normal_matrix = SMatrix::<f64, 4, 4>::zeros();
    let mut normal_rhs = SVector::<f64, 4>::zeros();

    for (row, residual) in sensitivity_matrix.iter().zip(residual_vector.iter()) {
        if !residual.is_finite() || row.iter().any(|value| !value.is_finite()) {
            return None;
        }

        for left_position in 0..4 {
            normal_rhs[left_position] -= row[left_position] * residual;

            for right_position in 0..4 {
                normal_matrix[(left_position, right_position)] +=
                    row[left_position] * row[right_position];
            }
        }
    }

    let diagonal_mean = (0..4)
        .map(|position| normal_matrix[(position, position)])
        .sum::<f64>()
        / 4.0;

    if !diagonal_mean.is_finite() || diagonal_mean <= 0.0 {
        return None;
    }

    let damping = damping_mean_diagonal_multiplier * diagonal_mean;
    for diagonal_position in 0..4 {
        normal_matrix[(diagonal_position, diagonal_position)] += damping;
    }

    let solution = normal_matrix.cholesky()?.solve(&normal_rhs);

    solution.iter().all(|value| value.is_finite()).then_some([
        solution[0],
        solution[1],
        solution[2],
        solution[3],
    ])
}

fn mean_matrix_diagonal(matrix: &DMatrix<f64>) -> f64 {
    let diagonal_sum = (0..matrix.nrows())
        .map(|position| matrix[(position, position)])
        .sum::<f64>();

    diagonal_sum / matrix.nrows().max(1) as f64
}

#[cfg(test)]
mod tests {
    use super::{
        mean_diagonal_damped_least_squares_step, mean_diagonal_damped_least_squares_step_4,
    };

    #[test]
    fn damped_least_squares_step_reduces_weighted_residual_norm() {
        let sensitivity_matrix = vec![vec![1.0, 0.0], vec![0.0, 1.0], vec![1.0, 1.0]];
        let residual_vector = vec![2.0, -1.0, 0.5];

        let step = mean_diagonal_damped_least_squares_step(
            sensitivity_matrix.as_slice(),
            residual_vector.as_slice(),
            1.0,
        )
        .expect("damped normal equation should solve");

        let baseline_norm = squared_residual_norm(
            sensitivity_matrix.as_slice(),
            residual_vector.as_slice(),
            &[0.0, 0.0],
        );

        let stepped_norm = squared_residual_norm(
            sensitivity_matrix.as_slice(),
            residual_vector.as_slice(),
            &step,
        );

        assert!(stepped_norm < baseline_norm);
    }

    #[test]
    fn fixed_four_parameter_step_reduces_weighted_residual_norm() {
        let sensitivity_matrix = vec![
            [1.0, 0.0, 0.5, 0.0],
            [0.0, 1.0, 0.0, -0.25],
            [1.0, 1.0, 0.0, 0.5],
            [0.0, -0.5, 1.0, 1.0],
        ];
        let residual_vector = vec![2.0, -1.0, 0.5, 1.25];

        let step = mean_diagonal_damped_least_squares_step_4(
            sensitivity_matrix.as_slice(),
            residual_vector.as_slice(),
            1.0,
        )
        .expect("damped normal equation should solve");

        let baseline_norm = squared_residual_norm_4(
            sensitivity_matrix.as_slice(),
            residual_vector.as_slice(),
            &[0.0; 4],
        );

        let stepped_norm = squared_residual_norm_4(
            sensitivity_matrix.as_slice(),
            residual_vector.as_slice(),
            &step,
        );

        assert!(stepped_norm < baseline_norm);
    }

    fn squared_residual_norm(
        sensitivity_matrix: &[Vec<f64>],
        residual_vector: &[f64],
        step: &[f64],
    ) -> f64 {
        sensitivity_matrix
            .iter()
            .zip(residual_vector.iter())
            .map(|(row, residual)| {
                let stepped_residual = residual
                    + row
                        .iter()
                        .zip(step.iter())
                        .map(|(sensitivity, component)| sensitivity * component)
                        .sum::<f64>();

                stepped_residual * stepped_residual
            })
            .sum()
    }

    fn squared_residual_norm_4(
        sensitivity_matrix: &[[f64; 4]],
        residual_vector: &[f64],
        step: &[f64; 4],
    ) -> f64 {
        sensitivity_matrix
            .iter()
            .zip(residual_vector.iter())
            .map(|(row, residual)| {
                let stepped_residual = residual
                    + row
                        .iter()
                        .zip(step.iter())
                        .map(|(sensitivity, component)| sensitivity * component)
                        .sum::<f64>();

                stepped_residual * stepped_residual
            })
            .sum()
    }
}
