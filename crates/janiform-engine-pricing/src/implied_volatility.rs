use std::f64::consts::SQRT_2;

use serde::{Deserialize, Serialize};
use statrs::function::erf::erfc;

use crate::{VanillaOptionPayoffType, VanillaOptionPricingMarketInputs};

/// Structured outcome for Black-Scholes implied-volatility inversion on Janiform prices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VanillaOptionImpliedVolatilityInversionStatus {
    Succeeded,
    PriceBelowNoArbitrageLowerBound,
    PriceEqualsFiniteUpperBound,
    PriceAboveFiniteUpperBound,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VanillaOptionImpliedVolatilityInversionOutcome {
    pub implied_volatility: Option<f64>,
    pub inversion_status: VanillaOptionImpliedVolatilityInversionStatus,
}

pub(crate) fn invert_vanilla_option_implied_volatility(
    market_inputs: VanillaOptionPricingMarketInputs,
    payoff_type: VanillaOptionPayoffType,
    option_price: f64,
) -> VanillaOptionImpliedVolatilityInversionOutcome {
    let lower_bound = no_arbitrage_lower_bound(market_inputs, payoff_type);
    let upper_bound = finite_upper_bound(market_inputs, payoff_type);

    if option_price < lower_bound {
        return VanillaOptionImpliedVolatilityInversionOutcome {
            implied_volatility: None,
            inversion_status:
                VanillaOptionImpliedVolatilityInversionStatus::PriceBelowNoArbitrageLowerBound,
        };
    }

    if option_price == lower_bound {
        return VanillaOptionImpliedVolatilityInversionOutcome {
            implied_volatility: Some(0.0),
            inversion_status: VanillaOptionImpliedVolatilityInversionStatus::Succeeded,
        };
    }

    if option_price == upper_bound {
        return VanillaOptionImpliedVolatilityInversionOutcome {
            implied_volatility: None,
            inversion_status:
                VanillaOptionImpliedVolatilityInversionStatus::PriceEqualsFiniteUpperBound,
        };
    }

    if option_price > upper_bound {
        return VanillaOptionImpliedVolatilityInversionOutcome {
            implied_volatility: None,
            inversion_status:
                VanillaOptionImpliedVolatilityInversionStatus::PriceAboveFiniteUpperBound,
        };
    }

    let mut lower_parameter: f64 = 0.0;
    let mut upper_parameter: f64 = 1.0;
    let mut best_implied_volatility: Option<f64> = None;
    let mut best_price_error = f64::INFINITY;

    loop {
        let midpoint_parameter = lower_parameter.midpoint(upper_parameter);
        if midpoint_parameter == lower_parameter || midpoint_parameter == upper_parameter {
            break;
        }

        let midpoint_implied_volatility =
            implied_volatility_parameter_to_volatility(midpoint_parameter, market_inputs);
        let midpoint_price =
            black_scholes_price(market_inputs, payoff_type, midpoint_implied_volatility);

        if midpoint_implied_volatility.is_finite() {
            let midpoint_price_error = (midpoint_price - option_price).abs();
            if midpoint_price_error < best_price_error {
                best_price_error = midpoint_price_error;
                best_implied_volatility = Some(midpoint_implied_volatility);
            }
        }

        if midpoint_price < option_price {
            lower_parameter = midpoint_parameter;
            continue;
        }

        if midpoint_price > option_price {
            upper_parameter = midpoint_parameter;
            continue;
        }

        return VanillaOptionImpliedVolatilityInversionOutcome {
            implied_volatility: Some(midpoint_implied_volatility),
            inversion_status: VanillaOptionImpliedVolatilityInversionStatus::Succeeded,
        };
    }

    VanillaOptionImpliedVolatilityInversionOutcome {
        implied_volatility: best_implied_volatility,
        inversion_status: VanillaOptionImpliedVolatilityInversionStatus::Succeeded,
    }
}

fn no_arbitrage_lower_bound(
    market_inputs: VanillaOptionPricingMarketInputs,
    payoff_type: VanillaOptionPayoffType,
) -> f64 {
    let discounted_forward = discounted_forward_price(market_inputs);
    let discounted_strike = discounted_strike(market_inputs);

    match payoff_type {
        VanillaOptionPayoffType::Call => (discounted_forward - discounted_strike).max(0.0),
        VanillaOptionPayoffType::Put => (discounted_strike - discounted_forward).max(0.0),
    }
}

fn finite_upper_bound(
    market_inputs: VanillaOptionPricingMarketInputs,
    payoff_type: VanillaOptionPayoffType,
) -> f64 {
    match payoff_type {
        VanillaOptionPayoffType::Call => discounted_forward_price(market_inputs),
        VanillaOptionPayoffType::Put => discounted_strike(market_inputs),
    }
}

fn discounted_forward_price(market_inputs: VanillaOptionPricingMarketInputs) -> f64 {
    market_inputs.spot * (-market_inputs.dividend_yield * market_inputs.maturity_interval).exp()
}

fn discounted_strike(market_inputs: VanillaOptionPricingMarketInputs) -> f64 {
    market_inputs.strike * (-market_inputs.risk_free_rate * market_inputs.maturity_interval).exp()
}

fn implied_volatility_parameter_to_volatility(
    implied_volatility_parameter: f64,
    market_inputs: VanillaOptionPricingMarketInputs,
) -> f64 {
    let total_standard_deviation =
        implied_volatility_parameter / (1.0 - implied_volatility_parameter);

    total_standard_deviation / market_inputs.maturity_interval.sqrt()
}

