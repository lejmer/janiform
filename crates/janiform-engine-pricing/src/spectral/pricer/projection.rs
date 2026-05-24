use std::collections::BTreeSet;

use super::{VanillaOptionGridRequest, VanillaOptionPayoffType};
use crate::intraday_forward_price;

pub(super) fn spectral_projection_payoff_type_for_forward(
    payoff_type: VanillaOptionPayoffType,
    forward_price: f64,
    strike: f64,
) -> Option<VanillaOptionPayoffType> {
    if !forward_price.is_finite() || forward_price <= 0.0 || !strike.is_finite() || strike <= 0.0 {
        return None;
    }

    match payoff_type {
        VanillaOptionPayoffType::Call if strike < forward_price => {
            Some(VanillaOptionPayoffType::Put)
        }
        VanillaOptionPayoffType::Call => Some(VanillaOptionPayoffType::Call),
        VanillaOptionPayoffType::Put if strike > forward_price => {
            Some(VanillaOptionPayoffType::Call)
        }
        VanillaOptionPayoffType::Put => Some(VanillaOptionPayoffType::Put),
    }
}

pub(super) fn required_projection_payoff_types_for_grid(
    spot: f64,
    grid: &VanillaOptionGridRequest,
) -> Result<BTreeSet<VanillaOptionPayoffType>, String> {
    let mut payoff_types = BTreeSet::new();

    for maturity_interval in &grid.maturity_intervals {
        let forward_price = intraday_forward_price(
            spot,
            grid.risk_free_rate,
            grid.dividend_yield,
            *maturity_interval,
        )
        .ok_or_else(|| {
            format!(
                "failed to resolve finite positive forward price for maturity_interval={maturity_interval}"
            )
        })?;

        for strike in &grid.strikes {
            let projection_payoff_type = spectral_projection_payoff_type_for_forward(
                grid.payoff_type,
                forward_price,
                *strike,
            )
            .ok_or_else(|| {
                format!(
                    "failed to resolve spectral projection payoff type for strike={strike} maturity_interval={maturity_interval}"
                )
            })?;

            payoff_types.insert(projection_payoff_type);
        }
    }

    Ok(payoff_types)
}

#[cfg(test)]
mod tests {
    use super::{
        VanillaOptionGridRequest, VanillaOptionPayoffType,
        required_projection_payoff_types_for_grid, spectral_projection_payoff_type_for_forward,
    };

    #[test]
    fn spectral_projection_payoff_type_uses_otm_parity_side() {
        let in_the_money_call_projection =
            spectral_projection_payoff_type_for_forward(VanillaOptionPayoffType::Call, 100.0, 90.0);

        let out_of_the_money_call_projection = spectral_projection_payoff_type_for_forward(
            VanillaOptionPayoffType::Call,
            100.0,
            110.0,
        );

        let out_of_the_money_put_projection =
            spectral_projection_payoff_type_for_forward(VanillaOptionPayoffType::Put, 100.0, 110.0);

        let in_the_money_put_projection =
            spectral_projection_payoff_type_for_forward(VanillaOptionPayoffType::Put, 100.0, 90.0);

        assert_eq!(
            in_the_money_call_projection,
            Some(VanillaOptionPayoffType::Put)
        );
        assert_eq!(
            out_of_the_money_call_projection,
            Some(VanillaOptionPayoffType::Call)
        );
        assert_eq!(
            out_of_the_money_put_projection,
            Some(VanillaOptionPayoffType::Call)
        );
        assert_eq!(
            in_the_money_put_projection,
            Some(VanillaOptionPayoffType::Put),
        );
    }

    #[test]
    fn spectral_projection_payoff_type_rejects_invalid_forward_or_strike() {
        assert_eq!(
            spectral_projection_payoff_type_for_forward(
                VanillaOptionPayoffType::Call,
                f64::NAN,
                90.0,
            ),
            None,
        );
        assert_eq!(
            spectral_projection_payoff_type_for_forward(VanillaOptionPayoffType::Call, 100.0, 0.0),
            None,
        );
    }

    #[test]
    fn required_projection_payoff_types_cover_each_needed_payoff_side() {
        let grid = VanillaOptionGridRequest {
            payoff_type: VanillaOptionPayoffType::Call,
            risk_free_rate: 0.0,
            dividend_yield: 0.0,
            strikes: vec![90.0, 110.0],
            maturity_intervals: vec![1.0],
        };

        let payoff_types = required_projection_payoff_types_for_grid(100.0, &grid)
            .expect("projection payoff types should resolve")
            .into_iter()
            .collect::<Vec<_>>();

        assert_eq!(
            payoff_types,
            vec![VanillaOptionPayoffType::Call, VanillaOptionPayoffType::Put],
        );
    }
}
