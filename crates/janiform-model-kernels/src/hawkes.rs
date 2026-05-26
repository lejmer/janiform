//! Canonical bivariate Hawkes state and kernel parameter types.
//!
//! The de-seasonalized buy and sell intensities are:
//! `tilde_lambda_plus = lambda_bar + Y++ + Y+-`
//! `tilde_lambda_minus = lambda_bar + Y-+ + Y--`
//!
//! The sign mapping is:
//! - `plus` means buy (`+`)
//! - `minus` means sell (`-`)
//!
//! For component notation `X_ij`:
//! - first sign `i` is target intensity (`+` buy, `-` sell)
//! - second sign `j` is source event sign (`+` buy, `-` sell)
//!
//! This module uses explicit named fields for all 4 components to avoid
//! tuple or matrix-index ambiguity in public core interfaces.

mod analytics;
mod loadings;
mod parameters;
mod state;

pub use analytics::{
    HawkesSpectralRadiusStabilityCheck, HawkesStationaryIntensityMultipliers,
    baseline_intensity_for_stationary_total_intensity, spectral_radius_alpha_over_beta,
    spectral_radius_stability_check, stationary_intensity_multipliers,
};
pub use loadings::{
    HawkesKernelLoadings, HawkesStructuredAlphaParameterError,
    HawkesStructuredAlphaParameterization, HawkesStructuredRhoParameterError,
    HawkesStructuredRhoParameterization, HawkesStructuredRhoSelfDominancePolicy,
    HawkesStructuredRhoValidationOutput,
};
pub use parameters::{
    HAWKES_DEFAULT_STABILITY_NUMERIC_TOLERANCE,
    HAWKES_KERNEL_PARAMETER_VALIDATION_DETERMINISM_POLICY,
    HAWKES_KERNEL_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION, HAWKES_STRICT_STATIONARITY_THRESHOLD,
    HawkesKernelParameterCheck, HawkesKernelParameterCheckCategory,
    HawkesKernelParameterConstraint, HawkesKernelParameterError,
    HawkesKernelParameterValidationReport, HawkesKernelParameterValidationSummary,
    HawkesKernelParameterViolation, HawkesKernelParameters, HawkesKernelValidationStatus,
};
pub use state::{
    HawkesEventSide, HawkesEventSidePayloadError, HawkesExcitationState, HawkesIntensityProjection,
    HawkesNewsSeverityMarkError, HawkesSeasonalityIntervalError, HawkesSideIntensities,
    HawkesSourceComponents, HawkesTransitionError, HawkesTransitionStep, evolve_hawkes_state,
};

#[cfg(test)]
mod tests;
