//! Shared linear algebra helpers.

mod least_squares;
mod two_by_two;

pub use least_squares::{
    RankAwareLeastSquaresSolution, mean_diagonal_damped_least_squares_step,
    mean_diagonal_damped_least_squares_step_4, non_negative_least_squares_2,
    rank_aware_one_column_least_squares, rank_aware_two_column_least_squares,
    rank_aware_two_column_unit_interval_second_coefficient_least_squares,
    rank_aware_unit_interval_gain_least_squares,
    weighted_ridge_least_squares_from_normal_equations,
};
pub use two_by_two::solve_two_by_two_linear_source_system;
