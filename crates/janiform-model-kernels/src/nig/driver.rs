use serde::Serialize;

use janiform_foundations::numeric::validate_negative_number;
use janiform_foundations::randomness::SimulationRandomnessStream;

use super::constants::{
    NIG_SHARED_PLUS_SHIFT_CONFIGURATION_POLICY, NIG_TRADING_TIME_INCREMENT_COMPOSITION_POLICY,
    NIG_TRADING_TIME_RETURN_SIGN_CONVENTION_POLICY,
};
use super::errors::{NigSharedPlusShiftConfigurationError, NigTradingTimeIncrementSamplingError};
use super::parameters::NigParameters;
use crate::risk_neutral::{
    EsscherTransformError, NigEsscherTiltDomain, differentiate_nig_characteristic_exponent,
    differentiate_nig_characteristic_exponent_wrt_argument,
    differentiate_nig_esscher_log_moment_exponent_for_pure_imaginary_argument,
    differentiate_shifted_characteristic_exponent_wrt_theta,
};
use janiform_foundations::complex_math::ComplexScalar;

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
        validate_negative_number(delta_beta, "delta_beta").map_err(|source| {
            NigSharedPlusShiftConfigurationError::DeltaBetaDomainViolation {
                value: source.value,
            }
        })
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NigSharedPlusShiftCharacteristicExponentSensitivity {
    pub exponent: ComplexScalar,
    pub alpha_derivative: ComplexScalar,
    pub beta_0_derivative: ComplexScalar,
    pub delta_derivative: ComplexScalar,
    pub mu_derivative: ComplexScalar,
    pub delta_beta_derivative: ComplexScalar,
    pub theta_shift_derivative: ComplexScalar,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NigSharedPlusShiftPureImaginaryLogMomentSensitivity {
    pub observed_log_moment_exponent: f64,
    pub alpha_derivative: f64,
    pub beta_0_derivative: f64,
    pub delta_derivative: f64,
    pub mu_derivative: f64,
    pub delta_beta_derivative: f64,
    pub theta_derivative: f64,
}

/// Trading-time characteristic exponent contributions for one return argument.
///
/// Equation reference:
/// - `design/janiform_compact_model_spec.tex`, Section 5
/// - `design/archive/janiform_design_brainstorm_notes.md`, Section 2.1
///
/// The canonical mean-compensated return composition sign convention is:
/// \f[
/// A^+(\psi_+(u)-iu m_+) + A^-(\psi_-(-u)+iu m_-).
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
/// \Delta X_{\mathrm{trading}}
/// = (\Delta L_{A^+}^+ - A^+m_+) - (\Delta L_{A^-}^- - A^-m_-).
/// \f]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NigTradingTimeIncrementContributions {
    pub buy_side_increment: f64,
    pub sell_side_increment: f64,
    pub combined_increment: f64,
    pub buy_side_subordinator_increment: f64,
    pub sell_side_subordinator_increment: f64,
    pub combined_subordinator_increment: f64,
    pub policy: &'static str,
}

impl NigBuySellDriverParameters {
    /// Evaluate buy-side mean-compensated NIG exponent with direct argument \f$u\f$.
    pub fn buy_side_characteristic_exponent(self, argument: ComplexScalar) -> ComplexScalar {
        compensated_buy_side_characteristic_exponent(self.buy_side_parameters, argument)
    }

    pub fn differentiate_buy_side_characteristic_exponent_wrt_argument(
        self,
        argument: ComplexScalar,
    ) -> Result<ComplexScalar, EsscherTransformError> {
        let derivative = differentiate_nig_characteristic_exponent_wrt_argument(
            self.buy_side_parameters,
            argument,
        )?;

        Ok(derivative.plus(ComplexScalar::new(
            0.0,
            -self.buy_side_parameters.mean_per_unit_time(),
        )))
    }

