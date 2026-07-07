use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use janiform::calibration::market_data::MarketDataNormalizedOptionSurfaceRecord;
use janiform::pricing::VanillaOptionPayoffType;

use crate::error::WorkflowError;

use super::super::validation::pricing_validation_error;
use super::types::{ObservedOptionTypeFilter, PreparedSnapshotGroup, PreparedSnapshotNode};

pub(super) fn resolve_effective_valuation_time_utc_nanos(
    valuation_time_utc: Option<DateTime<Utc>>,
    option_records: &[MarketDataNormalizedOptionSurfaceRecord],
) -> Result<i64, WorkflowError> {
    if let Some(valuation_time_utc) = valuation_time_utc {
        return valuation_time_utc.timestamp_nanos_opt().ok_or_else(|| {
            pricing_validation_error(
                "valuation_time_utc",
                "timestamp is outside the supported nanosecond range",
            )
        });
    }

    let unique_quote_timestamps = option_records
        .iter()
        .map(|record| record.quote_timestamp_utc_nanos)
        .collect::<BTreeSet<_>>();

    if unique_quote_timestamps.len() == 1 {
        return unique_quote_timestamps
            .first()
            .copied()
            .ok_or_else(|| pricing_validation_error("option_surface", "must contain quotes"));
    }

    Err(pricing_validation_error(
        "valuation_time_utc",
        "is required when option_surface contains more than one quote timestamp",
    ))
}

pub(super) fn prepare_snapshot_groups(
    option_records: &[MarketDataNormalizedOptionSurfaceRecord],
    valuation_time_utc_nanos: i64,
    option_type: Option<ObservedOptionTypeFilter>,
) -> Result<Vec<PreparedSnapshotGroup>, WorkflowError> {
    let mut latest_by_node = BTreeMap::new();

    for record in option_records {
        if record.quote_timestamp_utc_nanos > valuation_time_utc_nanos {
            continue;
        }

        insert_latest_snapshot_node_from_record(&mut latest_by_node, record, option_type)?;
    }

    if latest_by_node.is_empty() {
        return Err(pricing_validation_error(
            "valuation_time_utc",
            "could not find option quotes at or before valuation_time_utc",
        ));
    }

    let mut grouped = BTreeMap::<(OptionTypeSortKey, i64), Vec<PreparedSnapshotNode>>::new();

    for node in latest_by_node.into_values() {
        grouped
            .entry((
                option_type_sort_key(node.option_type),
                node.expiry_timestamp_utc_nanos,
            ))
            .or_default()
            .push(node);
    }

    Ok(grouped
        .into_iter()
        .map(|((_option_type, expiry_timestamp_utc_nanos), mut nodes)| {
            nodes.sort_by(|left, right| left.strike.total_cmp(&right.strike));
            PreparedSnapshotGroup {
                payoff_type: nodes[0].option_type,
                expiry_timestamp_utc_nanos,
                nodes,
            }
        })
        .collect())
}

pub(super) fn selected_expiry_history(
    option_records: &[MarketDataNormalizedOptionSurfaceRecord],
    valuation_time_utc_nanos: i64,
    selected_expiry_timestamp_utc_nanos: i64,
    option_type: Option<ObservedOptionTypeFilter>,
) -> Result<Vec<PreparedSnapshotNode>, WorkflowError> {
    let mut history = Vec::new();

    for record in option_records {
        if record.quote_timestamp_utc_nanos > valuation_time_utc_nanos {
            continue;
        }

        if record.expiry_timestamp_utc_nanos != selected_expiry_timestamp_utc_nanos {
            continue;
        }

        if let Some(node) = prepared_snapshot_node_from_record(record, option_type)? {
            history.push(node);
        }
    }

    if history.is_empty() {
        return Err(pricing_validation_error(
            "valuation_time_utc",
            "could not find option quotes for the selected expiry at or before valuation_time_utc",
        ));
    }

    history.sort_by(|left, right| {
        left.quote_timestamp_utc_nanos
            .cmp(&right.quote_timestamp_utc_nanos)
            .then_with(|| left.strike.total_cmp(&right.strike))
    });

    Ok(history)
}

pub(super) fn resolve_single_expiry_surface_valuation_times(
    selected_expiry_history: &[PreparedSnapshotNode],
    valuation_time_utc_nanos: i64,
    retained_snapshot_valuation_count: usize,
    valuation_count: Option<usize>,
    use_all_valuation_times: bool,
) -> Vec<i64> {
    let full_history_valuation_times = selected_expiry_history
        .iter()
        .map(|node| node.quote_timestamp_utc_nanos)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    if use_all_valuation_times {
        return full_history_valuation_times;
    }

    let sample_count = valuation_count
        .unwrap_or(retained_snapshot_valuation_count)
        .min(full_history_valuation_times.len());

    let mut valuation_times = if sample_count == full_history_valuation_times.len() {
        full_history_valuation_times
    } else if sample_count == 1 {
        vec![valuation_time_utc_nanos]
    } else {
        let selected_positions = (0..sample_count)
            .map(|selection_index| {
                rounded_selection_position(
                    selection_index,
                    full_history_valuation_times.len(),
                    sample_count,
                )
            })
            .collect::<BTreeSet<_>>();

        selected_positions
            .into_iter()
            .map(|position| full_history_valuation_times[position])
            .collect()
    };

    if valuation_times.last().copied() != Some(valuation_time_utc_nanos)
        && let Some(last) = valuation_times.last_mut()
    {
        *last = valuation_time_utc_nanos;
    }

    valuation_times
}

