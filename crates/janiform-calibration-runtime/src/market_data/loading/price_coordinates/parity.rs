use std::collections::BTreeMap;

use janiform_engine_pricing::{intraday_discount_factor, intraday_forward_price};
use janiform_foundations::numeric::compute_sorted_median;
use janiform_foundations::time::duration_nanos_to_hours;

use crate::market_data::loading::contracts::{
    MarketDataLoadingError, MarketDataNormalizedOptionSurfaceRecord, OptionForwardParityCheck,
};

pub(super) fn parity_forward_check(
    option_records: &[MarketDataNormalizedOptionSurfaceRecord],
    valuation_time_utc_nanos: i64,
    model_forward_price: f64,
    risk_free_rate: f64,
    dividend_yield: f64,
    relative_threshold: f64,
) -> Result<Option<OptionForwardParityCheck>, MarketDataLoadingError> {
    let latest_by_node = latest_option_records_by_node(option_records, valuation_time_utc_nanos);

    let mut mid_forwards = Vec::new();
    let mut lower_forwards = Vec::new();
    let mut upper_forwards = Vec::new();

    for ((expiry_timestamp_utc_nanos, strike_bits), call_record) in &latest_by_node.calls {
        let Some(put_record) = latest_by_node
            .puts
            .get(&(*expiry_timestamp_utc_nanos, *strike_bits))
        else {
            continue;
        };

        let maturity_interval =
            maturity_interval_hours(valuation_time_utc_nanos, *expiry_timestamp_utc_nanos)?;

        let discount_factor = intraday_discount_factor(risk_free_rate, maturity_interval)
            .ok_or_else(|| invalid_parity_value("failed to resolve discount factor"))?;

        let carry_factor =
            intraday_forward_price(1.0, risk_free_rate, dividend_yield, maturity_interval)
                .ok_or_else(|| invalid_parity_value("failed to resolve carry factor"))?;

        let strike = f64::from_bits(*strike_bits);

        if let (Some(call_mid), Some(put_mid)) = (
            call_record.observed_mid_price(),
            put_record.observed_mid_price(),
        ) {
            let forward = strike + (call_mid - put_mid) / discount_factor;
            if forward.is_finite() && forward > 0.0 {
                mid_forwards.push(forward / carry_factor);
            }
        }

        if let (Some(call_bid), Some(call_ask), Some(put_bid), Some(put_ask)) = (
            call_record.bid_price,
            call_record.ask_price,
            put_record.bid_price,
            put_record.ask_price,
        ) {
            let lower = strike + (call_bid - put_ask) / discount_factor;
            let upper = strike + (call_ask - put_bid) / discount_factor;

            if lower.is_finite() && upper.is_finite() && lower > 0.0 && upper >= lower {
                lower_forwards.push(lower / carry_factor);
                upper_forwards.push(upper / carry_factor);
            }
        }
    }

    if mid_forwards.is_empty() {
        return Ok(Some(OptionForwardParityCheck {
            model_forward_price,
            observed_forward_price: None,
            absolute_delta: None,
            relative_delta: None,
            observed_bid_ask_forward_lower: None,
            observed_bid_ask_forward_upper: None,
            relative_warning_threshold: relative_threshold,
            paired_strike_count: 0,
            status: "unavailable".to_string(),
            warning: None,
        }));
    }

    mid_forwards.sort_by(f64::total_cmp);
    lower_forwards.sort_by(f64::total_cmp);
    upper_forwards.sort_by(f64::total_cmp);

    let observed_forward_price = compute_sorted_median(mid_forwards.as_slice());

    let absolute_delta = model_forward_price - observed_forward_price;
    let relative_delta = absolute_delta.abs() / observed_forward_price.abs();

    let lower =
        (!lower_forwards.is_empty()).then(|| compute_sorted_median(lower_forwards.as_slice()));
    let upper =
        (!upper_forwards.is_empty()).then(|| compute_sorted_median(upper_forwards.as_slice()));

    let outside_spread_band = match (lower, upper) {
        (Some(lower), Some(upper)) => model_forward_price < lower || model_forward_price > upper,
        _ => true,
    };

    let warning = if relative_delta > relative_threshold && outside_spread_band {
        Some(format!(
            "option valuation forward differs from observed put-call parity by {:.4}%: model_forward={}, observed_forward={}, absolute_delta={}",
            100.0 * relative_delta,
            model_forward_price,
            observed_forward_price,
            absolute_delta
        ))
    } else {
        None
    };

    Ok(Some(OptionForwardParityCheck {
        model_forward_price,
        observed_forward_price: Some(observed_forward_price),
        absolute_delta: Some(absolute_delta),
        relative_delta: Some(relative_delta),
        observed_bid_ask_forward_lower: lower,
        observed_bid_ask_forward_upper: upper,
        relative_warning_threshold: relative_threshold,
        paired_strike_count: mid_forwards.len(),
        status: if warning.is_some() {
            "warning".to_string()
        } else {
            "ok".to_string()
        },
        warning,
    }))
}

pub(super) fn validated_forward_price(
    spot: f64,
    risk_free_rate: f64,
    dividend_yield: f64,
    maturity_interval: f64,
) -> Result<f64, MarketDataLoadingError> {
    intraday_forward_price(spot, risk_free_rate, dividend_yield, maturity_interval)
        .filter(|price| price.is_finite() && *price > 0.0)
        .ok_or_else(|| invalid_parity_value("failed to resolve finite positive forward price"))
}

struct LatestOptionRecordsByNode<'a> {
    calls: BTreeMap<(i64, u64), &'a MarketDataNormalizedOptionSurfaceRecord>,
    puts: BTreeMap<(i64, u64), &'a MarketDataNormalizedOptionSurfaceRecord>,
}

fn latest_option_records_by_node(
    option_records: &[MarketDataNormalizedOptionSurfaceRecord],
    valuation_time_utc_nanos: i64,
) -> LatestOptionRecordsByNode<'_> {
    let mut calls = BTreeMap::new();
    let mut puts = BTreeMap::new();

    for record in option_records {
        if record.quote_timestamp_utc_nanos > valuation_time_utc_nanos {
            continue;
        }

        let key = (record.expiry_timestamp_utc_nanos, record.strike.to_bits());
        match record.option_type.as_str() {
            "call" => {
                calls.insert(key, record);
            }
            "put" => {
                puts.insert(key, record);
            }
            _ => {}
        }
    }

    LatestOptionRecordsByNode { calls, puts }
}

fn maturity_interval_hours(
    valuation_time_utc_nanos: i64,
    expiry_timestamp_utc_nanos: i64,
) -> Result<f64, MarketDataLoadingError> {
    let maturity_duration_nanos = expiry_timestamp_utc_nanos
        .checked_sub(valuation_time_utc_nanos)
        .ok_or_else(|| invalid_parity_value("option expiry must be after valuation time"))?;

    if maturity_duration_nanos <= 0 {
        return Err(invalid_parity_value(
            "option expiry must be after valuation time",
        ));
    }

    Ok(duration_nanos_to_hours(maturity_duration_nanos))
}

fn invalid_parity_value(message: &str) -> MarketDataLoadingError {
    MarketDataLoadingError::InvalidValue {
        field_path: "price_coordinate_resolution.parity_forward_check".to_string(),
        message: message.to_string(),
    }
}
