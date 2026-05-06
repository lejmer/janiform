use std::collections::{BTreeMap, BTreeSet};

use janiform_calibration_artifacts::JointRefinementStageCarryInput;
use janiform_engine_pricing::{
    INTRADAY_PRICING_HOURS_PER_YEAR, PricingIntervalSchedule, VanillaOptionPayoffType,
};
use janiform_foundations::numeric::{
    ceil_integer_square_root, compute_median_f64_or_zero, compute_median_i64_or_zero,
};
use serde::{Deserialize, Serialize};

use super::contracts::{
    JointRefinementStageCurrentStateEstimationInput, JointRefinementStageExecutionFailure,
    JointRefinementStageOptionPricingInput, JointRefinementStageOptionSnapshotNode,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum JointRefinementOptionSurfaceTierKind {
    Search,
    Selection,
    Audit,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct JointRefinementCanonicalOptionQuote {
    pub(crate) source_position: usize,
    pub(crate) quote_timestamp_utc_nanos: i64,
    pub(crate) expiry_timestamp_utc_nanos: i64,
    pub(crate) terminal_interval_index: usize,
    pub(crate) maturity_interval: f64,
    pub(crate) strike: f64,
    pub(crate) payoff_type: VanillaOptionPayoffType,
    pub(crate) bid_price: f64,
    pub(crate) ask_price: f64,
    pub(crate) observed_mid_price: f64,
    pub(crate) source_observed_mid_price: f64,
    pub(crate) absolute_spread: f64,
    pub(crate) relative_spread: f64,
    pub(crate) log_forward_moneyness: f64,
    pub(crate) staleness_nanos: i64,
    pub(crate) violates_static_bounds: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct JointRefinementOptionSurfaceTierMaturity {
    pub(crate) terminal_interval_index: usize,
    pub(crate) maturity_interval: f64,
    pub(crate) expiry_timestamp_utc_nanos: i64,
    pub(crate) maturity_weight: usize,
    pub(crate) quote_indices: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct JointRefinementOptionSurfaceTier {
    pub(crate) tier_kind: JointRefinementOptionSurfaceTierKind,
    pub(crate) maturity_count: usize,
    pub(crate) maturity_representatives: Vec<JointRefinementOptionSurfaceTierMaturity>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct JointRefinementOptionSurfaceTierSet {
    pub(crate) valid_input_quote_count: usize,
    pub(crate) dropped_input_quote_count: usize,
    pub(crate) canonical_quotes: Vec<JointRefinementCanonicalOptionQuote>,
    pub(crate) search: JointRefinementOptionSurfaceTier,
    pub(crate) selection: JointRefinementOptionSurfaceTier,
    pub(crate) audit: JointRefinementOptionSurfaceTier,
}

#[derive(Debug, Clone, PartialEq)]
struct CanonicalOptionQuoteCandidate {
    source_position: usize,
    quote_timestamp_utc_nanos: i64,
    expiry_timestamp_utc_nanos: i64,
    strike: f64,
    payoff_type: VanillaOptionPayoffType,
    bid_price: f64,
    ask_price: f64,
    observed_mid_price: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct MaturityQualitySummary {
    terminal_interval_index: usize,
    maturity_interval: f64,
    expiry_timestamp_utc_nanos: i64,
    quote_indices: Vec<usize>,
    valid_quote_count: usize,
    distinct_strike_count: usize,
    two_sided_moneyness_coverage: usize,
    median_relative_spread: f64,
    static_bound_violation_share: f64,
    median_staleness_nanos: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct CanonicalOptionQuoteKey {
    payoff_type_rank: u8,
    expiry_timestamp_utc_nanos: i64,
    strike_bits: u64,
}

pub(crate) fn clean_option_pricing_input(
    option_pricing_input: &JointRefinementStageOptionPricingInput,
) -> Result<JointRefinementStageOptionPricingInput, JointRefinementStageExecutionFailure> {
    let candidates = canonical_quote_candidates(option_pricing_input);

    if candidates.is_empty() {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint option calibration requires at least one structurally valid option quote"
                        .to_string(),
            },
        );
    }

    Ok(JointRefinementStageOptionPricingInput {
        valuation_time_utc_nanos: option_pricing_input.valuation_time_utc_nanos,
        pricing_interval_schedule_policy: option_pricing_input
            .pricing_interval_schedule_policy
            .clone(),
        carry_input: option_pricing_input.carry_input.clone(),
        current_state_estimation_input: JointRefinementStageCurrentStateEstimationInput {
            state_time_utc_nanos: option_pricing_input
                .current_state_estimation_input
                .state_time_utc_nanos,
            observed_spot: option_pricing_input
                .current_state_estimation_input
                .observed_spot,
            observed_log_price: option_pricing_input
                .current_state_estimation_input
                .observed_log_price,
            observation_timestamp_utc_nanos: option_pricing_input
                .current_state_estimation_input
                .observation_timestamp_utc_nanos
                .clone(),
            observation_trade_prices: option_pricing_input
                .current_state_estimation_input
                .observation_trade_prices
                .clone(),
            observation_trade_sides: option_pricing_input
                .current_state_estimation_input
                .observation_trade_sides
                .clone(),
        },
        option_snapshot_nodes: candidates
            .into_iter()
            .map(|candidate| JointRefinementStageOptionSnapshotNode {
                quote_timestamp_utc_nanos: candidate.quote_timestamp_utc_nanos,
                expiry_timestamp_utc_nanos: candidate.expiry_timestamp_utc_nanos,
                strike: candidate.strike,
                payoff_type: candidate.payoff_type,
                bid_price: candidate.bid_price,
                ask_price: candidate.ask_price,
                observed_mid_price: canonical_mid_price(candidate.bid_price, candidate.ask_price),
            })
            .collect(),
    })
}

pub(crate) fn build_option_surface_tiers(
    original_option_pricing_input: &JointRefinementStageOptionPricingInput,
    cleaned_option_pricing_input: &JointRefinementStageOptionPricingInput,
    spot: f64,
    runtime_interval_schedule: &PricingIntervalSchedule,
) -> Result<JointRefinementOptionSurfaceTierSet, JointRefinementStageExecutionFailure> {
    let canonical_quotes = enrich_canonical_quotes(
        original_option_pricing_input,
        cleaned_option_pricing_input,
        spot,
        runtime_interval_schedule,
    )?;

    if canonical_quotes.is_empty() {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint option calibration requires at least one valid option maturity"
                        .to_string(),
            },
        );
    }

    let maturity_summaries = maturity_quality_summaries(canonical_quotes.as_slice());
    let maturity_count = maturity_summaries.len();
    let search_count = maturity_count.min(ceil_integer_square_root(maturity_count).max(4));
    let selection_count = maturity_count
        .min(ceil_integer_square_root(maturity_count.saturating_mul(4)).max(search_count + 2));

    Ok(JointRefinementOptionSurfaceTierSet {
        valid_input_quote_count: canonical_quotes.len(),
        dropped_input_quote_count: original_option_pricing_input
            .option_snapshot_nodes
            .len()
            .saturating_sub(canonical_quotes.len()),
        search: build_tier(
            JointRefinementOptionSurfaceTierKind::Search,
            search_count,
            maturity_summaries.as_slice(),
        ),
        selection: build_tier(
            JointRefinementOptionSurfaceTierKind::Selection,
            selection_count,
            maturity_summaries.as_slice(),
        ),
        audit: build_tier(
            JointRefinementOptionSurfaceTierKind::Audit,
            maturity_count,
            maturity_summaries.as_slice(),
        ),
        canonical_quotes,
    })
}

fn canonical_quote_candidates(
    option_pricing_input: &JointRefinementStageOptionPricingInput,
) -> Vec<CanonicalOptionQuoteCandidate> {
    let mut best_by_key = BTreeMap::<CanonicalOptionQuoteKey, CanonicalOptionQuoteCandidate>::new();

    for (source_position, node) in option_pricing_input
        .option_snapshot_nodes
        .iter()
        .enumerate()
    {
        if !quote_is_structurally_valid(option_pricing_input.valuation_time_utc_nanos, node) {
            continue;
        }

        let candidate = CanonicalOptionQuoteCandidate {
            source_position,
            quote_timestamp_utc_nanos: node.quote_timestamp_utc_nanos,
            expiry_timestamp_utc_nanos: node.expiry_timestamp_utc_nanos,
            strike: node.strike,
            payoff_type: node.payoff_type,
            bid_price: node.bid_price,
            ask_price: node.ask_price,
            observed_mid_price: node.observed_mid_price,
        };

        let key = CanonicalOptionQuoteKey {
            payoff_type_rank: payoff_type_rank(node.payoff_type),
            expiry_timestamp_utc_nanos: node.expiry_timestamp_utc_nanos,
            strike_bits: node.strike.to_bits(),
        };

        match best_by_key.get(&key) {
            Some(existing) if !candidate_is_preferred(&candidate, existing) => {}
            _ => {
                best_by_key.insert(key, candidate);
            }
        }
    }

    best_by_key.into_values().collect()
}

fn quote_is_structurally_valid(
    valuation_time_utc_nanos: i64,
    node: &JointRefinementStageOptionSnapshotNode,
) -> bool {
    node.quote_timestamp_utc_nanos <= valuation_time_utc_nanos
        && node.expiry_timestamp_utc_nanos > valuation_time_utc_nanos
        && node.strike.is_finite()
        && node.strike > 0.0
        && node.bid_price.is_finite()
        && node.ask_price.is_finite()
        && node.bid_price >= 0.0
        && node.ask_price > node.bid_price
}

fn candidate_is_preferred(
    candidate: &CanonicalOptionQuoteCandidate,
    existing: &CanonicalOptionQuoteCandidate,
) -> bool {
    candidate
        .quote_timestamp_utc_nanos
        .cmp(&existing.quote_timestamp_utc_nanos)
        .then_with(|| {
            (existing.ask_price - existing.bid_price)
                .total_cmp(&(candidate.ask_price - candidate.bid_price))
        })
        .then_with(|| candidate.bid_price.total_cmp(&existing.bid_price))
        .then_with(|| existing.source_position.cmp(&candidate.source_position))
        .is_gt()
}

fn enrich_canonical_quotes(
    original_option_pricing_input: &JointRefinementStageOptionPricingInput,
    cleaned_option_pricing_input: &JointRefinementStageOptionPricingInput,
    spot: f64,
    runtime_interval_schedule: &PricingIntervalSchedule,
) -> Result<Vec<JointRefinementCanonicalOptionQuote>, JointRefinementStageExecutionFailure> {
    let canonical_candidates = canonical_quote_candidates(original_option_pricing_input);
    let mut candidate_by_snapshot_key =
        BTreeMap::<CanonicalOptionQuoteKey, CanonicalOptionQuoteCandidate>::new();

    for candidate in canonical_candidates {
        candidate_by_snapshot_key.insert(
            CanonicalOptionQuoteKey {
                payoff_type_rank: payoff_type_rank(candidate.payoff_type),
                expiry_timestamp_utc_nanos: candidate.expiry_timestamp_utc_nanos,
                strike_bits: candidate.strike.to_bits(),
            },
            candidate,
        );
    }

    cleaned_option_pricing_input
        .option_snapshot_nodes
        .iter()
        .map(|node| {
            let terminal_interval_index = runtime_interval_schedule
                .terminal_interval_index_for_timestamp(node.expiry_timestamp_utc_nanos)
                .map_err(|source| {
                    JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                        error_message: format!(
                            "joint option calibration maturity tiering failed: {source}"
                        ),
                    }
                })?;
            let maturity_interval = runtime_interval_schedule
                .maturity_interval_hours(terminal_interval_index)
                .map_err(|source| {
                    JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                        error_message: format!(
                            "joint option calibration maturity tiering failed: {source}"
                        ),
                    }
                })?;
            let key = CanonicalOptionQuoteKey {
                payoff_type_rank: payoff_type_rank(node.payoff_type),
                expiry_timestamp_utc_nanos: node.expiry_timestamp_utc_nanos,
                strike_bits: node.strike.to_bits(),
            };
            let Some(source_candidate) = candidate_by_snapshot_key.get(&key) else {
                return Err(
                    JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                        error_message: "joint option calibration canonical quote lookup failed"
                            .to_string(),
                    },
                );
            };
            let observed_mid_price = canonical_mid_price(node.bid_price, node.ask_price);
            let absolute_spread = node.ask_price - node.bid_price;
            let relative_spread = absolute_spread / observed_mid_price;
            let log_forward_moneyness = log_forward_moneyness(
                node.strike,
                spot,
                original_option_pricing_input.carry_input.carry_rate,
                maturity_interval,
            );
            let staleness_nanos = original_option_pricing_input.valuation_time_utc_nanos
                - node.quote_timestamp_utc_nanos;

            Ok(JointRefinementCanonicalOptionQuote {
                source_position: source_candidate.source_position,
                quote_timestamp_utc_nanos: node.quote_timestamp_utc_nanos,
                expiry_timestamp_utc_nanos: node.expiry_timestamp_utc_nanos,
                terminal_interval_index,
                maturity_interval,
                strike: node.strike,
                payoff_type: node.payoff_type,
                bid_price: node.bid_price,
                ask_price: node.ask_price,
                observed_mid_price,
                source_observed_mid_price: source_candidate.observed_mid_price,
                absolute_spread,
                relative_spread,
                log_forward_moneyness,
                staleness_nanos,
                violates_static_bounds: violates_static_option_bounds(
                    node.payoff_type,
                    observed_mid_price,
                    node.strike,
                    spot,
                    &original_option_pricing_input.carry_input,
                    maturity_interval,
                ),
            })
        })
        .collect()
}

