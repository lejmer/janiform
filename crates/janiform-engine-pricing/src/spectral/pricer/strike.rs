use rayon::iter::{IndexedParallelIterator, IntoParallelRefIterator, ParallelIterator};

use super::domain::resolve_bounded_node_price_sensitivity;
use super::interval_operator::SolvedMaturityLaw;
use super::payoff::{
    cos_price_from_terminal_characteristic_function,
    cos_price_sensitivity_from_terminal_characteristic_function,
};
use super::projection::spectral_projection_payoff_type_for_forward;
use super::{
    PricingProgressEvent, VanillaOptionGridRequest, VanillaOptionImpliedVolatilityInversionOutcome,
    VanillaOptionImpliedVolatilityInversionStatus, VanillaOptionNodePriceSensitivity,
    VanillaOptionPayoffType, VanillaOptionPricingFailureStage, VanillaOptionPricingMarketInputs,
    VanillaOptionPricingStatus, emit_pricing_progress, install_pricing_parallelism,
    intraday_black_scholes_no_arbitrage_price_bounds, invert_vanilla_option_implied_volatility,
    resolved_pricing_cpu_count,
};
use crate::vanilla_payoff::vanilla_payoff_price_from_discounted_projection;
use crate::{intraday_discount_factor, intraday_forward_price};

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

struct StrikeNodePricingContext<'a> {
    spot: f64,
    grid: &'a VanillaOptionGridRequest,
    solved_maturity: &'a SolvedMaturityLaw,
    forward_price: Option<f64>,
    discount_factor: Option<f64>,
    discounted_spot: Option<f64>,
    boundary_state_norm: Option<f64>,
}

struct PricedStrikeNode {
    price: Option<f64>,
    implied_volatility: Option<f64>,
    implied_volatility_status: Option<VanillaOptionImpliedVolatilityInversionStatus>,
    pricing_status: VanillaOptionPricingStatus,
    pricing_failure_stage: Option<VanillaOptionPricingFailureStage>,
    pricing_failure_reason: Option<String>,
    boundary_state_norm: Option<f64>,
    node_price_sensitivity: Option<VanillaOptionNodePriceSensitivity>,
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

    let forward_price_row_value = intraday_forward_price(
        spot,
        grid.risk_free_rate,
        grid.dividend_yield,
        solved_maturity.maturity_interval,
    );

    let discount_factor_row_value =
        intraday_discount_factor(grid.risk_free_rate, solved_maturity.maturity_interval);

    let discounted_spot = forward_price_row_value
        .zip(discount_factor_row_value)
        .map(|(forward_price, discount_factor)| forward_price * discount_factor);

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

    let pricing_context = StrikeNodePricingContext {
        spot,
        grid,
        solved_maturity,
        forward_price: forward_price_row_value,
        discount_factor: discount_factor_row_value,
        discounted_spot,
        boundary_state_norm: boundary_state_norm_row_value,
    };

