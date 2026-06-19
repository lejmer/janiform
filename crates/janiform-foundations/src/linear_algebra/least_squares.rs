use nalgebra::{DMatrix, DVector, Matrix2, SMatrix, SVector, Vector2};

use crate::numeric::clamp_to_unit_interval;

/// Coefficients and algebraic rank for a one- or two-column least-squares fit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RankAwareLeastSquaresSolution {
    /// Coefficient for the first design column.
    pub first_coefficient: f64,
    /// Coefficient for the second design column, or zero for one-column fits.
    pub second_coefficient: f64,
    /// Algebraic rank identified from exact zero and dependence checks.
    pub rank: usize,
}

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

/// Solve a rank-aware one-column ordinary least-squares fit.
pub fn rank_aware_one_column_least_squares(
    column: &[f64],
    response: &[f64],
) -> Option<RankAwareLeastSquaresSolution> {
    if !rank_aware_one_column_input_is_valid(column, response) {
        return None;
    }

    let self_inner_product = column.iter().map(|value| value * value).sum::<f64>();
    if self_inner_product == 0.0 {
        return Some(RankAwareLeastSquaresSolution {
            first_coefficient: 0.0,
            second_coefficient: 0.0,
            rank: 0,
        });
    }

    let response_inner_product = column
        .iter()
        .zip(response.iter())
        .map(|(column_value, response_value)| column_value * response_value)
        .sum::<f64>();

    Some(RankAwareLeastSquaresSolution {
        first_coefficient: response_inner_product / self_inner_product,
        second_coefficient: 0.0,
        rank: 1,
    })
}

/// Solve a rank-aware one-column least-squares gain constrained to `[0, 1]`.
pub fn rank_aware_unit_interval_gain_least_squares(
    column: &[f64],
    response: &[f64],
) -> Option<RankAwareLeastSquaresSolution> {
    let unconstrained_solution = rank_aware_one_column_least_squares(column, response)?;

    Some(RankAwareLeastSquaresSolution {
        first_coefficient: clamp_to_unit_interval(unconstrained_solution.first_coefficient),
        second_coefficient: unconstrained_solution.second_coefficient,
        rank: unconstrained_solution.rank,
    })
}

/// Solve a rank-aware two-column ordinary least-squares fit.
pub fn rank_aware_two_column_least_squares(
    first_column: &[f64],
    second_column: &[f64],
    response: &[f64],
) -> Option<RankAwareLeastSquaresSolution> {
    if !rank_aware_two_column_input_is_valid(first_column, second_column, response) {
        return None;
    }

    let first_self_inner_product = first_column.iter().map(|value| value * value).sum::<f64>();
    let second_self_inner_product = second_column.iter().map(|value| value * value).sum::<f64>();

    let cross_inner_product = first_column
        .iter()
        .zip(second_column.iter())
        .map(|(first_value, second_value)| first_value * second_value)
        .sum::<f64>();

    let first_response_inner_product = first_column
        .iter()
        .zip(response.iter())
        .map(|(first_value, response_value)| first_value * response_value)
        .sum::<f64>();

    let second_response_inner_product = second_column
        .iter()
        .zip(response.iter())
        .map(|(second_value, response_value)| second_value * response_value)
        .sum::<f64>();

    let first_column_is_zero = first_self_inner_product == 0.0;
    let second_column_is_zero = second_self_inner_product == 0.0;

    if first_column_is_zero && second_column_is_zero {
        return Some(RankAwareLeastSquaresSolution {
            first_coefficient: 0.0,
            second_coefficient: 0.0,
            rank: 0,
        });
    }

    if first_column_is_zero {
        return Some(RankAwareLeastSquaresSolution {
            first_coefficient: 0.0,
            second_coefficient: second_response_inner_product / second_self_inner_product,
            rank: 1,
        });
    }

    if second_column_is_zero {
        return Some(RankAwareLeastSquaresSolution {
            first_coefficient: first_response_inner_product / first_self_inner_product,
            second_coefficient: 0.0,
            rank: 1,
        });
    }

    let normal_matrix = Matrix2::new(
        first_self_inner_product,
        cross_inner_product,
        cross_inner_product,
        second_self_inner_product,
    );

    let response_vector = Vector2::new(first_response_inner_product, second_response_inner_product);
    if let Some(coefficients) = normal_matrix.lu().solve(&response_vector) {
        return Some(RankAwareLeastSquaresSolution {
            first_coefficient: coefficients.x,
            second_coefficient: coefficients.y,
            rank: 2,
        });
    }

    let dependence_ratio = cross_inner_product / first_self_inner_product;
    let effective_coefficient = first_response_inner_product / first_self_inner_product;
    let normalization = 1.0 + dependence_ratio * dependence_ratio;

    Some(RankAwareLeastSquaresSolution {
        first_coefficient: effective_coefficient / normalization,
        second_coefficient: effective_coefficient * dependence_ratio / normalization,
        rank: 1,
    })
}

