use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_engine_pricing::{
    VanillaOptionPricingCalibrationHooks, VanillaOptionPricingCalibrationHooksError,
};

/// Canonical calibration observation distilled from pricing calibration hooks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OptionSurfaceCalibrationObservation {
    pub option_price: f64,
    pub log_moneyness: f64,
    pub forward_moneyness: f64,
    pub maturity_interval: f64,
    pub integration_numeric_quality_is_poor: Option<bool>,
    pub integration_convergence_check_passed: Option<bool>,
    pub integration_truncation_check_passed: Option<bool>,
}

/// Consume one pricing calibration hook payload into the calibration observation shape.
pub fn build_option_surface_calibration_observation_from_pricing_hooks(
    hooks: VanillaOptionPricingCalibrationHooks,
) -> Result<OptionSurfaceCalibrationObservation, OptionSurfaceCalibrationObservationError> {
    hooks.validate().map_err(|source| {
        OptionSurfaceCalibrationObservationError::InvalidPricingCalibrationHooks { source }
    })?;

    Ok(OptionSurfaceCalibrationObservation {
        option_price: hooks.option_price,
        log_moneyness: hooks.log_moneyness,
        forward_moneyness: hooks.forward_moneyness,
        maturity_interval: hooks.maturity_interval,
        integration_numeric_quality_is_poor: hooks.integration_numeric_quality_is_poor,
        integration_convergence_check_passed: hooks.integration_convergence_check_passed,
        integration_truncation_check_passed: hooks.integration_truncation_check_passed,
    })
}

/// Structured failure modes for pricing-hook calibration observation conversion.
#[derive(Debug, Clone, PartialEq)]
pub enum OptionSurfaceCalibrationObservationError {
    InvalidPricingCalibrationHooks {
        source: VanillaOptionPricingCalibrationHooksError,
    },
}

impl Display for OptionSurfaceCalibrationObservationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPricingCalibrationHooks { source } => {
                write!(
                    formatter,
                    "pricing calibration hooks payload is invalid for calibration consumption: {source}"
                )
            }
        }
    }
}

impl Error for OptionSurfaceCalibrationObservationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPricingCalibrationHooks { source } => Some(source),
        }
    }
}
