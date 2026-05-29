use chrono::{DateTime, Utc};
use janiform::calibration::current_state::CurrentStateEstimationBatchProgressEvent;
use janiform::calibration::market_data::{
    MarketDataNormalizedOptionSurfaceRecord, MarketDataTranslationTickRecord,
};
use janiform::model::hawkes::HawkesEventSide;
use janiform_calibration_runtime::{
    CurrentStateEstimationBatchInput, estimate_current_states_from_observed_data_with_observer,
};
use janiform_terminal_ui::ProgressAdapter;

use crate::error::WorkflowError;
use crate::model::{fitted_model_context, model_reference_price};

use super::super::validation::pricing_validation_error;
use super::pricing_spot::resolve_snapshot_pricing_spots;
use super::progress::emit_valuation_state_progress;
use super::types::{
    DEFAULT_PRICING_INTERVAL_DURATION_NANOS, ObservedOptionsPricingRequest,
    ObservedTickStateColumns, PreparedObservedSnapshot, PreparedSnapshotGroup,
};

pub(super) fn prepare_observed_snapshots(
    request: &ObservedOptionsPricingRequest,
    snapshot_specs: Vec<(i64, Vec<PreparedSnapshotGroup>)>,
    option_records: &[MarketDataNormalizedOptionSurfaceRecord],
    tick_records: &[MarketDataTranslationTickRecord],
    option_valuation_spot: Option<f64>,
    option_valuation_tick_records: Option<&[MarketDataTranslationTickRecord]>,
    progress_adapter: &mut Option<&mut ProgressAdapter>,
) -> Result<Vec<PreparedObservedSnapshot>, WorkflowError> {
    if snapshot_specs.is_empty() {
        return Err(pricing_validation_error(
            "valuation_time_utc",
            "must include at least one valuation time",
        ));
    }

    let valuation_times = snapshot_specs
        .iter()
        .map(|(valuation_time_utc_nanos, _)| *valuation_time_utc_nanos)
        .collect::<Vec<_>>();

    let pricing_spots = resolve_snapshot_pricing_spots(
        option_records,
        snapshot_specs.as_slice(),
        request.risk_free_rate,
        request.dividend_yield,
    )?;

    let option_valuation_spots = option_valuation_tick_records
        .map(|tick_records| {
            option_valuation_spots_at_times(tick_records, valuation_times.as_slice())
        })
        .transpose()?;

    let valuation_count = valuation_times.len();

    let final_valuation_time_utc_nanos = valuation_times[valuation_times.len() - 1];
    let final_valuation_time_utc =
        DateTime::<Utc>::from_timestamp_nanos(final_valuation_time_utc_nanos);

    let context = fitted_model_context(
        &request.model_payload,
        final_valuation_time_utc,
        DEFAULT_PRICING_INTERVAL_DURATION_NANOS,
        1,
    )?;

    let reference_price = model_reference_price(&context.model)?;

    let tick_state_columns =
        observed_tick_state_columns(tick_records, final_valuation_time_utc_nanos)?;

    let mut progress_error = None::<WorkflowError>;
    let mut progress_observer = |event: CurrentStateEstimationBatchProgressEvent| {
        if progress_error.is_some() {
            return;
        }

        if let Err(source) = emit_valuation_state_progress(progress_adapter, event) {
            progress_error = Some(source);
        }
    };

    let state_items = estimate_current_states_from_observed_data_with_observer(
        CurrentStateEstimationBatchInput {
            kernel_parameters: context
                .fitted_parameters
                .hawkes_fitted_parameters
                .kernel_parameters,
            long_variance_parameters: context
                .fitted_parameters
                .return_components_fitted_parameters
                .long_variance_parameters,
            // Intentional copy because the batch estimator owns validated
            // fitted parameters while the workflow context remains reusable.
            mean_reversion_fitted_parameters: context
                .fitted_parameters
                .return_components_fitted_parameters
                .mean_reversion_fitted_parameters
                .clone(),
            reference_price,
            state_times_utc_nanos: valuation_times,
            observation_timestamp_utc_nanos: tick_state_columns.timestamp_utc_nanos,
            observation_trade_prices: tick_state_columns.trade_prices,
            observation_trade_sides: tick_state_columns.trade_sides,
        },
        Some(&mut progress_observer),
    )
    .map_err(|source| pricing_validation_error("observation_data", source.to_string()))?;

    if let Some(progress_error) = progress_error {
        return Err(progress_error);
    }

    snapshot_specs
        .into_iter()
        .zip(state_items)
        .zip(pricing_spots)
        .zip(option_valuation_spots.unwrap_or_else(|| vec![option_valuation_spot; valuation_count]))
        .map(
            |(
                (((valuation_time_utc_nanos, snapshot_groups), state_item), pricing_spot),
                option_coordinate_spot,
            )| {
                if valuation_time_utc_nanos != state_item.state_time_utc_nanos {
                    return Err(pricing_validation_error(
                        "valuation_time_utc",
                        "prepared state order did not match valuation order",
                    ));
                }

                Ok(PreparedObservedSnapshot {
                    valuation_time_utc_nanos,
                    spot: pricing_spot
                        .or(option_coordinate_spot)
                        .unwrap_or(state_item.observed_price),
                    estimated_state: state_item.state,
                    snapshot_groups,
                })
            },
        )
        .collect()
}