    pub fn differentiate_buy_side_characteristic_exponent_for_shared_plus_shift_configuration(
        self,
        argument: ComplexScalar,
    ) -> Result<NigSharedPlusShiftCharacteristicExponentSensitivity, EsscherTransformError> {
        let sensitivity =
            differentiate_nig_characteristic_exponent(self.buy_side_parameters, argument)?;

        let mean_sensitivity =
            nig_mean_per_unit_time_sensitivity(self.buy_side_parameters, argument)?;

        Ok(NigSharedPlusShiftCharacteristicExponentSensitivity {
            exponent: self.buy_side_characteristic_exponent(argument),
            alpha_derivative: sensitivity.alpha_derivative.plus(
                buy_side_mean_compensation_correction(argument, mean_sensitivity.alpha_derivative),
            ),
            beta_0_derivative: sensitivity.beta_derivative.plus(
                buy_side_mean_compensation_correction(argument, mean_sensitivity.beta_derivative),
            ),
            delta_derivative: sensitivity.delta_derivative.plus(
                buy_side_mean_compensation_correction(argument, mean_sensitivity.delta_derivative),
            ),
            mu_derivative: sensitivity
                .mu_derivative
                .plus(buy_side_mean_compensation_correction(
                    argument,
                    mean_sensitivity.mu_derivative,
                )),
            delta_beta_derivative: ComplexScalar::new(0.0, 0.0),
            theta_shift_derivative: ComplexScalar::new(0.0, 0.0),
        })
    }

    pub fn differentiate_buy_side_characteristic_exponent_for_shared_plus_shift_configuration_with_negative_imaginary_shift(
        self,
        shifted_argument: ComplexScalar,
    ) -> Result<NigSharedPlusShiftCharacteristicExponentSensitivity, EsscherTransformError> {
        let mut sensitivity = self
            .differentiate_buy_side_characteristic_exponent_for_shared_plus_shift_configuration(
                shifted_argument,
            )?;

        sensitivity.theta_shift_derivative =
            differentiate_shifted_characteristic_exponent_wrt_theta(
                self.buy_side_parameters,
                shifted_argument,
                "buy_side_negative_imaginary_shift",
            )?
            .scale(-1.0)
            .plus(ComplexScalar::new(
                -self.buy_side_parameters.mean_per_unit_time(),
                0.0,
            ));

        Ok(sensitivity)
    }

    /// Evaluate sell-side mean-compensated NIG exponent with the negated argument.
    ///
    /// This sign convention matches the model equation
    /// \f[
    /// X_t = L_{A_t^+}^+ - L_{A_t^-}^- + G_t,
    /// \f]
    /// which yields the characteristic-function term
    /// \f[
    /// \exp\!\left(A^+(\psi_+(u)-iu m_+) + A^-(\psi_-(-u)+iu m_-)\right).
    /// \f]
    pub fn sell_side_characteristic_exponent_for_return_composition(
        self,
        argument: ComplexScalar,
    ) -> ComplexScalar {
        compensated_sell_side_characteristic_exponent(self.sell_side_parameters, argument)
    }

    pub fn differentiate_sell_side_characteristic_exponent_for_return_composition_wrt_argument(
        self,
        argument: ComplexScalar,
    ) -> Result<ComplexScalar, EsscherTransformError> {
        let physical_derivative = differentiate_nig_characteristic_exponent_wrt_argument(
            self.sell_side_parameters,
            argument.scale(-1.0),
        )?;

        Ok(physical_derivative.scale(-1.0).plus(ComplexScalar::new(
            0.0,
            self.sell_side_parameters.mean_per_unit_time(),
        )))
    }

