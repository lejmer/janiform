use janiform::pricing::{
    VanillaOptionImpliedVolatilityInversionStatus, VanillaOptionPayoffType,
    VanillaOptionPricingMarketInputs, intraday_year_fraction,
    invert_vanilla_option_implied_volatility, vanilla_option_terminal_payoff,
};
use janiform_foundations::numeric::arithmetic_mean;

use crate::error::WorkflowError;

use super::super::super::validation::pricing_validation_error;
use super::super::types::{
    ObservedOptionsPricingNode, ObservedOptionsSnapshotResult, PreparedObservedSnapshot,
};
use super::types::ObservedOptionsPhysicalDistributionDiagnosticsNode;

pub(super) fn build_physical_distribution_nodes(
    prepared_snapshots: &[PreparedObservedSnapshot],
    snapshot_results: &[ObservedOptionsSnapshotResult],
    terminal_returns_by_snapshot: &[Vec<Vec<f64>>],
    path_count: usize,
) -> Result<Vec<ObservedOptionsPhysicalDistributionDiagnosticsNode>, WorkflowError> {
    let mut nodes = Vec::new();

    for (snapshot_position, (prepared_snapshot, snapshot_result)) in prepared_snapshots
        .iter()
        .zip(snapshot_results.iter())
        .enumerate()
    {
        let group_returns = terminal_returns_by_snapshot
            .get(snapshot_position)
            .ok_or_else(|| {
                pricing_validation_error(
                    "physical_distribution_diagnostics",
                    "missing terminal returns for valuation state",
                )
            })?;

        let mut node_offset = 0usize;

        for (group_position, group) in prepared_snapshot.snapshot_groups.iter().enumerate() {
            let group_returns = group_returns.get(group_position).ok_or_else(|| {
                pricing_validation_error(
                    "physical_distribution_diagnostics",
                    "missing terminal returns for maturity group",
                )
            })?;
            for node in snapshot_result
                .nodes
                .iter()
                .skip(node_offset)
                .take(group.nodes.len())
            {
                nodes.push(build_physical_distribution_node(
                    snapshot_result.spot,
                    node,
                    group_returns.as_slice(),
                    path_count,
                )?);
                node_offset += 1;
            }
        }

        if node_offset != snapshot_result.nodes.len() {
            return Err(pricing_validation_error(
                "physical_distribution_diagnostics",
                "prepared snapshot node count did not match priced node count",
            ));
        }
    }

    Ok(nodes)
}

fn build_physical_distribution_node(
    spot: f64,
    node: &ObservedOptionsPricingNode,
    terminal_returns: &[f64],
    path_count: usize,
) -> Result<ObservedOptionsPhysicalDistributionDiagnosticsNode, WorkflowError> {
    if terminal_returns.len() != path_count {
        return Err(pricing_validation_error(
            "physical_distribution_diagnostics.path_count",
            format!(
                "expected {path_count} terminal samples, found {}",
                terminal_returns.len()
            ),
        ));
    }

    let values = compute_physical_distribution_node_values(spot, node, terminal_returns)?;

    Ok(ObservedOptionsPhysicalDistributionDiagnosticsNode {
        valuation_time_utc: node.valuation_time_utc,
        quote_timestamp_utc: node.quote_timestamp_utc,
        expiry_timestamp_utc: node.expiry_timestamp_utc,
        strike: node.strike,
        option_type: node.option_type,
        risk_free_rate: node.risk_free_rate,
        dividend_yield: node.dividend_yield,
        maturity_interval: node.maturity_interval,
        q_forward_price: values.q_forward_price,
        p_simulated_forward_price: values.p_simulated_forward_price,
        model_q_implied_volatility: node.model_implied_volatility,
        observed_q_implied_volatility: values.observed_q_implied_volatility,
        observed_q_implied_volatility_inversion_status: values
            .observed_q_implied_volatility_inversion_status,
        sample_count: terminal_returns.len(),
        physical_forward_normalized_price: values.physical_forward_normalized_price,
        physical_forward_normalized_implied_volatility: values
            .physical_forward_normalized_implied_volatility,
        physical_forward_normalized_implied_volatility_inversion_status: values
            .physical_forward_normalized_implied_volatility_inversion_status,
        market_forward_physical_price: values.market_forward_physical_price,
        market_forward_physical_implied_volatility: values
            .market_forward_physical_implied_volatility,
        market_forward_physical_implied_volatility_inversion_status: values
            .market_forward_physical_implied_volatility_inversion_status,
    })
}

struct PhysicalDistributionNodeValues {
    q_forward_price: f64,
    p_simulated_forward_price: f64,
    observed_q_implied_volatility: Option<f64>,
    observed_q_implied_volatility_inversion_status:
        Option<VanillaOptionImpliedVolatilityInversionStatus>,
    physical_forward_normalized_price: f64,
    physical_forward_normalized_implied_volatility: Option<f64>,
    physical_forward_normalized_implied_volatility_inversion_status:
        VanillaOptionImpliedVolatilityInversionStatus,
    market_forward_physical_price: f64,
    market_forward_physical_implied_volatility: Option<f64>,
    market_forward_physical_implied_volatility_inversion_status:
        VanillaOptionImpliedVolatilityInversionStatus,
}