fn maturity_quality_summaries(
    canonical_quotes: &[JointRefinementCanonicalOptionQuote],
) -> Vec<MaturityQualitySummary> {
    let mut quote_indices_by_terminal_interval = BTreeMap::<usize, Vec<usize>>::new();
    for (quote_index, quote) in canonical_quotes.iter().enumerate() {
        quote_indices_by_terminal_interval
            .entry(quote.terminal_interval_index)
            .or_default()
            .push(quote_index);
    }

    quote_indices_by_terminal_interval
        .into_iter()
        .map(|(terminal_interval_index, quote_indices)| {
            let first_quote = &canonical_quotes[quote_indices[0]];
            let distinct_strike_count = quote_indices
                .iter()
                .map(|quote_index| canonical_quotes[*quote_index].strike.to_bits())
                .collect::<BTreeSet<_>>()
                .len();
            let has_left_moneyness = quote_indices
                .iter()
                .any(|quote_index| canonical_quotes[*quote_index].log_forward_moneyness < 0.0);
            let has_right_moneyness = quote_indices
                .iter()
                .any(|quote_index| canonical_quotes[*quote_index].log_forward_moneyness > 0.0);
            let violation_count = quote_indices
                .iter()
                .filter(|quote_index| canonical_quotes[**quote_index].violates_static_bounds)
                .count();

            MaturityQualitySummary {
                terminal_interval_index,
                maturity_interval: first_quote.maturity_interval,
                expiry_timestamp_utc_nanos: quote_indices
                    .iter()
                    .map(|quote_index| canonical_quotes[*quote_index].expiry_timestamp_utc_nanos)
                    .min()
                    .unwrap_or(first_quote.expiry_timestamp_utc_nanos),
                valid_quote_count: quote_indices.len(),
                distinct_strike_count,
                two_sided_moneyness_coverage: usize::from(has_left_moneyness)
                    + usize::from(has_right_moneyness),
                median_relative_spread: {
                    let mut relative_spreads = quote_indices
                        .iter()
                        .map(|quote_index| canonical_quotes[*quote_index].relative_spread)
                        .collect::<Vec<_>>();

                    compute_median_f64_or_zero(relative_spreads.as_mut_slice())
                },
                static_bound_violation_share: violation_count as f64 / quote_indices.len() as f64,
                median_staleness_nanos: {
                    let mut staleness_values = quote_indices
                        .iter()
                        .map(|quote_index| canonical_quotes[*quote_index].staleness_nanos)
                        .collect::<Vec<_>>();

                    compute_median_i64_or_zero(staleness_values.as_mut_slice())
                },
                quote_indices,
            }
        })
        .collect()
}

