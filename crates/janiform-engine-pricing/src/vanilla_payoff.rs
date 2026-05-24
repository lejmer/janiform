use crate::contracts::VanillaOptionPayoffType;

/// Evaluate an undiscounted vanilla option payoff at terminal spot.
pub fn vanilla_option_terminal_payoff(
    payoff_type: VanillaOptionPayoffType,
    terminal_spot: f64,
    strike: f64,
) -> f64 {
    match payoff_type {
        VanillaOptionPayoffType::Call => (terminal_spot - strike).max(0.0),
        VanillaOptionPayoffType::Put => (strike - terminal_spot).max(0.0),
    }
}

pub(crate) fn vanilla_payoff_price_from_discounted_call(
    payoff_type: VanillaOptionPayoffType,
    discounted_call_price: f64,
    discounted_spot: f64,
    discounted_strike: f64,
) -> Option<f64> {
    if !discounted_call_price.is_finite()
        || !discounted_spot.is_finite()
        || !discounted_strike.is_finite()
    {
        return None;
    }

    let price = match payoff_type {
        VanillaOptionPayoffType::Call => discounted_call_price,
        VanillaOptionPayoffType::Put => discounted_call_price - discounted_spot + discounted_strike,
    };

    price.is_finite().then_some(price)
}