fn compute_physical_distribution_node_values(
    spot: f64,
    node: &ObservedOptionsPricingNode,
    terminal_returns: &[f64],
) -> Result<PhysicalDistributionNodeValues, WorkflowError> {
    let terminal_ratios = terminal_return_ratios(terminal_returns)?;

    let mean_terminal_ratio = required_sample_mean(
        terminal_ratios.as_slice(),
        "physical_distribution_diagnostics.path_count",
    )?;

    let p_simulated_forward_price = spot * mean_terminal_ratio;

    let maturity_year_fraction = intraday_year_fraction(node.maturity_interval);

    let q_forward_price =
        spot * ((node.risk_free_rate - node.dividend_yield) * maturity_year_fraction).exp();

    let discount_factor = (-node.risk_free_rate * maturity_year_fraction).exp();

    let physical_forward_normalized_strike = node.strike / p_simulated_forward_price;
    let physical_forward_normalized_price = mean_physical_forward_normalized_payoff(
        node.option_type,
        terminal_ratios.as_slice(),
        mean_terminal_ratio,
        physical_forward_normalized_strike,
    )?;

    let market_forward_physical_price = discount_factor
        * mean_market_forward_physical_payoff(
            node.option_type,
            spot,
            node.strike,
            terminal_ratios.as_slice(),
        )?;

    let physical_forward_normalized_inversion = invert_price(
        1.0,
        physical_forward_normalized_strike,
        node.maturity_interval,
        0.0,
        0.0,
        node.option_type,
        physical_forward_normalized_price,
    )?;

    let market_forward_physical_inversion = invert_price(
        spot,
        node.strike,
        node.maturity_interval,
        node.risk_free_rate,
        node.dividend_yield,
        node.option_type,
        market_forward_physical_price,
    )?;

    let observed_q_inversion = node
        .observed_mid_price
        .filter(|value| value.is_finite())
        .map(|observed_price| {
            invert_price(
                spot,
                node.strike,
                node.maturity_interval,
                node.risk_free_rate,
                node.dividend_yield,
                node.option_type,
                observed_price,
            )
        })
        .transpose()?;

    Ok(PhysicalDistributionNodeValues {
        q_forward_price,
        p_simulated_forward_price,
        observed_q_implied_volatility: observed_q_inversion
            .map(|inversion| inversion.implied_volatility)
            .unwrap_or(None),
        observed_q_implied_volatility_inversion_status: observed_q_inversion
            .map(|inversion| inversion.inversion_status),
        physical_forward_normalized_price,
        physical_forward_normalized_implied_volatility: physical_forward_normalized_inversion
            .implied_volatility,
        physical_forward_normalized_implied_volatility_inversion_status:
            physical_forward_normalized_inversion.inversion_status,
        market_forward_physical_price,
        market_forward_physical_implied_volatility: market_forward_physical_inversion
            .implied_volatility,
        market_forward_physical_implied_volatility_inversion_status:
            market_forward_physical_inversion.inversion_status,
    })
}

fn terminal_return_ratios(terminal_returns: &[f64]) -> Result<Vec<f64>, WorkflowError> {
    terminal_returns
        .iter()
        .copied()
        .map(|terminal_return| {
            let ratio = terminal_return.exp();
            if !ratio.is_finite() || ratio <= 0.0 {
                return Err(WorkflowError::PricingExecutionFailed {
                    message: format!(
                        "physical distribution terminal ratio must be finite and positive, found {ratio}"
                    ),
                });
            }

            Ok(ratio)
        })
        .collect()
}

fn required_sample_mean(values: &[f64], field: &'static str) -> Result<f64, WorkflowError> {
    if values.is_empty() {
        return Err(pricing_validation_error(
            field,
            "must produce at least one sample",
        ));
    }

    arithmetic_mean(values).ok_or_else(|| WorkflowError::PricingExecutionFailed {
        message: format!("{field} must produce a finite sample mean"),
    })
}

fn mean_physical_forward_normalized_payoff(
    payoff_type: VanillaOptionPayoffType,
    terminal_ratios: &[f64],
    mean_terminal_ratio: f64,
    strike: f64,
) -> Result<f64, WorkflowError> {
    let payoffs = terminal_ratios
        .iter()
        .copied()
        .map(|terminal_ratio| {
            vanilla_option_terminal_payoff(
                payoff_type,
                terminal_ratio / mean_terminal_ratio,
                strike,
            )
        })
        .collect::<Vec<_>>();

    required_sample_mean(
        payoffs.as_slice(),
        "physical_distribution_diagnostics.physical_forward_normalized_price",
    )
}

fn mean_market_forward_physical_payoff(
    payoff_type: VanillaOptionPayoffType,
    spot: f64,
    strike: f64,
    terminal_ratios: &[f64],
) -> Result<f64, WorkflowError> {
    let payoffs = terminal_ratios
        .iter()
        .copied()
        .map(|terminal_ratio| {
            vanilla_option_terminal_payoff(payoff_type, spot * terminal_ratio, strike)
        })
        .collect::<Vec<_>>();

    required_sample_mean(
        payoffs.as_slice(),
        "physical_distribution_diagnostics.market_forward_physical_price",
    )
}

fn invert_price(
    spot: f64,
    strike: f64,
    maturity_interval: f64,
    risk_free_rate: f64,
    dividend_yield: f64,
    payoff_type: VanillaOptionPayoffType,
    option_price: f64,
) -> Result<janiform::pricing::VanillaOptionImpliedVolatilityInversionOutcome, WorkflowError> {
    let market_inputs = VanillaOptionPricingMarketInputs::try_new_single_node(
        spot,
        strike,
        maturity_interval,
        risk_free_rate,
        dividend_yield,
    )
    .map_err(|source| {
        pricing_validation_error("physical_distribution_market_inputs", source.to_string())
    })?;

    Ok(invert_vanilla_option_implied_volatility(
        &market_inputs,
        payoff_type,
        option_price,
    ))
}