fn build_tier(
    tier_kind: JointRefinementOptionSurfaceTierKind,
    target_maturity_count: usize,
    maturity_summaries: &[MaturityQualitySummary],
) -> JointRefinementOptionSurfaceTier {
    let mut sorted_maturity_positions = (0..maturity_summaries.len()).collect::<Vec<_>>();
    sorted_maturity_positions.sort_by(|left_position, right_position| {
        maturity_summaries[*left_position]
            .maturity_interval
            .total_cmp(&maturity_summaries[*right_position].maturity_interval)
            .then_with(|| {
                maturity_summaries[*left_position]
                    .terminal_interval_index
                    .cmp(&maturity_summaries[*right_position].terminal_interval_index)
            })
    });

    let selected_positions =
        farthest_point_maturity_positions(target_maturity_count, maturity_summaries);

    let mut representatives = selected_positions
        .iter()
        .map(|selected_position| {
            let cell_positions = sorted_maturity_positions
                .iter()
                .copied()
                .filter(|maturity_position| {
                    nearest_selected_maturity_position(
                        *maturity_position,
                        selected_positions.as_slice(),
                        maturity_summaries,
                    ) == *selected_position
                })
                .collect::<Vec<_>>();
            let representative_position =
                best_maturity_quality_position(cell_positions.as_slice(), maturity_summaries);
            let representative = &maturity_summaries[representative_position];

            JointRefinementOptionSurfaceTierMaturity {
                terminal_interval_index: representative.terminal_interval_index,
                maturity_interval: representative.maturity_interval,
                expiry_timestamp_utc_nanos: representative.expiry_timestamp_utc_nanos,
                maturity_weight: cell_positions.len(),
                quote_indices: representative.quote_indices.clone(),
            }
        })
        .collect::<Vec<_>>();

    representatives.sort_by(|left, right| {
        left.maturity_interval
            .total_cmp(&right.maturity_interval)
            .then_with(|| {
                left.terminal_interval_index
                    .cmp(&right.terminal_interval_index)
            })
    });

    JointRefinementOptionSurfaceTier {
        tier_kind,
        maturity_count: representatives.len(),
        maturity_representatives: representatives,
    }
}