    pub fn differentiate_sell_side_characteristic_exponent_for_return_composition_for_shared_plus_shift_configuration(
        self,
        argument: ComplexScalar,
    ) -> Result<NigSharedPlusShiftCharacteristicExponentSensitivity, EsscherTransformError> {
        let physical_sensitivity = differentiate_nig_characteristic_exponent(
            self.sell_side_parameters,
            argument.scale(-1.0),
        )?;

        let mean_sensitivity =
            nig_mean_per_unit_time_sensitivity(self.sell_side_parameters, argument)?;

        Ok(NigSharedPlusShiftCharacteristicExponentSensitivity {
            exponent: self.sell_side_characteristic_exponent_for_return_composition(argument),
            alpha_derivative: physical_sensitivity.alpha_derivative.plus(
                sell_side_mean_compensation_correction(argument, mean_sensitivity.alpha_derivative),
            ),
            beta_0_derivative: physical_sensitivity.beta_derivative.plus(
                sell_side_mean_compensation_correction(argument, mean_sensitivity.beta_derivative),
            ),
            delta_derivative: physical_sensitivity.delta_derivative.plus(
                sell_side_mean_compensation_correction(argument, mean_sensitivity.delta_derivative),
            ),
            mu_derivative: physical_sensitivity.mu_derivative.plus(
                sell_side_mean_compensation_correction(argument, mean_sensitivity.mu_derivative),
            ),
            delta_beta_derivative: physical_sensitivity.beta_derivative.plus(
                sell_side_mean_compensation_correction(argument, mean_sensitivity.beta_derivative),
            ),
            theta_shift_derivative: ComplexScalar::new(0.0, 0.0),
        })
    }

    pub fn differentiate_sell_side_characteristic_exponent_for_return_composition_for_shared_plus_shift_configuration_with_negative_imaginary_shift(
        self,
        shifted_argument: ComplexScalar,
    ) -> Result<NigSharedPlusShiftCharacteristicExponentSensitivity, EsscherTransformError> {
        let effective_argument = shifted_argument.scale(-1.0);

        let mut sensitivity = self
            .differentiate_sell_side_characteristic_exponent_for_return_composition_for_shared_plus_shift_configuration(
                shifted_argument,
            )?;

        sensitivity.theta_shift_derivative =
            differentiate_shifted_characteristic_exponent_wrt_theta(
                self.sell_side_parameters,
                effective_argument,
                "sell_side_return_composition_negative_imaginary_shift",
            )?
            .plus(ComplexScalar::new(
                self.sell_side_parameters.mean_per_unit_time(),
                0.0,
            ));

        Ok(sensitivity)
    }

    pub fn differentiate_buy_side_pure_imaginary_log_moment_for_shared_plus_shift_configuration(
        self,
        argument_imaginary: f64,
        theta: f64,
    ) -> Result<NigSharedPlusShiftPureImaginaryLogMomentSensitivity, EsscherTransformError> {
        let sensitivity =
            differentiate_nig_esscher_log_moment_exponent_for_pure_imaginary_argument(
                self.buy_side_parameters,
                argument_imaginary,
                theta,
            )?;
        let mean_sensitivity = nig_mean_per_unit_time_sensitivity(
            self.buy_side_parameters,
            ComplexScalar::new(0.0, argument_imaginary),
        )?;

        Ok(NigSharedPlusShiftPureImaginaryLogMomentSensitivity {
            observed_log_moment_exponent: sensitivity.observed_log_moment_exponent
                + argument_imaginary * self.buy_side_parameters.mean_per_unit_time(),
            alpha_derivative: sensitivity.alpha_derivative
                + argument_imaginary * mean_sensitivity.alpha_derivative,
            beta_0_derivative: sensitivity.beta_derivative
                + argument_imaginary * mean_sensitivity.beta_derivative,
            delta_derivative: sensitivity.delta_derivative
                + argument_imaginary * mean_sensitivity.delta_derivative,
            mu_derivative: sensitivity.mu_derivative
                + argument_imaginary * mean_sensitivity.mu_derivative,
            delta_beta_derivative: 0.0,
            theta_derivative: sensitivity.theta_derivative,
        })
    }

