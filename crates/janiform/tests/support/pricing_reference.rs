#![allow(dead_code)]

use janiform::complex_math::ComplexScalar;
use janiform::complex_math::complex_exponential;
use janiform::pricing::{VanillaOptionPayoffType, VanillaOptionPricingMarketInputs};

// Shared Black-Scholes CF baseline used across pricing regression tests.
pub fn black_scholes_log_return_characteristic_function(
    market_inputs: VanillaOptionPricingMarketInputs,
    volatility: f64,
    argument: ComplexScalar,
) -> ComplexScalar {
    let variance = volatility * volatility * market_inputs.maturity_interval;
    let drift = (market_inputs.risk_free_rate
        - market_inputs.dividend_yield
        - 0.5 * volatility * volatility)
        * market_inputs.maturity_interval;
    let imaginary_unit_term = ComplexScalar::new(-argument.imaginary, argument.real);
    let drift_term = imaginary_unit_term.scale(drift);
    let variance_term = argument.square().scale(-0.5 * variance);

    complex_exponential(drift_term.add(variance_term))
}

// Shared closed-form Black-Scholes price baseline used for fixture checks.
pub fn black_scholes_price(
    market_inputs: VanillaOptionPricingMarketInputs,
    volatility: f64,
    payoff_type: VanillaOptionPayoffType,
) -> f64 {
    let sigma_square_root_time = volatility * market_inputs.maturity_interval.sqrt();
    let log_moneyness = (market_inputs.spot / market_inputs.strike).ln();
    let carry = (market_inputs.risk_free_rate - market_inputs.dividend_yield)
        * market_inputs.maturity_interval;
    let d1 = (log_moneyness + carry + 0.5 * sigma_square_root_time * sigma_square_root_time)
        / sigma_square_root_time;
    let d2 = d1 - sigma_square_root_time;
    let discounted_spot = market_inputs.spot
        * (-market_inputs.dividend_yield * market_inputs.maturity_interval).exp();
    let discounted_strike = market_inputs.strike
        * (-market_inputs.risk_free_rate * market_inputs.maturity_interval).exp();

    match payoff_type {
        VanillaOptionPayoffType::Call => {
            discounted_spot * standard_normal_cdf(d1) - discounted_strike * standard_normal_cdf(d2)
        }
        VanillaOptionPayoffType::Put => {
            discounted_strike * standard_normal_cdf(-d2)
                - discounted_spot * standard_normal_cdf(-d1)
        }
    }
}

// Shared no-arbitrage bound baseline used for pricing sanity checks.
pub fn no_arbitrage_price_bounds(
    market_inputs: VanillaOptionPricingMarketInputs,
    payoff_type: VanillaOptionPayoffType,
) -> (f64, f64) {
    let discounted_spot = market_inputs.spot
        * (-market_inputs.dividend_yield * market_inputs.maturity_interval).exp();
    let discounted_strike = market_inputs.strike
        * (-market_inputs.risk_free_rate * market_inputs.maturity_interval).exp();

    match payoff_type {
        VanillaOptionPayoffType::Call => (
            (discounted_spot - discounted_strike).max(0.0),
            discounted_spot,
        ),
        VanillaOptionPayoffType::Put => (
            (discounted_strike - discounted_spot).max(0.0),
            discounted_strike,
        ),
    }
}

fn standard_normal_cdf(value: f64) -> f64 {
    if value < 0.0 {
        return 1.0 - standard_normal_cdf(-value);
    }

    let polynomial_scale = 1.0 / (1.0 + 0.231_641_9 * value);
    let polynomial_value = polynomial_scale
        * (0.319_381_53
            + polynomial_scale
                * (-0.356_563_782
                    + polynomial_scale
                        * (1.781_477_937
                            + polynomial_scale
                                * (-1.821_255_978 + polynomial_scale * 1.330_274_429))));
    let density = 0.398_942_280_401_432_7 * (-0.5 * value * value).exp();

    1.0 - density * polynomial_value
}
