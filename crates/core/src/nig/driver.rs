use serde::Serialize;

use crate::randomness::SimulationRandomnessStream;

use super::constants::{
    NIG_SHARED_PLUS_SHIFT_CONFIGURATION_POLICY, NIG_TRADING_TIME_INCREMENT_COMPOSITION_POLICY,
    NIG_TRADING_TIME_RETURN_SIGN_CONVENTION_POLICY,
};
use super::errors::{NigSharedPlusShiftConfigurationError, NigTradingTimeIncrementSamplingError};
use super::parameters::NigParameters;
use crate::complex_math::ComplexScalar;

/// Shared-plus-shift NIG driver configuration for buy and sell trading-time components.
///
/// Parameter mapping:
/// - buy driver uses `(alpha, beta_0, delta, mu)`
/// - sell driver uses `(alpha, beta_0 + delta_beta, delta, mu)`
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct NigSharedPlusShiftConfiguration {
    pub alpha: f64,
    pub beta_0: f64,
    pub delta: f64,
    pub mu: f64,
    pub delta_beta: f64,
}

impl NigSharedPlusShiftConfiguration {
    fn validate_delta_beta(delta_beta: f64) -> Result<(), NigSharedPlusShiftConfigurationError> {
        if !delta_beta.is_finite() || delta_beta >= 0.0 {
            return Err(
                NigSharedPlusShiftConfigurationError::DeltaBetaDomainViolation {
                    value: delta_beta,
                },
            );
        }

        Ok(())
    }

    /// Validate and build one canonical shared-plus-shift configuration payload.
    pub fn try_new(
        alpha: f64,
        beta_0: f64,
        delta: f64,
        mu: f64,
        delta_beta: f64,
    ) -> Result<Self, NigSharedPlusShiftConfigurationError> {
        Self::validate_delta_beta(delta_beta)?;

        let _buy_side_parameters =
            NigParameters::try_new(alpha, beta_0, delta, mu).map_err(|source| {
                NigSharedPlusShiftConfigurationError::BuySideParameterDomainViolation { source }
            })?;
        let derived_sell_beta = beta_0 + delta_beta;
        let _sell_side_parameters = NigParameters::try_new(alpha, derived_sell_beta, delta, mu)
            .map_err(|source| {
                NigSharedPlusShiftConfigurationError::SellSideParameterDomainViolation {
                    beta_0,
                    delta_beta,
                    derived_sell_beta,
                    source,
                }
            })?;

        Ok(Self {
            alpha,
            beta_0,
            delta,
            mu,
            delta_beta,
        })
    }

    /// Return the derived sell-side skew parameter `beta_0 + delta_beta`.
    pub fn derived_sell_beta(self) -> f64 {
        self.beta_0 + self.delta_beta
    }

