//! Shared linear algebra helpers.

mod least_squares;
mod two_by_two;

pub use least_squares::{
    mean_diagonal_damped_least_squares_step, mean_diagonal_damped_least_squares_step_4,
    non_negative_least_squares_2, weighted_ridge_least_squares_from_normal_equations,
};
pub use two_by_two::solve_two_by_two_linear_source_system;