fn rounded_selection_position(
    selection_index: usize,
    full_history_count: usize,
    sample_count: usize,
) -> usize {
    let numerator = selection_index * (full_history_count - 1);
    let denominator = sample_count - 1;

    (numerator + denominator / 2) / denominator
}

pub(super) fn retained_snapshot_valuation_count(
    snapshot_groups: &[PreparedSnapshotGroup],
) -> usize {
    snapshot_groups
        .iter()
        .flat_map(|group| {
            group
                .nodes
                .iter()
                .map(|node| node.quote_timestamp_utc_nanos)
        })
        .collect::<BTreeSet<_>>()
        .len()
}

pub(super) fn prepare_selected_expiry_snapshot_specs_by_valuation_times(
    history: &[PreparedSnapshotNode],
    valuation_times_utc_nanos: &[i64],
) -> Result<Vec<(i64, Vec<PreparedSnapshotGroup>)>, WorkflowError> {
    let mut snapshot_specs = Vec::with_capacity(valuation_times_utc_nanos.len());
    let mut latest_by_strike = BTreeMap::new();
    let mut history_index = 0usize;

    for &valuation_time_utc_nanos in valuation_times_utc_nanos {
        while history_index < history.len()
            && history[history_index].quote_timestamp_utc_nanos <= valuation_time_utc_nanos
        {
            let node = &history[history_index];

            // Intentional copy because each valuation slice owns the latest
            // node per strike while the selected-expiry history remains reusable.
            latest_by_strike.insert(node.strike.to_bits(), node.clone());
            history_index += 1;
        }

        if latest_by_strike.is_empty() {
            return Err(pricing_validation_error(
                "valuation_time_utc",
                "could not find option quotes for the selected expiry at or before valuation_time_utc",
            ));
        }

        let mut nodes = latest_by_strike.values().cloned().collect::<Vec<_>>();

        nodes.sort_by(|left, right| left.strike.total_cmp(&right.strike));

        snapshot_specs.push((
            valuation_time_utc_nanos,
            vec![PreparedSnapshotGroup {
                payoff_type: nodes[0].option_type,
                expiry_timestamp_utc_nanos: nodes[0].expiry_timestamp_utc_nanos,
                nodes,
            }],
        ));
    }

    Ok(snapshot_specs)
}

fn prepared_snapshot_node_from_record(
    record: &MarketDataNormalizedOptionSurfaceRecord,
    option_type: Option<ObservedOptionTypeFilter>,
) -> Result<Option<PreparedSnapshotNode>, WorkflowError> {
    let payoff_type =
        normalized_option_payoff_type(&record.option_type, "option_surface.option_type")?;

    if let Some(option_type) = option_type
        && payoff_type != option_type.payoff_type()
    {
        return Ok(None);
    }

    validate_strike(record.strike)?;

    Ok(Some(PreparedSnapshotNode {
        quote_timestamp_utc_nanos: record.quote_timestamp_utc_nanos,
        expiry_timestamp_utc_nanos: record.expiry_timestamp_utc_nanos,
        strike: record.strike,
        option_type: payoff_type,
        observed_bid_price: record.bid_price,
        observed_ask_price: record.ask_price,
        observed_mid_price: record.observed_mid_price(),
    }))
}

pub(super) fn insert_latest_snapshot_node_from_record(
    latest_by_node: &mut BTreeMap<OptionRecordKey, PreparedSnapshotNode>,
    record: &MarketDataNormalizedOptionSurfaceRecord,
    option_type: Option<ObservedOptionTypeFilter>,
) -> Result<(), WorkflowError> {
    let Some(node) = prepared_snapshot_node_from_record(record, option_type)? else {
        return Ok(());
    };

    latest_by_node.insert(
        option_record_key(
            node.option_type,
            node.expiry_timestamp_utc_nanos,
            node.strike,
        ),
        node,
    );

    Ok(())
}

fn normalized_option_payoff_type(
    raw_value: &str,
    field_path: &'static str,
) -> Result<VanillaOptionPayoffType, WorkflowError> {
    match raw_value {
        "call" => Ok(VanillaOptionPayoffType::Call),
        "put" => Ok(VanillaOptionPayoffType::Put),
        _ => Err(pricing_validation_error(
            field_path,
            "must be normalized to one of: call, put",
        )),
    }
}

fn validate_strike(strike: f64) -> Result<(), WorkflowError> {
    if !strike.is_finite() || strike <= 0.0 {
        return Err(pricing_validation_error(
            "option_surface.strike",
            "must be finite and strictly positive",
        ));
    }

    Ok(())
}

pub(super) type OptionRecordKey = (OptionTypeSortKey, i64, u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(super) enum OptionTypeSortKey {
    Call,
    Put,
}

fn option_type_sort_key(payoff_type: VanillaOptionPayoffType) -> OptionTypeSortKey {
    match payoff_type {
        VanillaOptionPayoffType::Call => OptionTypeSortKey::Call,
        VanillaOptionPayoffType::Put => OptionTypeSortKey::Put,
    }
}

fn option_record_key(
    payoff_type: VanillaOptionPayoffType,
    expiry_timestamp_utc_nanos: i64,
    strike: f64,
) -> OptionRecordKey {
    (
        option_type_sort_key(payoff_type),
        expiry_timestamp_utc_nanos,
        strike.to_bits(),
    )
}
