use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_foundations::complex_math::ComplexScalar;
use serde::{Deserialize, Serialize};

/// Pricing-kernel loadings for vanilla option pricing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VanillaOptionPricingKernelParameters {
    /// Likelihood-ratio multiplier applied to buy-side endogenous event intensity.
    pub buy_activity_premium: f64,
    /// Likelihood-ratio multiplier applied to sell-side endogenous event intensity.
    pub sell_activity_premium: f64,
    /// Loading applied to future signed side-imbalance exposure in the pricing kernel.
    pub pressure_signed_imbalance_exposure_risk_price: f64,
}

impl VanillaOptionPricingKernelParameters {
    /// Return the physical-measure kernel with no activity premium.
    pub const fn identity() -> Self {
        Self {
            buy_activity_premium: 1.0,
            sell_activity_premium: 1.0,
            pressure_signed_imbalance_exposure_risk_price: 0.0,
        }
    }

    /// Construct pricing-kernel parameters without validation.
    pub const fn new(
        buy_activity_premium: f64,
        sell_activity_premium: f64,
        pressure_signed_imbalance_exposure_risk_price: f64,
    ) -> Self {
        Self {
            buy_activity_premium,
            sell_activity_premium,
            pressure_signed_imbalance_exposure_risk_price,
        }
    }

    /// Construct validated pricing-kernel parameters.
    pub fn try_new(
        buy_activity_premium: f64,
        sell_activity_premium: f64,
        pressure_signed_imbalance_exposure_risk_price: f64,
    ) -> Result<Self, VanillaOptionPricingKernelParametersError> {
        let parameters = Self::new(
            buy_activity_premium,
            sell_activity_premium,
            pressure_signed_imbalance_exposure_risk_price,
        );
        parameters.validate()?;

        Ok(parameters)
    }

    /// Validate pricing-kernel parameters.
    pub fn validate(self) -> Result<(), VanillaOptionPricingKernelParametersError> {
        validate_activity_premium("buy_activity_premium", self.buy_activity_premium)?;
        validate_activity_premium("sell_activity_premium", self.sell_activity_premium)?;
        validate_pressure_risk_price(
            "pressure_signed_imbalance_exposure_risk_price",
            self.pressure_signed_imbalance_exposure_risk_price,
        )?;

        Ok(())
    }

    pub(crate) fn buy_terminal_density_transform_inputs(
        self,
        u_input: ComplexScalar,
        z_input: ComplexScalar,
    ) -> PricingKernelAffineInputs {
        terminal_density_transform_inputs(self.buy_activity_premium, u_input, z_input)
    }

    pub(crate) fn sell_terminal_density_transform_inputs(
        self,
        u_input: ComplexScalar,
        z_input: ComplexScalar,
    ) -> PricingKernelAffineInputs {
        terminal_density_transform_inputs(self.sell_activity_premium, u_input, z_input)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PricingKernelAffineInputs {
    pub(crate) u_input: ComplexScalar,
    pub(crate) z_input: ComplexScalar,
    pub(crate) log_activity_premium_u_derivative: ComplexScalar,
    pub(crate) log_activity_premium_z_derivative: ComplexScalar,
}

fn terminal_density_transform_inputs(
    activity_premium: f64,
    u_input: ComplexScalar,
    z_input: ComplexScalar,
) -> PricingKernelAffineInputs {
    PricingKernelAffineInputs {
        u_input,
        z_input: z_input.scale(activity_premium),
        log_activity_premium_u_derivative: ComplexScalar::new(0.0, 0.0),
        log_activity_premium_z_derivative: z_input.scale(activity_premium),
    }
}

fn validate_activity_premium(
    field: &'static str,
    value: f64,
) -> Result<(), VanillaOptionPricingKernelParametersError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(
            VanillaOptionPricingKernelParametersError::InvalidActivityPremium { field, value },
        );
    }

    Ok(())
}

fn validate_pressure_risk_price(
    field: &'static str,
    value: f64,
) -> Result<(), VanillaOptionPricingKernelParametersError> {
    if !value.is_finite() {
        return Err(
            VanillaOptionPricingKernelParametersError::InvalidPressureSignedImbalanceExposureRiskPrice {
                field,
                value,
            },
        );
    }

    Ok(())
}

/// Validation errors for pricing-kernel parameters.
#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionPricingKernelParametersError {
    InvalidActivityPremium { field: &'static str, value: f64 },
    InvalidPressureSignedImbalanceExposureRiskPrice { field: &'static str, value: f64 },
}

impl Display for VanillaOptionPricingKernelParametersError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidActivityPremium { field, value } => write!(
                formatter,
                "{field} must be finite and strictly positive, found {value}"
            ),
            Self::InvalidPressureSignedImbalanceExposureRiskPrice { field, value } => {
                write!(formatter, "{field} must be finite, found {value}")
            }
        }
    }
}

impl Error for VanillaOptionPricingKernelParametersError {}

#[cfg(test)]
mod tests {
    use super::VanillaOptionPricingKernelParameters;
    use janiform_foundations::complex_math::ComplexScalar;

    #[test]
    fn terminal_density_activity_premium_tilts_counts_without_intensity_compensator() {
        let kernel = VanillaOptionPricingKernelParameters::try_new(1.25, 0.75, 0.0)
            .expect("pricing kernel parameters should construct");
        let u_input = ComplexScalar::new(0.2, -0.3);
        let z_input = ComplexScalar::new(0.8, 0.4);

        let transformed_inputs = kernel.buy_terminal_density_transform_inputs(u_input, z_input);

        assert_eq!(transformed_inputs.u_input, u_input);
        assert_eq!(transformed_inputs.z_input, ComplexScalar::new(1.0, 0.5));
        assert_eq!(
            transformed_inputs.log_activity_premium_u_derivative,
            ComplexScalar::new(0.0, 0.0)
        );
        assert_eq!(
            transformed_inputs.log_activity_premium_z_derivative,
            ComplexScalar::new(1.0, 0.5)
        );
    }

    #[test]
    fn pricing_kernel_rejects_non_positive_premia() {
        let error = VanillaOptionPricingKernelParameters::try_new(0.0, 1.0, 0.0)
            .expect_err("zero premium should be rejected");

        assert_eq!(
            error.to_string(),
            "buy_activity_premium must be finite and strictly positive, found 0"
        );
    }

    #[test]
    fn pricing_kernel_rejects_non_finite_pressure_risk_price() {
        let error = VanillaOptionPricingKernelParameters::try_new(1.0, 1.0, f64::NAN)
            .expect_err("non-finite pressure risk price should be rejected");

        assert_eq!(
            error.to_string(),
            "pressure_signed_imbalance_exposure_risk_price must be finite, found NaN"
        );
    }
}
