use rayon::iter::{IndexedParallelIterator, IntoParallelRefIterator, ParallelIterator};

use super::domain::resolve_bounded_node_price_sensitivity;
use super::interval_operator::SolvedMaturityLaw;
use super::payoff::{
    cos_price_from_terminal_characteristic_function,
    cos_price_sensitivity_from_terminal_characteristic_function, spectral_projection_payoff_type,
};
use super::{
    PricingProgressEvent, VanillaOptionGridRequest, VanillaOptionImpliedVolatilityInversionOutcome,
    VanillaOptionImpliedVolatilityInversionStatus, VanillaOptionNodePriceSensitivity,
    VanillaOptionPayoffType, VanillaOptionPricingFailureStage, VanillaOptionPricingMarketInputs,
    VanillaOptionPricingStatus, emit_pricing_progress, install_pricing_parallelism,
    intraday_black_scholes_no_arbitrage_price_bounds, intraday_year_fraction,
    invert_vanilla_option_implied_volatility, resolved_pricing_cpu_count,
};
use crate::vanilla_payoff::vanilla_payoff_price_from_discounted_call;

pub(super) struct PricedStrikeRow {
    pub(super) price_row: Vec<Option<f64>>,
    pub(super) implied_volatility_row: Vec<Option<f64>>,
    pub(super) implied_volatility_status_row:
        Vec<Option<VanillaOptionImpliedVolatilityInversionStatus>>,
    pub(super) pricing_status_row: Vec<VanillaOptionPricingStatus>,
    pub(super) pricing_failure_stage_row: Vec<Option<VanillaOptionPricingFailureStage>>,
    pub(super) pricing_failure_reason_row: Vec<Option<String>>,
    pub(super) forward_price_row: Vec<Option<f64>>,
    pub(super) discount_factor_row: Vec<Option<f64>>,
    pub(super) boundary_state_norm_row: Vec<Option<f64>>,
    pub(super) node_price_sensitivity_row: Vec<Option<VanillaOptionNodePriceSensitivity>>,
}

pub(super) struct StrikeRowPricingInput<'a, 'observer, 'callback> {
    pub(super) spot: f64,
    pub(super) grid: &'a VanillaOptionGridRequest,
    pub(super) solved_maturity: &'a SolvedMaturityLaw,
    pub(super) boundary_state: janiform_model_kernels::hawkes::HawkesExcitationState,
    pub(super) pricing_thread_pool: Option<&'a rayon::ThreadPool>,
    pub(super) cpu_count: Option<usize>,
    pub(super) maturity_index: usize,
    pub(super) total_maturities: usize,
    pub(super) observer: &'observer mut Option<&'callback mut dyn FnMut(PricingProgressEvent)>,
}