    pub fn differentiate_sell_side_pure_imaginary_log_moment_for_shared_plus_shift_configuration(
        self,
        argument_imaginary: f64,
        theta: f64,
    ) -> Result<NigSharedPlusShiftPureImaginaryLogMomentSensitivity, EsscherTransformError> {
        let sensitivity =
            differentiate_nig_esscher_log_moment_exponent_for_pure_imaginary_argument(
                self.sell_side_parameters,
                argument_imaginary,
                theta,
            )?;
        let mean_sensitivity = nig_mean_per_unit_time_sensitivity(
            self.sell_side_parameters,
            ComplexScalar::new(0.0, argument_imaginary),
        )?;

        Ok(NigSharedPlusShiftPureImaginaryLogMomentSensitivity {
            observed_log_moment_exponent: sensitivity.observed_log_moment_exponent
                - argument_imaginary * self.sell_side_parameters.mean_per_unit_time(),
            alpha_derivative: sensitivity.alpha_derivative
                - argument_imaginary * mean_sensitivity.alpha_derivative,
            beta_0_derivative: sensitivity.beta_derivative
                - argument_imaginary * mean_sensitivity.beta_derivative,
            delta_derivative: sensitivity.delta_derivative
                - argument_imaginary * mean_sensitivity.delta_derivative,
            mu_derivative: sensitivity.mu_derivative
                - argument_imaginary * mean_sensitivity.mu_derivative,
            delta_beta_derivative: sensitivity.beta_derivative
                - argument_imaginary * mean_sensitivity.beta_derivative,
            theta_derivative: sensitivity.theta_derivative,
        })
    }

    /// Derive the exact open lambda-domain for the return-composed block log moment.
    ///
    /// The combined block increment is `L^+ - L^-`, so the admissible lambda-domain is the
    /// intersection of:
    /// - buy side mgf domain for `L^+`
    /// - reflected sell side mgf domain for `-L^-`
    pub fn derive_return_composition_log_moment_domain(self) -> NigEsscherTiltDomain {
        let buy_domain = NigEsscherTiltDomain::new(
            -self.buy_side_parameters.alpha - self.buy_side_parameters.beta,
            self.buy_side_parameters.alpha - self.buy_side_parameters.beta,
        );

        let sell_domain = NigEsscherTiltDomain::new(
            self.sell_side_parameters.beta - self.sell_side_parameters.alpha,
            self.sell_side_parameters.beta + self.sell_side_parameters.alpha,
        );

        buy_domain.intersect(sell_domain)
    }

    /// Evaluate the exact log moment `log E[exp(lambda * (L^+ - L^-))]`.
    pub fn evaluate_return_composition_log_moment(
        self,
        integrated_buy_intensity: f64,
        integrated_sell_intensity: f64,
        lambda: f64,
    ) -> Result<f64, EsscherTransformError> {
        let domain = self.derive_return_composition_log_moment_domain();
        if !domain.contains(lambda) {
            return Err(EsscherTransformError::PureImaginaryArgumentOutOfDomain {
                theta: lambda,
                lower_theta_exclusive: domain.lower_theta_exclusive,
                upper_theta_exclusive: domain.upper_theta_exclusive,
                argument_imaginary: 0.0,
            });
        }

        let buy_log_moment = evaluate_buy_side_log_moment(self.buy_side_parameters, lambda)?;
        let sell_log_moment =
            evaluate_sell_side_return_composition_log_moment(self.sell_side_parameters, lambda)?;

        let weighted_log_moment = integrated_buy_intensity
            .mul_add(buy_log_moment, integrated_sell_intensity * sell_log_moment);

        if !weighted_log_moment.is_finite() {
            return Err(EsscherTransformError::NonFiniteTransformedExponent {
                value: ComplexScalar::new(weighted_log_moment, 0.0),
            });
        }

        Ok(weighted_log_moment)
    }

    /// Evaluate the exact derivative of `log E[exp(lambda * (L^+ - L^-))]`.
    pub fn differentiate_return_composition_log_moment(
        self,
        integrated_buy_intensity: f64,
        integrated_sell_intensity: f64,
        lambda: f64,
    ) -> Result<f64, EsscherTransformError> {
        let domain = self.derive_return_composition_log_moment_domain();
        if !domain.contains(lambda) {
            return Err(EsscherTransformError::PureImaginaryArgumentOutOfDomain {
                theta: lambda,
                lower_theta_exclusive: domain.lower_theta_exclusive,
                upper_theta_exclusive: domain.upper_theta_exclusive,
                argument_imaginary: 0.0,
            });
        }

        let buy_derivative = differentiate_buy_side_log_moment(self.buy_side_parameters, lambda)?;
        let sell_derivative = differentiate_sell_side_return_composition_log_moment(
            self.sell_side_parameters,
            lambda,
        )?;

        let weighted_derivative = integrated_buy_intensity
            .mul_add(buy_derivative, integrated_sell_intensity * sell_derivative);

        if !weighted_derivative.is_finite() {
            return Err(
                EsscherTransformError::NonFiniteCharacteristicExponentSensitivity {
                    parameter: "return_composition_lambda",
                    value: ComplexScalar::new(weighted_derivative, 0.0),
                },
            );
        }

        Ok(weighted_derivative)
    }

