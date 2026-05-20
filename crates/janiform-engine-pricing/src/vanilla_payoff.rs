use crate::contracts::VanillaOptionPayoffType;

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