pub(super) fn price_strike_row(input: StrikeRowPricingInput<'_, '_, '_>) -> PricedStrikeRow {
    let StrikeRowPricingInput {
        spot,
        grid,
        solved_maturity,
        boundary_state,
        pricing_thread_pool,
        cpu_count,
        maturity_index,
        total_maturities,
        observer,
    } = input;

    let strike_count = grid.strikes.len();

    let maturity_year_fraction = intraday_year_fraction(solved_maturity.maturity_interval);

    let forward_price =
        spot * ((grid.risk_free_rate - grid.dividend_yield) * maturity_year_fraction).exp();

    let discount_factor = (-grid.risk_free_rate * maturity_year_fraction).exp();

    let discounted_spot = discount_factor * forward_price;

    let boundary_state_norm_row_value = Some(
        (boundary_state.y_plus_plus * boundary_state.y_plus_plus
            + boundary_state.y_plus_minus * boundary_state.y_plus_minus
            + boundary_state.y_minus_plus * boundary_state.y_minus_plus
            + boundary_state.y_minus_minus * boundary_state.y_minus_minus)
            .sqrt(),
    );

    if strike_count > 0 {
        emit_pricing_progress(
            observer,
            PricingProgressEvent::strike_pricing_progress(
                0,
                strike_count,
                Some(maturity_index),
                Some(total_maturities),
                Some(0),
                strike_count,
            ),
        );
    }

    let price_strike = |strike: f64| {
        let projection_payoff_type = spectral_projection_payoff_type(grid.payoff_type);

        let discounted_call_price = cos_price_from_terminal_characteristic_function(
            spot,
            strike,
            projection_payoff_type,
            grid.risk_free_rate,
            solved_maturity,
        );

        let discounted_strike = discount_factor * strike;

        let model_price = discounted_call_price.and_then(|price| {
            vanilla_payoff_price_from_discounted_call(
                grid.payoff_type,
                price,
                discounted_spot,
                discounted_strike,
            )
        });

        let market_inputs = VanillaOptionPricingMarketInputs::new_single_node(
            spot,
            strike,
            solved_maturity.maturity_interval,
            grid.risk_free_rate,
            grid.dividend_yield,
        );

        match model_price {
            Some(price) if price.is_finite() => {
                let (lower_bound, upper_bound) = intraday_black_scholes_no_arbitrage_price_bounds(
                    &market_inputs,
                    grid.payoff_type,
                );

                let bounded_price = price.clamp(lower_bound, upper_bound);
                let bounded_price_sensitivity = match solved_maturity
                    .characteristic_function_sensitivities
                    .as_ref()
                {
                    Some(_) => match cos_price_sensitivity_from_terminal_characteristic_function(
                        spot,
                        strike,
                        projection_payoff_type,
                        grid.risk_free_rate,
                        solved_maturity,
                    ) {
                        Ok(raw_price_sensitivity) => Some(resolve_bounded_node_price_sensitivity(
                            raw_price_sensitivity,
                            price,
                            bounded_price,
                        )),
                        Err(_) => None,
                    },
                    None => None,
                };

                let iv_outcome = invert_spectral_implied_volatility(
                    &market_inputs,
                    grid.payoff_type,
                    bounded_price,
                );

                (
                    Some(bounded_price),
                    iv_outcome.implied_volatility,
                    Some(iv_outcome.inversion_status),
                    VanillaOptionPricingStatus::Succeeded,
                    None,
                    None,
                    Some(forward_price),
                    Some(discount_factor),
                    boundary_state_norm_row_value,
                    bounded_price_sensitivity,
                )
            }
            _ => (
                None,
                None,
                None,
                VanillaOptionPricingStatus::Failed,
                Some(VanillaOptionPricingFailureStage::FourierPricing),
                Some("spectral strike valuation returned a non-finite price".to_string()),
                None,
                None,
                None,
                None,
            ),
        }
    };

    let priced_nodes = if resolved_pricing_cpu_count(cpu_count) > 1 && strike_count > 1 {
        install_pricing_parallelism(pricing_thread_pool, || {
            grid.strikes
                .par_iter()
                .copied()
                .enumerate()
                .map(|(_, strike)| price_strike(strike))
                .collect::<Vec<_>>()
        })
    } else {
        let mut priced_nodes = Vec::with_capacity(strike_count);

        for (strike_index, strike) in grid.strikes.iter().copied().enumerate() {
            priced_nodes.push(price_strike(strike));

            emit_pricing_progress(
                observer,
                PricingProgressEvent::strike_pricing_progress(
                    strike_index + 1,
                    strike_count,
                    Some(maturity_index),
                    Some(total_maturities),
                    Some(strike_index),
                    strike_count,
                ),
            );
        }

        priced_nodes
    };

    emit_pricing_progress(
        observer,
        PricingProgressEvent::strike_pricing_progress(
            strike_count,
            strike_count,
            Some(maturity_index),
            Some(total_maturities),
            strike_count.checked_sub(1),
            strike_count,
        ),
    );

    PricedStrikeRow {
        price_row: priced_nodes.iter().map(|node| node.0).collect(),
        implied_volatility_row: priced_nodes.iter().map(|node| node.1).collect(),
        implied_volatility_status_row: priced_nodes.iter().map(|node| node.2).collect(),
        pricing_status_row: priced_nodes.iter().map(|node| node.3).collect(),
        pricing_failure_stage_row: priced_nodes.iter().map(|node| node.4).collect(),
        pricing_failure_reason_row: priced_nodes.iter().map(|node| node.5.clone()).collect(),
        forward_price_row: vec![Some(forward_price); strike_count],
        discount_factor_row: vec![Some(discount_factor); strike_count],
        boundary_state_norm_row: vec![boundary_state_norm_row_value; strike_count],
        node_price_sensitivity_row: priced_nodes.iter().map(|node| node.9).collect(),
    }
}

pub(super) fn invert_spectral_implied_volatility(
    market_inputs: &VanillaOptionPricingMarketInputs,
    payoff_type: VanillaOptionPayoffType,
    option_price: f64,
) -> VanillaOptionImpliedVolatilityInversionOutcome {
    let (lower_bound, upper_bound) =
        intraday_black_scholes_no_arbitrage_price_bounds(market_inputs, payoff_type);

    let bounded_price = if option_price == upper_bound {
        upper_bound.next_down()
    } else if option_price == lower_bound {
        lower_bound.next_up()
    } else {
        option_price
    };

    invert_vanilla_option_implied_volatility(market_inputs, payoff_type, bounded_price)
}