fn farthest_point_maturity_positions(
    target_maturity_count: usize,
    maturity_summaries: &[MaturityQualitySummary],
) -> Vec<usize> {
    if maturity_summaries.is_empty() {
        return Vec::new();
    }
    if target_maturity_count >= maturity_summaries.len() {
        return (0..maturity_summaries.len()).collect();
    }

    let shortest_position = maturity_summaries
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| left.maturity_interval.total_cmp(&right.maturity_interval))
        .map(|(position, _)| position)
        .unwrap_or(0);
    let longest_position = maturity_summaries
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| left.maturity_interval.total_cmp(&right.maturity_interval))
        .map(|(position, _)| position)
        .unwrap_or(shortest_position);
    let mut selected_positions = vec![shortest_position];

    if longest_position != shortest_position && target_maturity_count > 1 {
        selected_positions.push(longest_position);
    }

    while selected_positions.len() < target_maturity_count {
        let Some(next_position) = (0..maturity_summaries.len())
            .filter(|position| !selected_positions.contains(position))
            .max_by(|left_position, right_position| {
                nearest_log_maturity_distance(
                    *left_position,
                    selected_positions.as_slice(),
                    maturity_summaries,
                )
                .total_cmp(&nearest_log_maturity_distance(
                    *right_position,
                    selected_positions.as_slice(),
                    maturity_summaries,
                ))
                .then_with(|| {
                    maturity_summaries[*right_position]
                        .terminal_interval_index
                        .cmp(&maturity_summaries[*left_position].terminal_interval_index)
                })
            })
        else {
            break;
        };

        selected_positions.push(next_position);
    }

    selected_positions.sort_by(|left, right| {
        maturity_summaries[*left]
            .maturity_interval
            .total_cmp(&maturity_summaries[*right].maturity_interval)
    });
    selected_positions
}