    /// Evaluate the exact return-composed exponent under an Esscher tilt `lambda`.
    ///
    /// This returns the transformed exponent
    /// `psi_lambda(u) = psi(u - i * lambda) - psi(-i * lambda)`
    /// for the return-composed increment `L^+ - L^-`.
    pub fn transform_return_composition_characteristic_exponent_under_esscher_tilt(
        self,
        argument: ComplexScalar,
        integrated_buy_intensity: f64,
        integrated_sell_intensity: f64,
        lambda: f64,
    ) -> Result<ComplexScalar, EsscherTransformError> {
        let domain = self.derive_return_composition_log_moment_domain();
        if !domain.contains(lambda) {
            return Err(EsscherTransformError::PureImaginaryArgumentOutOfDomain {
                theta: lambda,
                lower_theta_exclusive: domain.lower_theta_exclusive,
                upper_theta_exclusive: domain.upper_theta_exclusive,
                argument_imaginary: 0.0,
            });
        }

        let shifted_argument = argument.plus(ComplexScalar::new(0.0, -lambda));
        let shifted_exponent = self
            .compose_trading_time_weighted_characteristic_exponent(
                shifted_argument,
                integrated_buy_intensity,
                integrated_sell_intensity,
            )
            .combined_weighted_exponent;
        let anchor_log_moment = self.evaluate_return_composition_log_moment(
            integrated_buy_intensity,
            integrated_sell_intensity,
            lambda,
        )?;

        let combined_transformed =
            shifted_exponent.subtract(ComplexScalar::new(anchor_log_moment, 0.0));

        if !combined_transformed.is_finite() {
            return Err(EsscherTransformError::NonFiniteTransformedExponent {
                value: combined_transformed,
            });
        }

        Ok(combined_transformed)
    }

    /// Assemble mean-compensated trading-time weighted characteristic exponents.
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
            buy_side_weighted_exponent.plus(sell_side_weighted_exponent);

        NigTradingTimeCharacteristicExponentContributions {
            buy_side_weighted_exponent,
            sell_side_weighted_exponent,
            combined_weighted_exponent,
            policy: NIG_TRADING_TIME_RETURN_SIGN_CONVENTION_POLICY,
        }
    }

    /// Sample one mean-compensated trading-time return increment.
    ///
    /// Sampling path:
    /// - buy-side increment is raw buy-side NIG minus `integrated_buy_intensity * m_+`
    /// - sell-side increment is raw sell-side NIG minus `integrated_sell_intensity * m_-`
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
        let buy_side_sample = self
            .buy_side_parameters
            .sample_increment_with_subordinator(integrated_buy_intensity, randomness_stream)
            .map_err(
                |source| NigTradingTimeIncrementSamplingError::BuySideSamplingFailure {
                    integrated_buy_intensity,
                    source,
                },
            )?;

        let sell_side_sample = self
            .sell_side_parameters
            .sample_increment_with_subordinator(integrated_sell_intensity, randomness_stream)
            .map_err(
                |source| NigTradingTimeIncrementSamplingError::SellSideSamplingFailure {
                    integrated_sell_intensity,
                    source,
                },
            )?;

        let buy_side_increment = buy_side_sample.increment
            - integrated_buy_intensity * self.buy_side_parameters.mean_per_unit_time();
        let sell_side_increment = sell_side_sample.increment
            - integrated_sell_intensity * self.sell_side_parameters.mean_per_unit_time();
        let combined_subordinator_increment =
            buy_side_sample.subordinator_increment + sell_side_sample.subordinator_increment;

        Ok(NigTradingTimeIncrementContributions {
            buy_side_increment,
            sell_side_increment,
            combined_increment: buy_side_increment - sell_side_increment,
            buy_side_subordinator_increment: buy_side_sample.subordinator_increment,
            sell_side_subordinator_increment: sell_side_sample.subordinator_increment,
            combined_subordinator_increment,
            policy: NIG_TRADING_TIME_INCREMENT_COMPOSITION_POLICY,
        })
    }
}

