//! Exponential-affine one-interval transform equation models and RHS evaluation.
//!
//! Equation reference from `design/janiform_compact_model_spec.tex` Section 8:
//! - `F(u_+, u_-, z_+, z_- | Y_0) = exp(C(Delta) + D(Delta) · y_0)`
//! - `y_0 = [Y++, Y+-, Y-+, Y--]`
//! - `k_+ = [alpha_++, 0, alpha_-+, 0]`
//! - `k_- = [0, alpha_+-, 0, alpha_--]`
//! - `r = [rho_++, rho_+-, rho_-+, rho_--]`
//!
//! Mapping policy:
//! - All 4D components use explicit named fields in model order `[++, +-, -+, --]`
//! - `delta` is the interval length `Delta`
//! - `c_at_delta` stores `C(Delta)`
//! - `d_at_delta` stores `D(Delta)`
//! - `transform_value` stores `F`

mod constants;
mod contracts;
mod rhs;
mod runge_kutta;
mod schedule;
mod sensitivities;
mod serde_impls;
mod solver;

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod tests;

pub use constants::{
    EXPONENTIAL_AFFINE_TRANSFORM_BLOWUP_TERMINATION_POLICY,
    EXPONENTIAL_AFFINE_TRANSFORM_DEFAULT_SOLVER_POLICY,
    EXPONENTIAL_AFFINE_TRANSFORM_DOMAIN_GUARD_POLICY,
    EXPONENTIAL_AFFINE_TRANSFORM_EQUATION_MAPPING_POLICY,
    EXPONENTIAL_AFFINE_TRANSFORM_GUARD_FAILURE_SCHEMA_VERSION,
    EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_METADATA_SCHEMA_VERSION,
    EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_OUTPUT_SCHEMA_METADATA_VERSION,
    EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_GUARD_FAILURE_FIELDS,
    EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_METADATA_FIELDS,
    EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_RESULT_FIELDS,
    EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_TELEMETRY_FIELDS,
    EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_RESULT_SCHEMA_VERSION,
    EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_TELEMETRY_SCHEMA_VERSION,
    EXPONENTIAL_AFFINE_TRANSFORM_NEWS_INTENSITY_USAGE_POLICY,
};
pub use contracts::{
    ExponentialAffineTransformActivityLoadings, ExponentialAffineTransformActivityLoadingsError,
    ExponentialAffineTransformDVector, ExponentialAffineTransformDVectorError,
    ExponentialAffineTransformEquationSpecification,
    ExponentialAffineTransformEquationSpecificationError, ExponentialAffineTransformInput,
    ExponentialAffineTransformInputError, ExponentialAffineTransformNewsIntensityInput,
    ExponentialAffineTransformNewsIntensityInputError, ExponentialAffineTransformOutput,
    ExponentialAffineTransformOutputError,
};
pub use rhs::{
    ExponentialAffineTransformHamiltonians, ExponentialAffineTransformOdeRhs,
    evaluate_exponential_affine_transform_hamiltonians,
    evaluate_exponential_affine_transform_ode_rhs,
};
pub use schedule::{
    ExponentialAffineScheduleCumulantError, ExponentialAffineScheduleCumulantOutput,
    ExponentialAffineScheduleCumulantProgress, ExponentialAffineScheduleCumulantRequest,
    ExponentialAffineScheduleIntervalInput, ExponentialAffineTransformInputTangent,
    ExponentialAffineTransformInputTangentError, solve_exponential_affine_schedule_cumulant,
    solve_exponential_affine_schedule_cumulant_with_progress,
};
pub use sensitivities::{
    ExponentialAffineTransformInputSensitivityError,
    ExponentialAffineTransformInputSensitivityOutput, ExponentialAffineTransformOutputSensitivity,
    solve_exponential_affine_transform_input_sensitivities,
    solve_exponential_affine_transform_input_sensitivities_with_settings,
};
pub use solver::{
    EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_OUTPUT_SCHEMA_METADATA,
    ExponentialAffineTransformGuardFailure, ExponentialAffineTransformGuardFailureCode,
    ExponentialAffineTransformIntegrationMetadata,
    ExponentialAffineTransformIntegrationOutputSchemaMetadata,
    ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformIntegrationStatus,
    ExponentialAffineTransformIntegrationStatusFlags,
    ExponentialAffineTransformIntegrationTelemetry, ExponentialAffineTransformIntegratorMethod,
    ExponentialAffineTransformSolverError, ExponentialAffineTransformSolverSettings,
    ExponentialAffineTransformSolverSettingsError, ExponentialAffineTransformTelemetryMode,
    solve_exponential_affine_transform, solve_exponential_affine_transform_with_optional_settings,
    solve_exponential_affine_transform_with_settings,
    solve_exponential_affine_transform_with_terminal_loading,
};