fn nearest_selected_maturity_position(
    maturity_position: usize,
    selected_positions: &[usize],
    maturity_summaries: &[MaturityQualitySummary],
) -> usize {
    selected_positions
        .iter()
        .copied()
        .min_by(|left_position, right_position| {
            log_maturity_distance(maturity_position, *left_position, maturity_summaries)
                .total_cmp(&log_maturity_distance(
                    maturity_position,
                    *right_position,
                    maturity_summaries,
                ))
                .then_with(|| {
                    maturity_summaries[*left_position]
                        .terminal_interval_index
                        .cmp(&maturity_summaries[*right_position].terminal_interval_index)
                })
        })
        .unwrap_or(maturity_position)
}

fn nearest_log_maturity_distance(
    maturity_position: usize,
    selected_positions: &[usize],
    maturity_summaries: &[MaturityQualitySummary],
) -> f64 {
    selected_positions
        .iter()
        .map(|selected_position| {
            log_maturity_distance(maturity_position, *selected_position, maturity_summaries)
        })
        .fold(f64::INFINITY, f64::min)
}

fn log_maturity_distance(
    left_position: usize,
    right_position: usize,
    maturity_summaries: &[MaturityQualitySummary],
) -> f64 {
    (maturity_summaries[left_position].maturity_interval.ln()
        - maturity_summaries[right_position].maturity_interval.ln())
    .abs()
}