fn evaluate_buy_side_log_moment(
    parameters: NigParameters,
    lambda: f64,
) -> Result<f64, EsscherTransformError> {
    let gamma = stable_positive_gamma(parameters)?;

    let shifted_margin = parameters.alpha * parameters.alpha
        - (parameters.beta + lambda) * (parameters.beta + lambda);

    if !shifted_margin.is_finite() || shifted_margin <= 0.0 {
        return Err(EsscherTransformError::PureImaginaryArgumentOutOfDomain {
            theta: lambda,
            lower_theta_exclusive: -parameters.alpha - parameters.beta,
            upper_theta_exclusive: parameters.alpha - parameters.beta,
            argument_imaginary: 0.0,
        });
    }

    let shifted_gamma = shifted_margin.sqrt();

    let raw_log_moment = parameters
        .mu
        .mul_add(lambda, parameters.delta * (gamma - shifted_gamma));
    let log_moment = raw_log_moment - lambda * parameters.mean_per_unit_time();
    if !log_moment.is_finite() {
        return Err(EsscherTransformError::NonFiniteTransformedExponent {
            value: ComplexScalar::new(log_moment, 0.0),
        });
    }

    Ok(log_moment)
}

fn evaluate_sell_side_return_composition_log_moment(
    parameters: NigParameters,
    lambda: f64,
) -> Result<f64, EsscherTransformError> {
    let gamma = stable_positive_gamma(parameters)?;

    let shifted_margin = parameters.alpha * parameters.alpha
        - (parameters.beta - lambda) * (parameters.beta - lambda);

    if !shifted_margin.is_finite() || shifted_margin <= 0.0 {
        return Err(EsscherTransformError::PureImaginaryArgumentOutOfDomain {
            theta: lambda,
            lower_theta_exclusive: parameters.beta - parameters.alpha,
            upper_theta_exclusive: parameters.beta + parameters.alpha,
            argument_imaginary: 0.0,
        });
    }

    let shifted_gamma = shifted_margin.sqrt();

    let raw_log_moment =
        (-parameters.mu).mul_add(lambda, parameters.delta * (gamma - shifted_gamma));
    let log_moment = raw_log_moment + lambda * parameters.mean_per_unit_time();
    if !log_moment.is_finite() {
        return Err(EsscherTransformError::NonFiniteTransformedExponent {
            value: ComplexScalar::new(log_moment, 0.0),
        });
    }

    Ok(log_moment)
}

fn differentiate_buy_side_log_moment(
    parameters: NigParameters,
    lambda: f64,
) -> Result<f64, EsscherTransformError> {
    let shifted_margin = parameters.alpha * parameters.alpha
        - (parameters.beta + lambda) * (parameters.beta + lambda);

    if !shifted_margin.is_finite() || shifted_margin <= 0.0 {
        return Err(EsscherTransformError::PureImaginaryArgumentOutOfDomain {
            theta: lambda,
            lower_theta_exclusive: -parameters.alpha - parameters.beta,
            upper_theta_exclusive: parameters.alpha - parameters.beta,
            argument_imaginary: 0.0,
        });
    }

    let shifted_gamma = shifted_margin.sqrt();

    let raw_derivative = parameters.mu.mul_add(
        1.0,
        parameters.delta * (parameters.beta + lambda) / shifted_gamma,
    );
    let derivative = raw_derivative - parameters.mean_per_unit_time();
    if !derivative.is_finite() {
        return Err(
            EsscherTransformError::NonFiniteCharacteristicExponentSensitivity {
                parameter: "buy_side_log_moment_lambda",
                value: ComplexScalar::new(derivative, 0.0),
            },
        );
    }

    Ok(derivative)
}

