use super::maturity_law::SolvedMaturityLaw;
use super::{
    ComplexScalar, NodePriceSharedPlusShiftSensitivity, VanillaOptionPayoffType,
    VanillaOptionPricingRunError, asset_reference_log_price_from_observed_spot,
    complex_exponential, complex_multiply,
};
use crate::intraday_discount_factor;

#[derive(Debug, Clone, Copy)]
struct CosPayoffProjection {
    asset_reference_price: f64,
    strike_boundary: f64,
    domain_lower_bound: f64,
    domain_upper_bound: f64,
    width: f64,
    discount_factor: f64,
}

impl CosPayoffProjection {
    fn try_new(
        spot: f64,
        strike: f64,
        risk_free_rate: f64,
        solved_maturity: &SolvedMaturityLaw,
    ) -> Option<Self> {
        let domain_lower_bound =
            solved_maturity.terminal_anchor_value + solved_maturity.deviation_lower_bound;
        let domain_upper_bound =
            solved_maturity.terminal_anchor_value + solved_maturity.deviation_upper_bound;

        let width = domain_upper_bound - domain_lower_bound;
        if width <= 0.0 || !width.is_finite() {
            return None;
        }

        let asset_reference_log_price =
            asset_reference_log_price_from_observed_spot(spot, solved_maturity.observed_log_price)?;
        let asset_reference_price = asset_reference_log_price.exp();
        if !asset_reference_price.is_finite() || asset_reference_price <= 0.0 {
            return None;
        }

        let strike_boundary =
            strike_log_boundary_from_reference_log(strike, asset_reference_log_price)?;
        let discount_factor =
            intraday_discount_factor(risk_free_rate, solved_maturity.maturity_interval)?;

        Some(Self {
            asset_reference_price,
            strike_boundary,
            domain_lower_bound,
            domain_upper_bound,
            width,
            discount_factor,
        })
    }

    fn payoff_coefficient(
        self,
        payoff_type: VanillaOptionPayoffType,
        strike: f64,
        frequency_index: usize,
    ) -> Option<f64> {
        payoff_cos_coefficient(
            payoff_type,
            self.asset_reference_price,
            strike,
            self.domain_lower_bound,
            self.domain_upper_bound,
            self.strike_boundary,
            frequency_index,
        )
    }

    fn phase(self, frequency_index: usize) -> ComplexScalar {
        let frequency = frequency_index as f64 * std::f64::consts::PI / self.width;

        complex_exponential(ComplexScalar::new(
            0.0,
            -frequency * self.domain_lower_bound,
        ))
    }
}

pub(super) fn cos_price_from_terminal_characteristic_function(
    spot: f64,
    strike: f64,
    payoff_type: VanillaOptionPayoffType,
    risk_free_rate: f64,
    solved_maturity: &SolvedMaturityLaw,
) -> Option<f64> {
    if solved_maturity.characteristic_function_values.len() != solved_maturity.node_count {
        return None;
    }

    let projection = CosPayoffProjection::try_new(spot, strike, risk_free_rate, solved_maturity)?;

    let mut price = 0.0;

    for (frequency_index, phi_value) in solved_maturity
        .characteristic_function_values
        .iter()
        .enumerate()
    {
        let coefficient = projection.payoff_coefficient(payoff_type, strike, frequency_index)?;
        let weight = if frequency_index == 0 { 0.5 } else { 1.0 };

        price += weight
            * complex_multiply(*phi_value, projection.phase(frequency_index)).real
            * coefficient;
    }

    let discounted_price = projection.discount_factor * price;

    discounted_price
        .is_finite()
        .then_some(discounted_price.max(0.0))
}