/// Solve a rank-aware two-column fit with the second coefficient constrained to `[0, 1]`.
pub fn rank_aware_two_column_unit_interval_second_coefficient_least_squares(
    first_column: &[f64],
    second_column: &[f64],
    response: &[f64],
) -> Option<RankAwareLeastSquaresSolution> {
    let unconstrained_solution =
        rank_aware_two_column_least_squares(first_column, second_column, response)?;
    if (0.0..=1.0).contains(&unconstrained_solution.second_coefficient) {
        return Some(unconstrained_solution);
    }

    let lower_boundary_solution = rank_aware_two_column_least_squares_at_fixed_second_coefficient(
        first_column,
        second_column,
        response,
        0.0,
    )?;

    let upper_boundary_solution = rank_aware_two_column_least_squares_at_fixed_second_coefficient(
        first_column,
        second_column,
        response,
        1.0,
    )?;

    Some(
        if squared_residual_norm_for_two_column_fit(
            first_column,
            second_column,
            response,
            lower_boundary_solution.first_coefficient,
            lower_boundary_solution.second_coefficient,
        ) <= squared_residual_norm_for_two_column_fit(
            first_column,
            second_column,
            response,
            upper_boundary_solution.first_coefficient,
            upper_boundary_solution.second_coefficient,
        ) {
            lower_boundary_solution
        } else {
            upper_boundary_solution
        },
    )
}

fn rank_aware_one_column_input_is_valid(column: &[f64], response: &[f64]) -> bool {
    !column.is_empty()
        && column.len() == response.len()
        && column.iter().all(|value| value.is_finite())
        && response.iter().all(|value| value.is_finite())
}

