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
    vanilla_payoff_price_from_discounted_projection(
        payoff_type,
        VanillaOptionPayoffType::Call,
        discounted_call_price,
        discounted_spot,
        discounted_strike,
    )
}

pub(crate) fn vanilla_payoff_price_from_discounted_projection(
    payoff_type: VanillaOptionPayoffType,
    projection_payoff_type: VanillaOptionPayoffType,
    discounted_projection_price: f64,
    discounted_spot: f64,
    discounted_strike: f64,
) -> Option<f64> {
    if !discounted_projection_price.is_finite()
        || !discounted_spot.is_finite()
        || !discounted_strike.is_finite()
    {
        return None;
    }

    let price = unvalidated_vanilla_payoff_price_from_discounted_projection(
        payoff_type,
        projection_payoff_type,
        discounted_projection_price,
        discounted_spot,
        discounted_strike,
    );

    price.is_finite().then_some(price)
}

pub(crate) fn unvalidated_vanilla_payoff_price_from_discounted_projection(
    payoff_type: VanillaOptionPayoffType,
    projection_payoff_type: VanillaOptionPayoffType,
    discounted_projection_price: f64,
    discounted_spot: f64,
    discounted_strike: f64,
) -> f64 {
    match (payoff_type, projection_payoff_type) {
        (VanillaOptionPayoffType::Call, VanillaOptionPayoffType::Call)
        | (VanillaOptionPayoffType::Put, VanillaOptionPayoffType::Put) => {
            discounted_projection_price
        }
        (VanillaOptionPayoffType::Call, VanillaOptionPayoffType::Put) => {
            discounted_projection_price + discounted_spot - discounted_strike
        }
        (VanillaOptionPayoffType::Put, VanillaOptionPayoffType::Call) => {
            discounted_projection_price - discounted_spot + discounted_strike
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        VanillaOptionPayoffType, vanilla_payoff_price_from_discounted_call,
        vanilla_payoff_price_from_discounted_projection,
    };

    #[test]
    fn discounted_projection_price_maps_put_projection_to_call_by_parity() {
        let price = vanilla_payoff_price_from_discounted_projection(
            VanillaOptionPayoffType::Call,
            VanillaOptionPayoffType::Put,
            3.0,
            101.0,
            99.0,
        );

        assert_eq!(price, Some(5.0));
    }

    #[test]
    fn discounted_projection_price_maps_call_projection_to_put_by_parity() {
        let price = vanilla_payoff_price_from_discounted_projection(
            VanillaOptionPayoffType::Put,
            VanillaOptionPayoffType::Call,
            5.0,
            101.0,
            99.0,
        );

        assert_eq!(price, Some(3.0));
    }

    #[test]
    fn discounted_call_mapping_uses_call_projection() {
        let put_price = vanilla_payoff_price_from_discounted_call(
            VanillaOptionPayoffType::Put,
            5.0,
            101.0,
            99.0,
        );

        assert_eq!(put_price, Some(3.0));
    }
}
