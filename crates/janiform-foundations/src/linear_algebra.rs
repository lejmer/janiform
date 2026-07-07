//! Shared linear algebra helpers.

mod least_squares;
mod psd;
mod two_by_two;
mod vector;

pub use least_squares::{
    RankAwareLeastSquaresSolution, least_squares_cauchy_step, least_squares_dogleg_step,
    mean_diagonal_damped_least_squares_step, mean_diagonal_damped_least_squares_step_4,
    minimum_norm_least_squares, non_negative_least_squares_2, rank_aware_one_column_least_squares,
    rank_aware_two_column_least_squares,
    rank_aware_two_column_unit_interval_second_coefficient_least_squares,
    rank_aware_unit_interval_gain_least_squares,
    weighted_ridge_least_squares_from_normal_equations,
};
pub use psd::{WhitenedQuadraticForm, symmetric_psd_whitened_quadratic_form};
pub use two_by_two::solve_two_by_two_linear_source_system;
pub use vector::{dot_product, scale_vector_to_euclidean_radius};
