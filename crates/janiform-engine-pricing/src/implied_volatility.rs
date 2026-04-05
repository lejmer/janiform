use serde::{Deserialize, Serialize};

use super::time_scaling::intraday_year_fraction;
use crate::{
    VanillaOptionPayoffType, VanillaOptionPricingMarketInputs,
    intraday_black_scholes_no_arbitrage_price_bounds, intraday_black_scholes_price,
};

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
    market_inputs: &VanillaOptionPricingMarketInputs,
    payoff_type: VanillaOptionPayoffType,
    option_price: f64,
) -> VanillaOptionImpliedVolatilityInversionOutcome {
    let (lower_bound, upper_bound) =
        intraday_black_scholes_no_arbitrage_price_bounds(market_inputs, payoff_type);

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
            intraday_black_scholes_price(market_inputs, payoff_type, midpoint_implied_volatility);

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

fn implied_volatility_parameter_to_volatility(
    implied_volatility_parameter: f64,
    market_inputs: &VanillaOptionPricingMarketInputs,
) -> f64 {
    let maturity_year_fraction =
        intraday_year_fraction(market_inputs.single_maturity_interval().unwrap_or(f64::NAN));

    let total_standard_deviation =
        implied_volatility_parameter / (1.0 - implied_volatility_parameter);

    total_standard_deviation / maturity_year_fraction.sqrt()
}

#[cfg(test)]
mod tests {
    use super::{
        VanillaOptionImpliedVolatilityInversionStatus, invert_vanilla_option_implied_volatility,
    };
    use crate::{
        VanillaOptionPayoffType, VanillaOptionPricingMarketInputs,
        intraday_black_scholes_no_arbitrage_price_bounds, intraday_black_scholes_price,
    };

    #[test]
    fn implied_volatility_inversion_recovers_black_scholes_call_price() {
        let market_inputs =
            VanillaOptionPricingMarketInputs::try_new_single_node(100.0, 105.0, 1.5, 0.02, 0.01)
                .expect("market inputs should construct");

        let option_price =
            intraday_black_scholes_price(&market_inputs, VanillaOptionPayoffType::Call, 0.24);

        let inversion = invert_vanilla_option_implied_volatility(
            &market_inputs,
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
        let market_inputs =
            VanillaOptionPricingMarketInputs::try_new_single_node(100.0, 95.0, 0.75, 0.01, 0.0)
                .expect("market inputs should construct");

        let option_price =
            intraday_black_scholes_price(&market_inputs, VanillaOptionPayoffType::Put, 0.31);

        let inversion = invert_vanilla_option_implied_volatility(
            &market_inputs,
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
        let market_inputs =
            VanillaOptionPricingMarketInputs::try_new_single_node(100.0, 100.0, 1.0, 0.02, 0.0)
                .expect("market inputs should construct");

        let inversion = invert_vanilla_option_implied_volatility(
            &market_inputs,
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
            VanillaOptionPricingMarketInputs::try_new_single_node(100.0, 100.0, 1.0, 0.02, 0.01)
                .expect("market inputs should construct");

        let upper_bound = intraday_black_scholes_no_arbitrage_price_bounds(
            &market_inputs,
            VanillaOptionPayoffType::Call,
        )
        .1;

        let inversion = invert_vanilla_option_implied_volatility(
            &market_inputs,
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
            VanillaOptionPricingMarketInputs::try_new_single_node(100.0, 100.0, 1.0, 0.02, 0.01)
                .expect("market inputs should construct");

        let inversion = invert_vanilla_option_implied_volatility(
            &market_inputs,
            VanillaOptionPayoffType::Call,
            100.0,
        );

        assert_eq!(
            inversion.inversion_status,
            VanillaOptionImpliedVolatilityInversionStatus::PriceAboveFiniteUpperBound
        );
        assert!(inversion.implied_volatility.is_none());
    }

    #[test]
    fn zero_vol_intraday_call_price_uses_annualized_rate_year_fraction() {
        let market_inputs = VanillaOptionPricingMarketInputs::try_new_single_node(
            1.01037, 1.00537, 1.0, 0.02, 0.01,
        )
        .expect("market inputs should construct");

        let option_price =
            intraday_black_scholes_price(&market_inputs, VanillaOptionPayoffType::Call, 0.0);

        assert!((option_price - 0.0050011411912873616).abs() < 1.0e-15);
    }
}
