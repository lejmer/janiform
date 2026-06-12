use nalgebra::{DMatrix, DVector, Matrix2, SMatrix, SVector, Vector2};

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

/// Solve a two-coordinate least-squares fit with non-negative coefficients.
pub fn non_negative_least_squares_2(rows: &[[f64; 2]], response: &[f64]) -> Option<[f64; 2]> {
    if rows.len() != response.len() || rows.is_empty() {
        return None;
    }

    let mut normal_matrix = Matrix2::<f64>::zeros();
    let mut normal_rhs = Vector2::<f64>::zeros();

    for (row, current_response) in rows.iter().zip(response.iter()) {
        if !current_response.is_finite() || row.iter().any(|value| !value.is_finite()) {
            return None;
        }

        for left_position in 0..2 {
            normal_rhs[left_position] += row[left_position] * current_response;

            for right_position in 0..2 {
                normal_matrix[(left_position, right_position)] +=
                    row[left_position] * row[right_position];
            }
        }
    }

    let mut candidates = vec![[0.0, 0.0]];
    if let Some(unconstrained_solution) = normal_matrix.lu().solve(&normal_rhs)
        && unconstrained_solution[0] >= 0.0
        && unconstrained_solution[1] >= 0.0
        && unconstrained_solution[0].is_finite()
        && unconstrained_solution[1].is_finite()
    {
        candidates.push([unconstrained_solution[0], unconstrained_solution[1]]);
    }

    if let Some(first_coordinate_solution) = single_coordinate_solution(rows, response, 0) {
        candidates.push([first_coordinate_solution, 0.0]);
    }
    if let Some(second_coordinate_solution) = single_coordinate_solution(rows, response, 1) {
        candidates.push([0.0, second_coordinate_solution]);
    }

    candidates
        .into_iter()
        .min_by(|left_candidate, right_candidate| {
            squared_residual_norm_2(rows, response, left_candidate)
                .total_cmp(&squared_residual_norm_2(rows, response, right_candidate))
        })
}

/// Solve regularized normal equations for a weighted least-squares fit.
///
/// The caller owns construction of `X'WX` and `X'Wy`, which keeps hot paths
/// from materializing large design matrices when sufficient statistics are
/// already available.
pub fn weighted_ridge_least_squares_from_normal_equations(
    normal_matrix_rows: &[Vec<f64>],
    normal_rhs: &[f64],
    diagonal_penalties: &[f64],
) -> Option<Vec<f64>> {
    let parameter_count = normal_rhs.len();
    if parameter_count == 0
        || normal_matrix_rows.len() != parameter_count
        || diagonal_penalties.len() != parameter_count
    {
        return None;
    }

    let mut normal_matrix = DMatrix::<f64>::zeros(parameter_count, parameter_count);
    let mut rhs = DVector::<f64>::zeros(parameter_count);

    for (row_position, row) in normal_matrix_rows.iter().enumerate() {
        if row.len() != parameter_count {
            return None;
        }

        for (column_position, value) in row.iter().enumerate() {
            if !value.is_finite() {
                return None;
            }

            normal_matrix[(row_position, column_position)] = *value;
        }
    }

    for (position, (rhs_value, diagonal_penalty)) in
        normal_rhs.iter().zip(diagonal_penalties.iter()).enumerate()
    {
        if !rhs_value.is_finite() || !diagonal_penalty.is_finite() || *diagonal_penalty < 0.0 {
            return None;
        }

        normal_matrix[(position, position)] += *diagonal_penalty;
        rhs[position] = *rhs_value;
    }

    let solution = normal_matrix.cholesky()?.solve(&rhs);

    solution
        .iter()
        .all(|value| value.is_finite())
        .then(|| solution.iter().copied().collect())
}

fn mean_matrix_diagonal(matrix: &DMatrix<f64>) -> f64 {
    let diagonal_sum = (0..matrix.nrows())
        .map(|position| matrix[(position, position)])
        .sum::<f64>();

    diagonal_sum / matrix.nrows().max(1) as f64
}

fn single_coordinate_solution(
    rows: &[[f64; 2]],
    response: &[f64],
    coordinate_position: usize,
) -> Option<f64> {
    let mut denominator = 0.0;
    let mut numerator = 0.0;

    for (row, current_response) in rows.iter().zip(response.iter()) {
        denominator += row[coordinate_position] * row[coordinate_position];
        numerator += row[coordinate_position] * current_response;
    }

    if denominator <= 0.0 || !denominator.is_finite() || !numerator.is_finite() {
        return None;
    }

    Some((numerator / denominator).max(0.0))
}

fn squared_residual_norm_2(rows: &[[f64; 2]], response: &[f64], coefficients: &[f64; 2]) -> f64 {
    rows.iter()
        .zip(response.iter())
        .map(|(row, current_response)| {
            let residual = current_response
                - row
                    .iter()
                    .zip(coefficients.iter())
                    .map(|(sensitivity, coefficient)| sensitivity * coefficient)
                    .sum::<f64>();

            residual * residual
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::{
        mean_diagonal_damped_least_squares_step, mean_diagonal_damped_least_squares_step_4,
        non_negative_least_squares_2, weighted_ridge_least_squares_from_normal_equations,
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

    #[test]
    fn normal_equation_solver_solves_weighted_fit() {
        let normal_matrix_rows = vec![vec![2.0, 0.0], vec![0.0, 4.0]];
        let normal_rhs = vec![6.0, 8.0];
        let diagonal_penalties = vec![0.0, 0.0];

        let solution = weighted_ridge_least_squares_from_normal_equations(
            normal_matrix_rows.as_slice(),
            normal_rhs.as_slice(),
            diagonal_penalties.as_slice(),
        )
        .expect("positive definite normal equations should solve");

        assert_eq!(solution, vec![3.0, 2.0]);
    }

    #[test]
    fn non_negative_least_squares_keeps_positive_two_coordinate_solution() {
        let rows = [[1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
        let response = [2.0, 3.0, 5.0];

        let coefficients = non_negative_least_squares_2(rows.as_slice(), response.as_slice())
            .expect("constrained least-squares fit should solve");

        assert_eq!(coefficients, [2.0, 3.0]);
    }

    #[test]
    fn non_negative_least_squares_uses_boundary_when_unconstrained_fit_is_negative() {
        let rows = [[1.0, 0.0], [0.0, 1.0], [1.0, -1.0]];
        let response = [2.0, -1.0, 3.0];

        let coefficients = non_negative_least_squares_2(rows.as_slice(), response.as_slice())
            .expect("constrained least-squares fit should solve");

        assert_eq!(coefficients[0], 2.5);
        assert_eq!(coefficients[1], 0.0);
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