fn black_scholes_price(
    market_inputs: VanillaOptionPricingMarketInputs,
    payoff_type: VanillaOptionPayoffType,
    implied_volatility: f64,
) -> f64 {
    let discounted_forward = discounted_forward_price(market_inputs);
    let discounted_strike = discounted_strike(market_inputs);

    if implied_volatility == 0.0 {
        return match payoff_type {
            VanillaOptionPayoffType::Call => (discounted_forward - discounted_strike).max(0.0),
            VanillaOptionPayoffType::Put => (discounted_strike - discounted_forward).max(0.0),
        };
    }

    let total_standard_deviation = implied_volatility * market_inputs.maturity_interval.sqrt();
    if !total_standard_deviation.is_finite() {
        return match payoff_type {
            VanillaOptionPayoffType::Call => discounted_forward,
            VanillaOptionPayoffType::Put => discounted_strike,
        };
    }

    let log_forward_moneyness = (market_inputs.spot / market_inputs.strike).ln()
        + (market_inputs.risk_free_rate - market_inputs.dividend_yield)
            * market_inputs.maturity_interval;

    let d1 = (log_forward_moneyness + 0.5 * total_standard_deviation * total_standard_deviation)
        / total_standard_deviation;
    let d2 = d1 - total_standard_deviation;

    match payoff_type {
        VanillaOptionPayoffType::Call => {
            discounted_forward * standard_normal_cdf(d1)
                - discounted_strike * standard_normal_cdf(d2)
        }
        VanillaOptionPayoffType::Put => {
            discounted_strike * standard_normal_cdf(-d2)
                - discounted_forward * standard_normal_cdf(-d1)
        }
    }
}

fn standard_normal_cdf(value: f64) -> f64 {
    0.5 * erfc(-value / SQRT_2)
}

#[cfg(test)]
mod tests {
    use super::{
        VanillaOptionImpliedVolatilityInversionStatus, black_scholes_price,
        invert_vanilla_option_implied_volatility,
    };
    use crate::{VanillaOptionPayoffType, VanillaOptionPricingMarketInputs};

    #[test]
    fn implied_volatility_inversion_recovers_black_scholes_call_price() {
        let market_inputs =
            VanillaOptionPricingMarketInputs::try_new(100.0, 105.0, 1.5, 0.02, 0.01)
                .expect("market inputs should construct");
        let option_price = black_scholes_price(market_inputs, VanillaOptionPayoffType::Call, 0.24);

        let inversion = invert_vanilla_option_implied_volatility(
            market_inputs,
            VanillaOptionPayoffType::Call,
            option_price,
        );

        assert_eq!(
            inversion.inversion_status,
            VanillaOptionImpliedVolatilityInversionStatus::Succeeded
        );
        let implied_volatility = inversion
            .implied_volatility
            .expect("successful inversion should return a volatility");
        assert!((implied_volatility - 0.24).abs() < 1.0e-12);
    }

    #[test]
    fn implied_volatility_inversion_recovers_black_scholes_put_price() {
        let market_inputs = VanillaOptionPricingMarketInputs::try_new(100.0, 95.0, 0.75, 0.01, 0.0)
            .expect("market inputs should construct");
        let option_price = black_scholes_price(market_inputs, VanillaOptionPayoffType::Put, 0.31);

        let inversion = invert_vanilla_option_implied_volatility(
            market_inputs,
            VanillaOptionPayoffType::Put,
            option_price,
        );

        assert_eq!(
            inversion.inversion_status,
            VanillaOptionImpliedVolatilityInversionStatus::Succeeded
        );

        let implied_volatility = inversion
            .implied_volatility
            .expect("successful inversion should return a volatility");
        assert!((implied_volatility - 0.31).abs() < 1.0e-12);
    }

    #[test]
    fn implied_volatility_inversion_reports_price_below_lower_bound() {
        let market_inputs = VanillaOptionPricingMarketInputs::try_new(100.0, 100.0, 1.0, 0.02, 0.0)
            .expect("market inputs should construct");

        let inversion = invert_vanilla_option_implied_volatility(
            market_inputs,
            VanillaOptionPayoffType::Call,
            -0.01,
        );

        assert_eq!(
            inversion.inversion_status,
            VanillaOptionImpliedVolatilityInversionStatus::PriceBelowNoArbitrageLowerBound
        );
        assert!(inversion.implied_volatility.is_none());
    }

    #[test]
    fn implied_volatility_inversion_reports_price_at_upper_bound() {
        let market_inputs =
            VanillaOptionPricingMarketInputs::try_new(100.0, 100.0, 1.0, 0.02, 0.01)
                .expect("market inputs should construct");
        let upper_bound = market_inputs.spot
            * (-market_inputs.dividend_yield * market_inputs.maturity_interval).exp();

        let inversion = invert_vanilla_option_implied_volatility(
            market_inputs,
            VanillaOptionPayoffType::Call,
            upper_bound,
        );

        assert_eq!(
            inversion.inversion_status,
            VanillaOptionImpliedVolatilityInversionStatus::PriceEqualsFiniteUpperBound
        );
        assert!(inversion.implied_volatility.is_none());
    }

    #[test]
    fn implied_volatility_inversion_reports_price_above_upper_bound() {
        let market_inputs =
            VanillaOptionPricingMarketInputs::try_new(100.0, 100.0, 1.0, 0.02, 0.01)
                .expect("market inputs should construct");

        let inversion = invert_vanilla_option_implied_volatility(
            market_inputs,
            VanillaOptionPayoffType::Call,
            100.0,
        );

        assert_eq!(
            inversion.inversion_status,
            VanillaOptionImpliedVolatilityInversionStatus::PriceAboveFiniteUpperBound
        );
        assert!(inversion.implied_volatility.is_none());
    }
}