fn differentiate_sell_side_return_composition_log_moment(
    parameters: NigParameters,
    lambda: f64,
) -> Result<f64, EsscherTransformError> {
    let shifted_margin = parameters.alpha * parameters.alpha
        - (parameters.beta - lambda) * (parameters.beta - lambda);

    if !shifted_margin.is_finite() || shifted_margin <= 0.0 {
        return Err(EsscherTransformError::PureImaginaryArgumentOutOfDomain {
            theta: lambda,
            lower_theta_exclusive: parameters.beta - parameters.alpha,
            upper_theta_exclusive: parameters.beta + parameters.alpha,
            argument_imaginary: 0.0,
        });
    }

    let shifted_gamma = shifted_margin.sqrt();

    let raw_derivative = (-parameters.mu).mul_add(
        1.0,
        -parameters.delta * (parameters.beta - lambda) / shifted_gamma,
    );
    let derivative = raw_derivative + parameters.mean_per_unit_time();
    if !derivative.is_finite() {
        return Err(
            EsscherTransformError::NonFiniteCharacteristicExponentSensitivity {
                parameter: "sell_side_return_composition_log_moment_lambda",
                value: ComplexScalar::new(derivative, 0.0),
            },
        );
    }

    Ok(derivative)
}

fn stable_positive_gamma(parameters: NigParameters) -> Result<f64, EsscherTransformError> {
    let gamma_margin =
        (parameters.alpha - parameters.beta.abs()) * (parameters.alpha + parameters.beta.abs());
    if !gamma_margin.is_finite() || gamma_margin <= 0.0 {
        return Err(EsscherTransformError::NonFiniteTransformedExponent {
            value: ComplexScalar::new(gamma_margin, 0.0),
        });
    }

    Ok(gamma_margin.sqrt())
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct NigMeanPerUnitTimeSensitivity {
    alpha_derivative: f64,
    beta_derivative: f64,
    delta_derivative: f64,
    mu_derivative: f64,
}

fn compensated_buy_side_characteristic_exponent(
    parameters: NigParameters,
    argument: ComplexScalar,
) -> ComplexScalar {
    parameters
        .characteristic_exponent(argument)
        .plus(buy_side_mean_compensation_correction(
            argument,
            parameters.mean_per_unit_time(),
        ))
}

fn compensated_sell_side_characteristic_exponent(
    parameters: NigParameters,
    argument: ComplexScalar,
) -> ComplexScalar {
    parameters
        .characteristic_exponent(argument.scale(-1.0))
        .plus(sell_side_mean_compensation_correction(
            argument,
            parameters.mean_per_unit_time(),
        ))
}

fn buy_side_mean_compensation_correction(
    argument: ComplexScalar,
    mean_per_unit_time: f64,
) -> ComplexScalar {
    ComplexScalar::new(
        argument.imaginary * mean_per_unit_time,
        -argument.real * mean_per_unit_time,
    )
}

fn sell_side_mean_compensation_correction(
    argument: ComplexScalar,
    mean_per_unit_time: f64,
) -> ComplexScalar {
    ComplexScalar::new(
        -argument.imaginary * mean_per_unit_time,
        argument.real * mean_per_unit_time,
    )
}

fn nig_mean_per_unit_time_sensitivity(
    parameters: NigParameters,
    argument: ComplexScalar,
) -> Result<NigMeanPerUnitTimeSensitivity, EsscherTransformError> {
    let gamma = stable_positive_gamma(parameters)?;
    let gamma_cubed = gamma * gamma * gamma;

    let alpha_derivative = -parameters.delta * parameters.beta * parameters.alpha / gamma_cubed;
    let beta_derivative = parameters.delta * parameters.alpha * parameters.alpha / gamma_cubed;
    let delta_derivative = parameters.beta / gamma;
    let mu_derivative = 1.0;

    validate_real_sensitivity_component("nig_mean_per_unit_time_alpha", alpha_derivative)?;
    validate_real_sensitivity_component("nig_mean_per_unit_time_beta", beta_derivative)?;
    validate_real_sensitivity_component("nig_mean_per_unit_time_delta", delta_derivative)?;
    validate_real_sensitivity_component("nig_mean_per_unit_time_mu", mu_derivative)?;
    validate_complex_argument(argument)?;

    Ok(NigMeanPerUnitTimeSensitivity {
        alpha_derivative,
        beta_derivative,
        delta_derivative,
        mu_derivative,
    })
}

fn validate_real_sensitivity_component(
    parameter: &'static str,
    value: f64,
) -> Result<(), EsscherTransformError> {
    if !value.is_finite() {
        return Err(
            EsscherTransformError::NonFiniteCharacteristicExponentSensitivity {
                parameter,
                value: ComplexScalar::new(value, 0.0),
            },
        );
    }

    Ok(())
}

fn validate_complex_argument(argument: ComplexScalar) -> Result<(), EsscherTransformError> {
    if !argument.is_finite() {
        return Err(EsscherTransformError::InvalidArgument { value: argument });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use janiform_foundations::complex_math::ComplexScalar;

    use super::NigSharedPlusShiftConfiguration;

    #[test]
    fn return_composition_log_moment_domain_is_open_and_non_empty_for_valid_configuration() {
        let driver_parameters =
            NigSharedPlusShiftConfiguration::try_new(80.0, -12.0, 2.0e-4, 0.0, -8.0)
                .expect("configuration should validate")
                .derive_driver_parameters()
                .expect("driver parameters should derive");

        let domain = driver_parameters.derive_return_composition_log_moment_domain();

        assert!(domain.lower_theta_exclusive.is_finite());
        assert!(domain.upper_theta_exclusive.is_finite());
        assert!(domain.lower_theta_exclusive < domain.upper_theta_exclusive);
        assert!(domain.contains(0.0));
    }

    #[test]
    fn return_composition_log_moment_derivative_matches_central_difference() {
        let driver_parameters =
            NigSharedPlusShiftConfiguration::try_new(80.0, -12.0, 2.0e-4, 0.0, -8.0)
                .expect("configuration should validate")
                .derive_driver_parameters()
                .expect("driver parameters should derive");

        let integrated_buy_intensity = 140.0;
        let integrated_sell_intensity = 60.0;

        let lambda = -0.35;
        let lambda_step = 1.0e-6;

        let derivative = driver_parameters
            .differentiate_return_composition_log_moment(
                integrated_buy_intensity,
                integrated_sell_intensity,
                lambda,
            )
            .expect("derivative should evaluate");

        let forward_value = driver_parameters
            .evaluate_return_composition_log_moment(
                integrated_buy_intensity,
                integrated_sell_intensity,
                lambda + lambda_step,
            )
            .expect("forward log moment should evaluate");

        let backward_value = driver_parameters
            .evaluate_return_composition_log_moment(
                integrated_buy_intensity,
                integrated_sell_intensity,
                lambda - lambda_step,
            )
            .expect("backward log moment should evaluate");

        let finite_difference_derivative = (forward_value - backward_value) / (2.0 * lambda_step);

        assert_close(derivative, finite_difference_derivative);
    }

    #[test]
    fn return_composition_esscher_transformed_exponent_is_finite() {
        let driver_parameters =
            NigSharedPlusShiftConfiguration::try_new(80.0, -12.0, 2.0e-4, 0.0, -8.0)
                .expect("configuration should validate")
                .derive_driver_parameters()
                .expect("driver parameters should derive");

        let transformed_exponent = driver_parameters
            .transform_return_composition_characteristic_exponent_under_esscher_tilt(
                ComplexScalar::new(0.8, 0.0),
                140.0,
                60.0,
                -0.35,
            )
            .expect("transformed exponent should evaluate");

        assert!(transformed_exponent.is_finite());
    }

    fn assert_close(actual: f64, expected: f64) {
        let tolerance = 1.0e-9f64.max(1.0e-6 * actual.abs().max(expected.abs()));

        assert!(
            (actual - expected).abs() <= tolerance,
            "actual={actual} expected={expected} tolerance={tolerance}"
        );
    }
}