pub(super) fn cos_price_sensitivity_from_terminal_characteristic_function(
    spot: f64,
    strike: f64,
    payoff_type: VanillaOptionPayoffType,
    risk_free_rate: f64,
    solved_maturity: &SolvedMaturityLaw,
) -> Result<NodePriceSharedPlusShiftSensitivity, VanillaOptionPricingRunError> {
    let Some(characteristic_function_sensitivities) = solved_maturity
        .characteristic_function_sensitivities
        .as_ref()
    else {
        return Ok(NodePriceSharedPlusShiftSensitivity::zero());
    };

    if solved_maturity.characteristic_function_values.len() != solved_maturity.node_count
        || characteristic_function_sensitivities.len() != solved_maturity.node_count
    {
        return Err(VanillaOptionPricingRunError::InternalNumericsFailure {
            message: "COS price sensitivity requires characteristic function values for every node"
                .to_string(),
        });
    }

    let projection = CosPayoffProjection::try_new(spot, strike, risk_free_rate, solved_maturity)
        .ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
            message: "COS price sensitivity requires a valid payoff projection".to_string(),
        })?;

    let mut price_sensitivity = NodePriceSharedPlusShiftSensitivity::zero();

    for (frequency_index, phi_sensitivity) in
        characteristic_function_sensitivities.iter().enumerate()
    {
        let coefficient = projection
            .payoff_coefficient(payoff_type, strike, frequency_index)
            .ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
                message: format!(
                    "failed to resolve payoff COS coefficient for frequency_index={frequency_index}"
                ),
            })?;

        let phase = projection.phase(frequency_index);
        let weight = if frequency_index == 0 { 0.5 } else { 1.0 };
        let scaled_weight = projection.discount_factor * weight * coefficient;

        price_sensitivity = price_sensitivity.plus(NodePriceSharedPlusShiftSensitivity {
            alpha_derivative: scaled_weight
                * complex_multiply(phi_sensitivity.alpha_derivative, phase).real,
            beta_0_derivative: scaled_weight
                * complex_multiply(phi_sensitivity.beta_0_derivative, phase).real,
            delta_derivative: scaled_weight
                * complex_multiply(phi_sensitivity.delta_derivative, phase).real,
            delta_beta_derivative: scaled_weight
                * complex_multiply(phi_sensitivity.delta_beta_derivative, phase).real,
            buy_activity_premium_derivative: scaled_weight
                * complex_multiply(phi_sensitivity.buy_activity_premium_derivative, phase).real,
            sell_activity_premium_derivative: scaled_weight
                * complex_multiply(phi_sensitivity.sell_activity_premium_derivative, phase).real,
            pressure_signed_imbalance_exposure_risk_price_derivative: scaled_weight
                * complex_multiply(
                    phi_sensitivity.pressure_signed_imbalance_exposure_risk_price_derivative,
                    phase,
                )
                .real,
        });
    }

    Ok(price_sensitivity)
}

pub(super) fn strike_log_boundary_from_reference_log(
    strike: f64,
    asset_reference_log_price: f64,
) -> Option<f64> {
    if !strike.is_finite() || strike <= 0.0 || !asset_reference_log_price.is_finite() {
        return None;
    }

    let strike_boundary = strike.ln() - asset_reference_log_price;
    strike_boundary.is_finite().then_some(strike_boundary)
}

pub(super) fn payoff_cos_coefficient(
    payoff_type: VanillaOptionPayoffType,
    asset_reference_price: f64,
    strike: f64,
    domain_lower_bound: f64,
    domain_upper_bound: f64,
    strike_boundary: f64,
    frequency_index: usize,
) -> Option<f64> {
    let width = domain_upper_bound - domain_lower_bound;
    let angular_frequency = frequency_index as f64 * std::f64::consts::PI / width;

    match payoff_type {
        VanillaOptionPayoffType::Call => {
            if strike_boundary >= domain_upper_bound {
                return Some(0.0);
            }

            let lower = strike_boundary.max(domain_lower_bound);
            let upper = domain_upper_bound;

            Some(
                2.0 / width
                    * (asset_reference_price
                        * chi_coefficient(lower, upper, domain_lower_bound, angular_frequency)
                        - strike
                            * psi_coefficient(lower, upper, domain_lower_bound, angular_frequency)),
            )
        }
        VanillaOptionPayoffType::Put => {
            if strike_boundary <= domain_lower_bound {
                return Some(0.0);
            }

            let lower = domain_lower_bound;
            let upper = strike_boundary.min(domain_upper_bound);

            Some(
                2.0 / width
                    * (strike
                        * psi_coefficient(lower, upper, domain_lower_bound, angular_frequency)
                        - asset_reference_price
                            * chi_coefficient(lower, upper, domain_lower_bound, angular_frequency)),
            )
        }
    }
}

fn chi_coefficient(lower: f64, upper: f64, domain_lower_bound: f64, angular_frequency: f64) -> f64 {
    if angular_frequency == 0.0 {
        return upper.exp() - lower.exp();
    }

    let upper_shift = upper - domain_lower_bound;
    let lower_shift = lower - domain_lower_bound;

    let denominator = 1.0 + angular_frequency * angular_frequency;
    let upper_angle = angular_frequency * upper_shift;
    let lower_angle = angular_frequency * lower_shift;

    ((upper.exp() * (angular_frequency * upper_angle.sin() + upper_angle.cos()))
        - (lower.exp() * (angular_frequency * lower_angle.sin() + lower_angle.cos())))
        / denominator
}

fn psi_coefficient(lower: f64, upper: f64, domain_lower_bound: f64, angular_frequency: f64) -> f64 {
    if angular_frequency == 0.0 {
        return upper - lower;
    }

    let upper_shift = upper - domain_lower_bound;
    let lower_shift = lower - domain_lower_bound;
    let upper_angle = angular_frequency * upper_shift;
    let lower_angle = angular_frequency * lower_shift;

    (upper_angle.sin() - lower_angle.sin()) / angular_frequency
}