fn best_maturity_quality_position(
    maturity_positions: &[usize],
    maturity_summaries: &[MaturityQualitySummary],
) -> usize {
    maturity_positions
        .iter()
        .copied()
        .max_by(|left_position, right_position| {
            compare_maturity_quality(
                &maturity_summaries[*left_position],
                &maturity_summaries[*right_position],
            )
        })
        .unwrap_or(0)
}

fn compare_maturity_quality(
    left: &MaturityQualitySummary,
    right: &MaturityQualitySummary,
) -> std::cmp::Ordering {
    left.valid_quote_count
        .cmp(&right.valid_quote_count)
        .then_with(|| left.distinct_strike_count.cmp(&right.distinct_strike_count))
        .then_with(|| {
            left.two_sided_moneyness_coverage
                .cmp(&right.two_sided_moneyness_coverage)
        })
        .then_with(|| {
            right
                .median_relative_spread
                .total_cmp(&left.median_relative_spread)
        })
        .then_with(|| {
            right
                .static_bound_violation_share
                .total_cmp(&left.static_bound_violation_share)
        })
        .then_with(|| {
            right
                .median_staleness_nanos
                .cmp(&left.median_staleness_nanos)
        })
        .then_with(|| {
            right
                .expiry_timestamp_utc_nanos
                .cmp(&left.expiry_timestamp_utc_nanos)
        })
}

fn canonical_mid_price(bid_price: f64, ask_price: f64) -> f64 {
    0.5 * (bid_price + ask_price)
}

fn log_forward_moneyness(strike: f64, spot: f64, carry_rate: f64, maturity_interval: f64) -> f64 {
    let maturity_year_fraction = maturity_interval / INTRADAY_PRICING_HOURS_PER_YEAR;
    let forward = spot * (carry_rate * maturity_year_fraction).exp();
    (strike / forward).ln()
}

fn violates_static_option_bounds(
    payoff_type: VanillaOptionPayoffType,
    observed_mid_price: f64,
    strike: f64,
    spot: f64,
    carry_input: &JointRefinementStageCarryInput,
    maturity_interval: f64,
) -> bool {
    let maturity_year_fraction = maturity_interval / INTRADAY_PRICING_HOURS_PER_YEAR;
    let dividend_yield = carry_input.discount_rate - carry_input.carry_rate;
    let discounted_spot = spot * (-dividend_yield * maturity_year_fraction).exp();
    let discounted_strike = strike * (-carry_input.discount_rate * maturity_year_fraction).exp();

    match payoff_type {
        VanillaOptionPayoffType::Call => {
            observed_mid_price < (discounted_spot - discounted_strike).max(0.0)
                || observed_mid_price > discounted_spot
        }
        VanillaOptionPayoffType::Put => {
            observed_mid_price < (discounted_strike - discounted_spot).max(0.0)
                || observed_mid_price > discounted_strike
        }
    }
}

