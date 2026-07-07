use std::collections::{BTreeMap, BTreeSet};

use janiform::calibration::market_data::MarketDataNormalizedOptionSurfaceRecord;
use janiform::pricing::{intraday_discount_factor, intraday_forward_price};
use janiform_foundations::numeric::compute_sorted_median;
use janiform_foundations::time::duration_nanos_to_hours;

use crate::error::WorkflowError;

use super::super::validation::pricing_validation_error;
use super::data::{OptionRecordKey, OptionTypeSortKey, insert_latest_snapshot_node_from_record};
use super::types::{PreparedSnapshotGroup, PreparedSnapshotNode};

pub(super) fn resolve_snapshot_pricing_spots(
    option_records: &[MarketDataNormalizedOptionSurfaceRecord],
    snapshot_specs: &[(i64, Vec<PreparedSnapshotGroup>)],
    risk_free_rate: f64,
    dividend_yield: f64,
) -> Result<Vec<Option<f64>>, WorkflowError> {
    let mut pricing_spots = vec![None; snapshot_specs.len()];
    let mut snapshot_positions = (0..snapshot_specs.len()).collect::<Vec<_>>();

    snapshot_positions.sort_by_key(|position| snapshot_specs[*position].0);

    let mut option_record_positions = (0..option_records.len()).collect::<Vec<_>>();

    option_record_positions.sort_by(|left_position, right_position| {
        let left_record = &option_records[*left_position];
        let right_record = &option_records[*right_position];

        left_record
            .quote_timestamp_utc_nanos
            .cmp(&right_record.quote_timestamp_utc_nanos)
            .then_with(|| {
                left_record
                    .source_row_number
                    .cmp(&right_record.source_row_number)
            })
    });

    let mut latest_by_node = BTreeMap::new();
    let mut option_record_position = 0usize;

    for snapshot_position in snapshot_positions {
        let (valuation_time_utc_nanos, snapshot_groups) = &snapshot_specs[snapshot_position];

        while option_record_position < option_record_positions.len() {
            let record = &option_records[option_record_positions[option_record_position]];
            if record.quote_timestamp_utc_nanos > *valuation_time_utc_nanos {
                break;
            }

            insert_latest_snapshot_node_from_record(&mut latest_by_node, record, None)?;

            option_record_position += 1;
        }

        pricing_spots[snapshot_position] = put_call_parity_pricing_spot(
            &latest_by_node,
            snapshot_groups.as_slice(),
            *valuation_time_utc_nanos,
            risk_free_rate,
            dividend_yield,
        )?;
    }

    Ok(pricing_spots)
}

fn put_call_parity_pricing_spot(
    latest_by_node: &BTreeMap<OptionRecordKey, PreparedSnapshotNode>,
    snapshot_groups: &[PreparedSnapshotGroup],
    valuation_time_utc_nanos: i64,
    risk_free_rate: f64,
    dividend_yield: f64,
) -> Result<Option<f64>, WorkflowError> {
    let mut parity_spots = Vec::new();

    for expiry_timestamp_utc_nanos in snapshot_groups
        .iter()
        .map(|group| group.expiry_timestamp_utc_nanos)
        .collect::<BTreeSet<_>>()
    {
        let maturity_interval =
            maturity_interval_hours(valuation_time_utc_nanos, expiry_timestamp_utc_nanos)?;

        let discount_factor = intraday_discount_factor(risk_free_rate, maturity_interval)
            .ok_or_else(|| {
                pricing_validation_error(
                    "option_surface.put_call_parity",
                    "failed to resolve finite positive discount factor",
                )
            })?;

        let carry_factor =
            intraday_forward_price(1.0, risk_free_rate, dividend_yield, maturity_interval)
                .ok_or_else(|| {
                    pricing_validation_error(
                        "option_surface.put_call_parity",
                        "failed to resolve finite positive carry factor",
                    )
                })?;

        let (call_mid_by_strike, put_mid_by_strike) =
            paired_mid_prices_for_expiry(latest_by_node, expiry_timestamp_utc_nanos);

        for (strike_bits, call_mid_price) in call_mid_by_strike {
            let Some(put_mid_price) = put_mid_by_strike.get(&strike_bits).copied() else {
                continue;
            };

            let strike = f64::from_bits(strike_bits);

            let parity_forward_price = strike + (call_mid_price - put_mid_price) / discount_factor;
            let parity_spot = parity_forward_price / carry_factor;

            if parity_spot.is_finite() && parity_spot > 0.0 {
                parity_spots.push(parity_spot);
            }
        }
    }

    if parity_spots.is_empty() {
        return Ok(None);
    }

    parity_spots.sort_by(f64::total_cmp);

    Ok(Some(compute_sorted_median(parity_spots.as_slice())))
}

fn paired_mid_prices_for_expiry(
    latest_by_node: &BTreeMap<OptionRecordKey, PreparedSnapshotNode>,
    expiry_timestamp_utc_nanos: i64,
) -> (BTreeMap<u64, f64>, BTreeMap<u64, f64>) {
    let mut call_mid_by_strike = BTreeMap::new();
    let mut put_mid_by_strike = BTreeMap::new();

    for ((option_type, node_expiry_timestamp_utc_nanos, strike_bits), node) in latest_by_node {
        if *node_expiry_timestamp_utc_nanos != expiry_timestamp_utc_nanos {
            continue;
        }

        let Some(mid_price) = node
            .observed_mid_price
            .filter(|price| price.is_finite() && *price >= 0.0)
        else {
            continue;
        };

        match option_type {
            OptionTypeSortKey::Call => {
                call_mid_by_strike.insert(*strike_bits, mid_price);
            }
            OptionTypeSortKey::Put => {
                put_mid_by_strike.insert(*strike_bits, mid_price);
            }
        }
    }

    (call_mid_by_strike, put_mid_by_strike)
}

fn maturity_interval_hours(
    valuation_time_utc_nanos: i64,
    expiry_timestamp_utc_nanos: i64,
) -> Result<f64, WorkflowError> {
    let maturity_duration_nanos = expiry_timestamp_utc_nanos
        .checked_sub(valuation_time_utc_nanos)
        .ok_or_else(|| {
            pricing_validation_error(
                "option_surface.expiry_timestamp_utc_nanos",
                "must be after valuation_time_utc",
            )
        })?;

    if maturity_duration_nanos <= 0 {
        return Err(pricing_validation_error(
            "option_surface.expiry_timestamp_utc_nanos",
            "must be after valuation_time_utc",
        ));
    }

    Ok(duration_nanos_to_hours(maturity_duration_nanos))
}
