//! Canonical NIG driver parameter schema and domain validation.
//!
//! This module centralizes NIG parameter constraints so simulation and pricing
//! use one validation path.

mod constants;
mod driver;
mod errors;
mod guardrails;
pub(crate) mod math;
mod parameters;
mod serde_impls;
mod validation;

pub use constants::{
    NIG_CHARACTERISTIC_EXPONENT_GUARDRAIL_DETERMINISM_POLICY,
    NIG_CHARACTERISTIC_EXPONENT_GUARDRAIL_REPORT_SCHEMA_VERSION,
    NIG_CHARACTERISTIC_EXPONENT_STABILITY_POLICY, NIG_INCREMENT_NEAR_ZERO_SCALE_POLICY,
    NIG_INCREMENT_SAMPLING_POLICY, NIG_PARAMETER_VALIDATION_DETERMINISM_POLICY,
    NIG_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION, NIG_SHARED_PLUS_SHIFT_CONFIGURATION_POLICY,
    NIG_TRADING_TIME_DRIVER_INTERFACE_POLICY, NIG_TRADING_TIME_INCREMENT_COMPOSITION_POLICY,
    NIG_TRADING_TIME_RETURN_SIGN_CONVENTION_POLICY,
};
pub use driver::{
    NigBuySellDriverParameters, NigSharedPlusShiftCharacteristicExponentSensitivity,
    NigSharedPlusShiftConfiguration, NigTradingTimeCharacteristicExponentContributions,
    NigTradingTimeIncrementContributions,
};
pub use errors::{
    NigIncrementSamplingError, NigParameterError, NigSharedPlusShiftConfigurationError,
    NigTradingTimeIncrementSamplingError,
};
pub use guardrails::{
    NigCharacteristicExponentEvaluationReport, NigCharacteristicExponentGuardrailCode,
    NigCharacteristicExponentGuardrailEvent, NigNumericalGuardrailSeverity,
    NigNumericalGuardrailStatus,
};
pub use parameters::NigParameters;
pub use validation::{
    NigParameterConstraint, NigParameterValidationReport, NigParameterViolation,
    NigValidationStatus,
};

#[cfg(test)]
mod tests;