fn rank_aware_two_column_input_is_valid(
    first_column: &[f64],
    second_column: &[f64],
    response: &[f64],
) -> bool {
    rank_aware_one_column_input_is_valid(first_column, response)
        && second_column.len() == response.len()
        && second_column.iter().all(|value| value.is_finite())
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

fn rank_aware_two_column_least_squares_at_fixed_second_coefficient(
    first_column: &[f64],
    second_column: &[f64],
    response: &[f64],
    second_coefficient: f64,
) -> Option<RankAwareLeastSquaresSolution> {
    let adjusted_response = response
        .iter()
        .zip(second_column.iter())
        .map(|(response_value, second_value)| response_value - second_coefficient * second_value)
        .collect::<Vec<_>>();

    let first_solution =
        rank_aware_one_column_least_squares(first_column, adjusted_response.as_slice())?;

    Some(RankAwareLeastSquaresSolution {
        first_coefficient: first_solution.first_coefficient,
        second_coefficient,
        rank: first_solution.rank,
    })
}

fn squared_residual_norm_for_two_column_fit(
    first_column: &[f64],
    second_column: &[f64],
    response: &[f64],
    first_coefficient: f64,
    second_coefficient: f64,
) -> f64 {
    response
        .iter()
        .zip(first_column.iter())
        .zip(second_column.iter())
        .map(|((response_value, first_value), second_value)| {
            let residual = response_value
                - first_coefficient * first_value
                - second_coefficient * second_value;

            residual * residual
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::{
        mean_diagonal_damped_least_squares_step, mean_diagonal_damped_least_squares_step_4,
        non_negative_least_squares_2, rank_aware_one_column_least_squares,
        rank_aware_two_column_least_squares,
        rank_aware_two_column_unit_interval_second_coefficient_least_squares,
        rank_aware_unit_interval_gain_least_squares, squared_residual_norm_for_two_column_fit,
        weighted_ridge_least_squares_from_normal_equations,
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

        let baseline_norm = squared_residual_norm(
            sensitivity_matrix.as_slice(),
            residual_vector.as_slice(),
            [0.0; 4].as_slice(),
        );

        let stepped_norm = squared_residual_norm(
            sensitivity_matrix.as_slice(),
            residual_vector.as_slice(),
            step.as_slice(),
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

        assert!(
            (solution[0] - 3.0).abs() <= 3.0 * f64::EPSILON,
            "first coefficient should solve the diagonal normal equation, found {}",
            solution[0]
        );
        assert!(
            (solution[1] - 2.0).abs() <= f64::EPSILON,
            "second coefficient should solve the diagonal normal equation, found {}",
            solution[1]
        );
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

    #[test]
    fn rank_aware_one_column_reports_zero_support() {
        let solution = rank_aware_one_column_least_squares(&[0.0, 0.0], &[1.0, 2.0])
            .expect("valid zero-support fit should solve");

        assert_eq!(solution.rank, 0);
        assert_eq!(solution.first_coefficient, 0.0);
    }

    #[test]
    fn rank_aware_unit_interval_gain_clamps_to_contract_bounds() {
        let solution = rank_aware_unit_interval_gain_least_squares(&[1.0, 1.0], &[2.0, 2.0])
            .expect("valid unit-interval gain fit should solve");

        assert_eq!(solution.rank, 1);
        assert_eq!(solution.first_coefficient, 1.0);
    }

    #[test]
    fn rank_aware_two_column_reports_dependent_columns() {
        let first_column = [1.0, 2.0, 3.0];
        let second_column = [2.0, 4.0, 6.0];
        let response = [3.0, 6.0, 9.0];

        let solution = rank_aware_two_column_least_squares(
            first_column.as_slice(),
            second_column.as_slice(),
            response.as_slice(),
        )
        .expect("valid dependent two-column fit should solve");

        assert_eq!(solution.rank, 1);
        assert!(
            squared_residual_norm_for_two_column_fit(
                first_column.as_slice(),
                second_column.as_slice(),
                response.as_slice(),
                solution.first_coefficient,
                solution.second_coefficient,
            ) <= f64::EPSILON
        );
    }

    #[test]
    fn rank_aware_two_column_second_coefficient_uses_unit_boundary() {
        let first_column = [0.0, 0.0];
        let second_column = [1.0, 1.0];
        let response = [2.0, 2.0];

        let solution = rank_aware_two_column_unit_interval_second_coefficient_least_squares(
            first_column.as_slice(),
            second_column.as_slice(),
            response.as_slice(),
        )
        .expect("valid bounded two-column fit should solve");

        assert_eq!(solution.rank, 0);
        assert_eq!(solution.second_coefficient, 1.0);
    }

    #[test]
    fn rank_aware_least_squares_rejects_unaligned_inputs() {
        assert_eq!(
            rank_aware_one_column_least_squares(&[1.0], &[1.0, 2.0]),
            None
        );
        assert_eq!(
            rank_aware_two_column_least_squares(&[1.0], &[1.0], &[1.0, 2.0]),
            None
        );
    }

    fn squared_residual_norm<Row>(
        sensitivity_matrix: &[Row],
        residual_vector: &[f64],
        step: &[f64],
    ) -> f64
    where
        Row: AsRef<[f64]>,
    {
        sensitivity_matrix
            .iter()
            .zip(residual_vector.iter())
            .map(|(row, residual)| {
                let stepped_residual = residual
                    + row
                        .as_ref()
                        .iter()
                        .zip(step.iter())
                        .map(|(sensitivity, component)| sensitivity * component)
                        .sum::<f64>();

                stepped_residual * stepped_residual
            })
            .sum()
    }
}