    let priced_nodes = if resolved_pricing_cpu_count(cpu_count) > 1 && strike_count > 1 {
        install_pricing_parallelism(pricing_thread_pool, || {
            grid.strikes
                .par_iter()
                .copied()
                .enumerate()
                .map(|(_, strike)| price_strike_node(&pricing_context, strike))
                .collect::<Vec<_>>()
        })
    } else {
        let mut priced_nodes = Vec::with_capacity(strike_count);

        for (strike_index, strike) in grid.strikes.iter().copied().enumerate() {
            priced_nodes.push(price_strike_node(&pricing_context, strike));

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

    build_priced_strike_row(
        priced_nodes,
        strike_count,
        forward_price_row_value,
        discount_factor_row_value,
    )
}

fn build_priced_strike_row(
    priced_nodes: Vec<PricedStrikeNode>,
    strike_count: usize,
    forward_price_row_value: Option<f64>,
    discount_factor_row_value: Option<f64>,
) -> PricedStrikeRow {
    let mut price_row = Vec::with_capacity(strike_count);
    let mut implied_volatility_row = Vec::with_capacity(strike_count);
    let mut implied_volatility_status_row = Vec::with_capacity(strike_count);
    let mut pricing_status_row = Vec::with_capacity(strike_count);
    let mut pricing_failure_stage_row = Vec::with_capacity(strike_count);
    let mut pricing_failure_reason_row = Vec::with_capacity(strike_count);
    let mut boundary_state_norm_row = Vec::with_capacity(strike_count);
    let mut node_price_sensitivity_row = Vec::with_capacity(strike_count);

    for priced_node in priced_nodes {
        price_row.push(priced_node.price);
        implied_volatility_row.push(priced_node.implied_volatility);
        implied_volatility_status_row.push(priced_node.implied_volatility_status);
        pricing_status_row.push(priced_node.pricing_status);
        pricing_failure_stage_row.push(priced_node.pricing_failure_stage);
        pricing_failure_reason_row.push(priced_node.pricing_failure_reason);
        boundary_state_norm_row.push(priced_node.boundary_state_norm);
        node_price_sensitivity_row.push(priced_node.node_price_sensitivity);
    }

    PricedStrikeRow {
        price_row,
        implied_volatility_row,
        implied_volatility_status_row,
        pricing_status_row,
        pricing_failure_stage_row,
        pricing_failure_reason_row,
        forward_price_row: vec![forward_price_row_value; strike_count],
        discount_factor_row: vec![discount_factor_row_value; strike_count],
        boundary_state_norm_row,
        node_price_sensitivity_row,
    }
}

fn price_strike_node(context: &StrikeNodePricingContext<'_>, strike: f64) -> PricedStrikeNode {
    let discounted_strike = context
        .discount_factor
        .map(|discount_factor| discount_factor * strike);
    let projection_payoff_type = context.forward_price.and_then(|forward_price| {
        spectral_projection_payoff_type_for_forward(context.grid.payoff_type, forward_price, strike)
    });

    let model_price = projection_payoff_type
        .zip(context.discounted_spot)
        .zip(discounted_strike)
        .and_then(
            |((projection_payoff_type, discounted_spot), discounted_strike)| {
                cos_price_from_terminal_characteristic_function(
                    context.spot,
                    strike,
                    projection_payoff_type,
                    context.grid.risk_free_rate,
                    context.solved_maturity,
                )
                .and_then(|price| {
                    vanilla_payoff_price_from_discounted_projection(
                        context.grid.payoff_type,
                        projection_payoff_type,
                        price,
                        discounted_spot,
                        discounted_strike,
                    )
                })
            },
        );

    let market_inputs = VanillaOptionPricingMarketInputs::new_single_node(
        context.spot,
        strike,
        context.solved_maturity.maturity_interval,
        context.grid.risk_free_rate,
        context.grid.dividend_yield,
    );

    match model_price {
        Some(price) if price.is_finite() => {
            let (lower_bound, upper_bound) = intraday_black_scholes_no_arbitrage_price_bounds(
                &market_inputs,
                context.grid.payoff_type,
            );

            let bounded_price = price.clamp(lower_bound, upper_bound);
            let bounded_price_sensitivity = bounded_node_price_sensitivity(
                context,
                strike,
                projection_payoff_type,
                price,
                bounded_price,
            );

            let iv_outcome = invert_spectral_implied_volatility(
                &market_inputs,
                context.grid.payoff_type,
                bounded_price,
            );

            PricedStrikeNode {
                price: Some(bounded_price),
                implied_volatility: iv_outcome.implied_volatility,
                implied_volatility_status: Some(iv_outcome.inversion_status),
                pricing_status: VanillaOptionPricingStatus::Succeeded,
                pricing_failure_stage: None,
                pricing_failure_reason: None,
                boundary_state_norm: context.boundary_state_norm,
                node_price_sensitivity: bounded_price_sensitivity,
            }
        }
        _ => PricedStrikeNode {
            price: None,
            implied_volatility: None,
            implied_volatility_status: None,
            pricing_status: VanillaOptionPricingStatus::Failed,
            pricing_failure_stage: Some(VanillaOptionPricingFailureStage::FourierPricing),
            pricing_failure_reason: Some(
                "spectral strike valuation returned a non-finite price".to_string(),
            ),
            boundary_state_norm: context.boundary_state_norm,
            node_price_sensitivity: None,
        },
    }
}

fn bounded_node_price_sensitivity(
    context: &StrikeNodePricingContext<'_>,
    strike: f64,
    projection_payoff_type: Option<VanillaOptionPayoffType>,
    price: f64,
    bounded_price: f64,
) -> Option<VanillaOptionNodePriceSensitivity> {
    context
        .solved_maturity
        .characteristic_function_sensitivities
        .as_ref()?;
    let projection_payoff_type = projection_payoff_type?;

    match cos_price_sensitivity_from_terminal_characteristic_function(
        context.spot,
        strike,
        projection_payoff_type,
        context.grid.risk_free_rate,
        context.solved_maturity,
    ) {
        Ok(raw_price_sensitivity) => Some(resolve_bounded_node_price_sensitivity(
            raw_price_sensitivity,
            price,
            bounded_price,
        )),
        Err(_) => None,
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