fn payoff_type_rank(payoff_type: VanillaOptionPayoffType) -> u8 {
    match payoff_type {
        VanillaOptionPayoffType::Call => 0,
        VanillaOptionPayoffType::Put => 1,
    }
}

#[cfg(test)]
mod tests {
    use crate::stages::joint_refinement::contracts::{
        JointRefinementStageCurrentStateEstimationInput, JointRefinementStageExecutionFailure,
        JointRefinementStageOptionPricingInput, JointRefinementStageOptionSnapshotNode,
    };
    use janiform_calibration_artifacts::JointRefinementStageCarryInput;
    use janiform_engine_pricing::{
        PricingIntervalSchedulePolicy, VanillaOptionPayoffType, derive_pricing_interval_schedule,
    };
    use janiform_foundations::time::NANOS_PER_HOUR;
    use janiform_model_kernels::hawkes::HawkesEventSide;

    use super::{build_option_surface_tiers, clean_option_pricing_input};

    fn option_input_with_nodes(
        nodes: Vec<JointRefinementStageOptionSnapshotNode>,
    ) -> JointRefinementStageOptionPricingInput {
        JointRefinementStageOptionPricingInput {
            valuation_time_utc_nanos: 0,
            pricing_interval_schedule_policy: PricingIntervalSchedulePolicy::AutoExactSparse,
            carry_input: JointRefinementStageCarryInput {
                discount_rate: 0.01,
                carry_rate: 0.005,
            },
            current_state_estimation_input: JointRefinementStageCurrentStateEstimationInput {
                state_time_utc_nanos: 0,
                observed_spot: 100.0,
                observed_log_price: 100.0_f64.ln(),
                observation_timestamp_utc_nanos: vec![0],
                observation_trade_prices: vec![100.0],
                observation_trade_sides: vec![HawkesEventSide::Buy],
            },
            option_snapshot_nodes: nodes,
        }
    }

    fn option_node(
        quote_timestamp_utc_nanos: i64,
        expiry_hour: i64,
        strike: f64,
        payoff_type: VanillaOptionPayoffType,
        bid_price: f64,
        ask_price: f64,
        observed_mid_price: f64,
    ) -> JointRefinementStageOptionSnapshotNode {
        JointRefinementStageOptionSnapshotNode {
            quote_timestamp_utc_nanos,
            expiry_timestamp_utc_nanos: expiry_hour * NANOS_PER_HOUR,
            strike,
            payoff_type,
            bid_price,
            ask_price,
            observed_mid_price,
        }
    }

    fn runtime_schedule(
        option_pricing_input: &JointRefinementStageOptionPricingInput,
    ) -> janiform_engine_pricing::PricingIntervalSchedule {
        derive_pricing_interval_schedule(
            option_pricing_input.valuation_time_utc_nanos,
            &option_pricing_input
                .option_snapshot_nodes
                .iter()
                .map(|node| node.expiry_timestamp_utc_nanos)
                .collect::<Vec<_>>(),
            &option_pricing_input.pricing_interval_schedule_policy,
        )
        .expect("test schedule should be valid")
        .schedule
    }

