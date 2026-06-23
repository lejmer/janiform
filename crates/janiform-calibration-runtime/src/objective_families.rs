mod contracts;
mod covariance;
mod evaluation;

pub(crate) use contracts::{
    ObjectiveCoordinateInput, ObjectiveFamilyEvaluation, ObjectiveFamilyInput,
};
pub(crate) use covariance::{
    block_diagonal_covariance, covariance_from_delete_block_vectors, diagonal_covariance,
    mean_vector, model_mean_covariance, share_contrast_covariance, share_contrast_vector,
    sum_covariance_matrices,
};
pub(crate) use evaluation::evaluate_objective_family;