    /// Build both buy and sell driver parameter payloads from one shared source.
    pub fn derive_driver_parameters(
        self,
    ) -> Result<NigBuySellDriverParameters, NigSharedPlusShiftConfigurationError> {
        Self::validate_delta_beta(self.delta_beta)?;

        let buy_side_parameters =
            NigParameters::try_new(self.alpha, self.beta_0, self.delta, self.mu).map_err(
                |source| NigSharedPlusShiftConfigurationError::BuySideParameterDomainViolation {
                    source,
                },
            )?;
        let derived_sell_beta = self.derived_sell_beta();
        let sell_side_parameters =
            NigParameters::try_new(self.alpha, derived_sell_beta, self.delta, self.mu).map_err(
                |source| NigSharedPlusShiftConfigurationError::SellSideParameterDomainViolation {
                    beta_0: self.beta_0,
                    delta_beta: self.delta_beta,
                    derived_sell_beta,
                    source,
                },
            )?;

        Ok(NigBuySellDriverParameters {
            buy_side_parameters,
            sell_side_parameters,
            policy: NIG_SHARED_PLUS_SHIFT_CONFIGURATION_POLICY,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct NigBuySellDriverParameters {
    pub buy_side_parameters: NigParameters,
    pub sell_side_parameters: NigParameters,
    pub policy: &'static str,
}

/// Trading-time characteristic exponent contributions for one return argument.
///
/// Equation reference:
/// - `docs/design/janiform_compact_model_spec.tex`, Section 5
/// - `docs/design/janiform_design_brainstorm_notes.md`, Section 2.1
///
/// The canonical return composition sign convention is:
/// \f[
/// A^+\psi_+(u) + A^-\psi_-(-u).
/// \f]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NigTradingTimeCharacteristicExponentContributions {
    pub buy_side_weighted_exponent: ComplexScalar,
    pub sell_side_weighted_exponent: ComplexScalar,
    pub combined_weighted_exponent: ComplexScalar,
    pub policy: &'static str,
}

/// Trading-time sampled return increment contributions for one business-time interval.
///
/// Equation reference:
/// \f[
/// \Delta X_{\mathrm{trading}} = \Delta L_{A^+}^+ - \Delta L_{A^-}^-.
/// \f]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NigTradingTimeIncrementContributions {
    pub buy_side_increment: f64,
    pub sell_side_increment: f64,
    pub combined_increment: f64,
    pub policy: &'static str,
}

impl NigBuySellDriverParameters {
    /// Evaluate buy-side NIG exponent with the direct argument \f$u\f$, i.e. \f$\psi_+(u)\f$.
    pub fn buy_side_characteristic_exponent(self, argument: ComplexScalar) -> ComplexScalar {
        self.buy_side_parameters.characteristic_exponent(argument)
    }

    /// Evaluate sell-side NIG exponent with the negated argument, i.e. \f$\psi_-(-u)\f$.
    ///
    /// This sign convention matches the model equation
    /// \f[
    /// X_t = L_{A_t^+}^+ - L_{A_t^-}^- + G_t,
    /// \f]
    /// which yields the characteristic-function term
    /// \f[
    /// \exp\!\left(A^+\psi_+(u) + A^-\psi_-(-u)\right).
    /// \f]
    pub fn sell_side_characteristic_exponent_for_return_composition(
        self,
        argument: ComplexScalar,
    ) -> ComplexScalar {
        self.sell_side_parameters
            .characteristic_exponent(argument.scale(-1.0))
    }

    /// Assemble trading-time weighted characteristic exponents with canonical sign handling.
    pub fn compose_trading_time_weighted_characteristic_exponent(
        self,
        argument: ComplexScalar,
        integrated_buy_intensity: f64,
        integrated_sell_intensity: f64,
    ) -> NigTradingTimeCharacteristicExponentContributions {
        let buy_side_weighted_exponent = self
            .buy_side_characteristic_exponent(argument)
            .scale(integrated_buy_intensity);
        let sell_side_weighted_exponent = self
            .sell_side_characteristic_exponent_for_return_composition(argument)
            .scale(integrated_sell_intensity);
        let combined_weighted_exponent =
            buy_side_weighted_exponent.add(sell_side_weighted_exponent);

        NigTradingTimeCharacteristicExponentContributions {
            buy_side_weighted_exponent,
            sell_side_weighted_exponent,
            combined_weighted_exponent,
            policy: NIG_TRADING_TIME_RETURN_SIGN_CONVENTION_POLICY,
        }
    }

    /// Sample one trading-time return increment using the canonical buy-minus-sell composition.
    ///
    /// Sampling path:
    /// - buy-side increment uses `integrated_buy_intensity`
    /// - sell-side increment uses `integrated_sell_intensity`
    /// - combined trading-time increment is `buy_side_increment - sell_side_increment`
    pub fn sample_trading_time_return_increment<R>(
        self,
        integrated_buy_intensity: f64,
        integrated_sell_intensity: f64,
        randomness_stream: &mut R,
    ) -> Result<NigTradingTimeIncrementContributions, NigTradingTimeIncrementSamplingError>
    where
        R: SimulationRandomnessStream,
    {
        let buy_side_increment = self
            .buy_side_parameters
            .sample_increment(integrated_buy_intensity, randomness_stream)
            .map_err(
                |source| NigTradingTimeIncrementSamplingError::BuySideSamplingFailure {
                    integrated_buy_intensity,
                    source,
                },
            )?;
        let sell_side_increment = self
            .sell_side_parameters
            .sample_increment(integrated_sell_intensity, randomness_stream)
            .map_err(
                |source| NigTradingTimeIncrementSamplingError::SellSideSamplingFailure {
                    integrated_sell_intensity,
                    source,
                },
            )?;

        Ok(NigTradingTimeIncrementContributions {
            buy_side_increment,
            sell_side_increment,
            combined_increment: buy_side_increment - sell_side_increment,
            policy: NIG_TRADING_TIME_INCREMENT_COMPOSITION_POLICY,
        })
    }
}