    #[test]
    fn quote_cleaning_keeps_structural_quotes_and_canonicalizes_midpoints() {
        let input = option_input_with_nodes(vec![
            option_node(-10, 1, 100.0, VanillaOptionPayoffType::Call, 1.0, 3.0, 99.0),
            option_node(-5, 1, 100.0, VanillaOptionPayoffType::Call, 1.2, 1.6, 88.0),
            option_node(1, 1, 105.0, VanillaOptionPayoffType::Call, 1.0, 2.0, 1.5),
            option_node(-5, 1, 106.0, VanillaOptionPayoffType::Call, 2.0, 1.0, 1.5),
            option_node(-5, 2, 10_000.0, VanillaOptionPayoffType::Put, 0.1, 9.0, 4.0),
        ]);

        let cleaned_input =
            clean_option_pricing_input(&input).expect("cleaning should keep valid quotes");

        assert_eq!(cleaned_input.option_snapshot_nodes.len(), 2);
        assert_eq!(
            cleaned_input.option_snapshot_nodes[0],
            option_node(-5, 1, 100.0, VanillaOptionPayoffType::Call, 1.2, 1.6, 1.4)
        );
        assert_eq!(
            cleaned_input.option_snapshot_nodes[1],
            option_node(
                -5,
                2,
                10_000.0,
                VanillaOptionPayoffType::Put,
                0.1,
                9.0,
                4.55
            )
        );
    }

    #[test]
    fn quote_cleaning_rejects_empty_structurally_valid_surface() {
        let input = option_input_with_nodes(vec![
            option_node(1, 1, 100.0, VanillaOptionPayoffType::Call, 1.0, 1.2, 1.1),
            option_node(-1, 1, 100.0, VanillaOptionPayoffType::Put, 1.2, 1.0, 1.1),
        ]);

        let error =
            clean_option_pricing_input(&input).expect_err("invalid-only surface should fail");

        assert!(matches!(
            error,
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                ref error_message,
            } if error_message.contains("structurally valid option quote")
        ));
    }

    #[test]
    fn maturity_tiers_scale_by_square_root_and_keep_audit_full() {
        let nodes = (1..=9)
            .map(|expiry_hour| {
                option_node(
                    -1,
                    expiry_hour,
                    90.0 + expiry_hour as f64,
                    VanillaOptionPayoffType::Call,
                    1.0,
                    1.4,
                    1.2,
                )
            })
            .collect::<Vec<_>>();
        let input = option_input_with_nodes(nodes);
        let cleaned_input = clean_option_pricing_input(&input).expect("quotes should clean");
        let schedule = runtime_schedule(&cleaned_input);

        let tiers = build_option_surface_tiers(&input, &cleaned_input, 100.0, &schedule)
            .expect("tiers should build");

        assert_eq!(tiers.search.maturity_count, 4);
        assert_eq!(tiers.selection.maturity_count, 6);
        assert_eq!(tiers.audit.maturity_count, 9);
        assert_eq!(
            tiers
                .search
                .maturity_representatives
                .iter()
                .map(|representative| representative.maturity_weight)
                .sum::<usize>(),
            9
        );
        assert!(
            tiers
                .search
                .maturity_representatives
                .iter()
                .any(|representative| {
                    representative.expiry_timestamp_utc_nanos == NANOS_PER_HOUR
                })
        );
        assert!(
            tiers
                .search
                .maturity_representatives
                .iter()
                .any(|representative| {
                    representative.expiry_timestamp_utc_nanos == 9 * NANOS_PER_HOUR
                })
        );
        assert!(
            tiers
                .audit
                .maturity_representatives
                .iter()
                .all(|representative| representative.maturity_weight == 1)
        );
    }

    #[test]
    fn selected_maturity_representatives_keep_all_valid_quotes() {
        let input = option_input_with_nodes(vec![
            option_node(-1, 1, 90.0, VanillaOptionPayoffType::Call, 1.0, 1.2, 1.1),
            option_node(-1, 1, 100.0, VanillaOptionPayoffType::Call, 1.0, 1.2, 1.1),
            option_node(-1, 1, 110.0, VanillaOptionPayoffType::Put, 1.0, 1.2, 1.1),
        ]);
        let cleaned_input = clean_option_pricing_input(&input).expect("quotes should clean");
        let schedule = runtime_schedule(&cleaned_input);

        let tiers = build_option_surface_tiers(&input, &cleaned_input, 100.0, &schedule)
            .expect("tiers should build");

        assert_eq!(tiers.search.maturity_representatives.len(), 1);
        assert_eq!(
            tiers.search.maturity_representatives[0].quote_indices.len(),
            3
        );
    }
}
