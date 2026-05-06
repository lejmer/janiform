use janiform_foundations::complex_math::{ComplexScalar, complex_exponential};
use statrs::function::erf::erfc;

use super::contracts::{VanillaOptionPayoffType, VanillaOptionPricingMarketInputs};
use super::time_scaling::intraday_year_fraction;

/// Evaluate the Black-Scholes log-return characteristic function under the raw market-input time unit.
pub fn black_scholes_log_return_characteristic_function(
    market_inputs: &VanillaOptionPricingMarketInputs,
    volatility: f64,
    argument: ComplexScalar,
) -> ComplexScalar {
    let maturity_interval = single_maturity_interval_or_nan(market_inputs);

    let variance = volatility * volatility * maturity_interval;
    let drift = (market_inputs.risk_free_rate
        - market_inputs.dividend_yield
        - 0.5 * volatility * volatility)
        * maturity_interval;

    let imaginary_unit_term = ComplexScalar::new(-argument.imaginary, argument.real);
    let drift_term = imaginary_unit_term.scale(drift);
    let variance_term = argument.square().scale(-0.5 * variance);

    complex_exponential(drift_term.plus(variance_term))
}

/// Evaluate the Black-Scholes log-return characteristic function for hourly intraday maturities and annualized rates.
pub fn intraday_black_scholes_log_return_characteristic_function(
    market_inputs: &VanillaOptionPricingMarketInputs,
    volatility: f64,
    argument: ComplexScalar,
) -> ComplexScalar {
    black_scholes_log_return_characteristic_function(
        &intraday_black_scholes_market_inputs(market_inputs),
        volatility,
        argument,
    )
}

/// Evaluate the Black-Scholes vanilla option price under the raw market-input time unit.
pub fn black_scholes_price(
    market_inputs: &VanillaOptionPricingMarketInputs,
    payoff_type: VanillaOptionPayoffType,
    volatility: f64,
) -> f64 {
    let (discounted_forward, discounted_strike) = discounted_underlyings(market_inputs);

    let maturity_interval = single_maturity_interval_or_nan(market_inputs);
    let strike = single_strike_or_nan(market_inputs);

    if volatility == 0.0 {
        return match payoff_type {
            VanillaOptionPayoffType::Call => (discounted_forward - discounted_strike).max(0.0),
            VanillaOptionPayoffType::Put => (discounted_strike - discounted_forward).max(0.0),
        };
    }

    let total_standard_deviation = volatility * maturity_interval.sqrt();
    if !total_standard_deviation.is_finite() {
        return match payoff_type {
            VanillaOptionPayoffType::Call => discounted_forward,
            VanillaOptionPayoffType::Put => discounted_strike,
        };
    }

    let log_forward_moneyness = (market_inputs.spot / strike).ln()
        + (market_inputs.risk_free_rate - market_inputs.dividend_yield) * maturity_interval;
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

/// Evaluate the Black-Scholes vanilla option price for hourly intraday maturities and annualized rates.
pub fn intraday_black_scholes_price(
    market_inputs: &VanillaOptionPricingMarketInputs,
    payoff_type: VanillaOptionPayoffType,
    volatility: f64,
) -> f64 {
    black_scholes_price(
        &intraday_black_scholes_market_inputs(market_inputs),
        payoff_type,
        volatility,
    )
}

/// Compute the Black-Scholes finite no-arbitrage price bounds under the raw market-input time unit.
pub fn black_scholes_no_arbitrage_price_bounds(
    market_inputs: &VanillaOptionPricingMarketInputs,
    payoff_type: VanillaOptionPayoffType,
) -> (f64, f64) {
    let (discounted_forward, discounted_strike) = discounted_underlyings(market_inputs);

    match payoff_type {
        VanillaOptionPayoffType::Call => (
            (discounted_forward - discounted_strike).max(0.0),
            discounted_forward,
        ),
        VanillaOptionPayoffType::Put => (
            (discounted_strike - discounted_forward).max(0.0),
            discounted_strike,
        ),
    }
}

/// Compute the Black-Scholes finite no-arbitrage price bounds for hourly intraday maturities and annualized rates.
pub fn intraday_black_scholes_no_arbitrage_price_bounds(
    market_inputs: &VanillaOptionPricingMarketInputs,
    payoff_type: VanillaOptionPayoffType,
) -> (f64, f64) {
    black_scholes_no_arbitrage_price_bounds(
        &intraday_black_scholes_market_inputs(market_inputs),
        payoff_type,
    )
}

fn discounted_underlyings(market_inputs: &VanillaOptionPricingMarketInputs) -> (f64, f64) {
    let maturity_interval = single_maturity_interval_or_nan(market_inputs);
    let strike = single_strike_or_nan(market_inputs);

    let discounted_forward =
        market_inputs.spot * (-market_inputs.dividend_yield * maturity_interval).exp();
    let discounted_strike = strike * (-market_inputs.risk_free_rate * maturity_interval).exp();

    (discounted_forward, discounted_strike)
}

fn intraday_black_scholes_market_inputs(
    market_inputs: &VanillaOptionPricingMarketInputs,
) -> VanillaOptionPricingMarketInputs {
    VanillaOptionPricingMarketInputs::new_single_node(
        market_inputs.spot,
        single_strike_or_nan(market_inputs),
        intraday_year_fraction(single_maturity_interval_or_nan(market_inputs)),
        market_inputs.risk_free_rate,
        market_inputs.dividend_yield,
    )
}

fn single_strike_or_nan(market_inputs: &VanillaOptionPricingMarketInputs) -> f64 {
    market_inputs.single_strike().unwrap_or(f64::NAN)
}

fn single_maturity_interval_or_nan(market_inputs: &VanillaOptionPricingMarketInputs) -> f64 {
    market_inputs.single_maturity_interval().unwrap_or(f64::NAN)
}

fn standard_normal_cdf(value: f64) -> f64 {
    0.5 * erfc(-value / std::f64::consts::SQRT_2)
}
