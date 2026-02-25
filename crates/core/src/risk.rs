//! Risk-sampling adapters that delegate to the shared affine oracle solver.

use crate::affine_oracle::{
    ExponentialAffineTransformInput, ExponentialAffineTransformIntegrationResult,
    ExponentialAffineTransformSolverError, ExponentialAffineTransformSolverSettings,
    solve_exponential_affine_transform_with_optional_settings,
};

/// Solve one-interval exponential-affine transform for sufficient-statistics risk sampling.
///
/// This adapter stays conversion-focused and reuses the same shared solver entrypoint as pricing.
pub fn solve_exponential_affine_transform_for_risk_sampling(
    transform_input: ExponentialAffineTransformInput,
) -> Result<ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformSolverError> {
    solve_exponential_affine_transform_with_optional_settings(transform_input, None)
}

/// Solve one-interval exponential-affine transform for risk sampling with explicit settings.
pub fn solve_exponential_affine_transform_for_risk_sampling_with_settings(
    transform_input: ExponentialAffineTransformInput,
    solver_settings: ExponentialAffineTransformSolverSettings,
) -> Result<ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformSolverError> {
    solve_exponential_affine_transform_with_optional_settings(
        transform_input,
        Some(solver_settings),
    )
}