fn option_valuation_spots_at_times(
    tick_records: &[MarketDataTranslationTickRecord],
    valuation_times: &[i64],
) -> Result<Vec<Option<f64>>, WorkflowError> {
    let mut tick_iterator = tick_records.iter().peekable();
    let mut latest_price = None;
    let mut prices = Vec::with_capacity(valuation_times.len());

    for valuation_time_utc_nanos in valuation_times {
        while let Some(record) = tick_iterator.peek() {
            if record.timestamp_utc_nanos > *valuation_time_utc_nanos {
                break;
            }

            if let Some(price) = record.price {
                if !price.is_finite() || price <= 0.0 {
                    return Err(pricing_validation_error(
                        "tick_data.price",
                        "raw option valuation ticks must have finite, strictly positive prices",
                    ));
                }

                latest_price = Some(price);
            }

            tick_iterator.next();
        }

        prices.push(latest_price);
    }

    Ok(prices)
}

pub(super) fn observed_tick_state_columns(
    tick_records: &[MarketDataTranslationTickRecord],
    final_valuation_time_utc_nanos: i64,
) -> Result<ObservedTickStateColumns, WorkflowError> {
    let retained_tick_count = tick_records
        .iter()
        .take_while(|record| record.timestamp_utc_nanos <= final_valuation_time_utc_nanos)
        .count();

    if retained_tick_count == 0 {
        return Err(pricing_validation_error(
            "tick_data",
            "does not contain any observations at or before valuation_time_utc",
        ));
    }

    let mut timestamp_utc_nanos = Vec::with_capacity(retained_tick_count);
    let mut trade_prices = Vec::with_capacity(retained_tick_count);
    let mut trade_sides = Vec::with_capacity(retained_tick_count);

    for record in tick_records.iter().take(retained_tick_count) {
        let price = record.price.ok_or_else(|| {
            pricing_validation_error("tick_data.price", "retained ticks must have a price")
        })?;

        if !price.is_finite() || price <= 0.0 {
            return Err(pricing_validation_error(
                "tick_data.price",
                "retained ticks must have finite, strictly positive prices",
            ));
        }

        let trade_side = match record.trade_sign.as_deref() {
            Some(raw_trade_sign) => {
                HawkesEventSide::try_from(raw_trade_sign).map_err(|source| {
                    pricing_validation_error("tick_data.trade_sign", source.to_string())
                })?
            }
            None => HawkesEventSide::Buy,
        };

        timestamp_utc_nanos.push(record.timestamp_utc_nanos);
        trade_prices.push(price);
        trade_sides.push(trade_side);
    }

    Ok(ObservedTickStateColumns {
        timestamp_utc_nanos,
        trade_prices,
        trade_sides,
    })
}
