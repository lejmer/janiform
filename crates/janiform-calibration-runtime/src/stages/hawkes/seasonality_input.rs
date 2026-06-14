use crate::stages::seasonality::SeasonalityStageNormalizedPayload;
use janiform_market_time::calendar::{CLOSED_MARKET_STATE_CODE, OPEN_MARKET_STATE_CODE};

pub(super) fn validate_hawkes_seasonality_event_vectors(
    seasonality_payload: &SeasonalityStageNormalizedPayload,
) -> Result<(), String> {
    let expected_event_count = seasonality_payload.timestamp_utc_nanos.len();
    if expected_event_count == 0 {
        return Err("seasonality timestamp event vectors must be non-empty".to_string());
    }

    validate_event_vector_length(
        "market_state_codes",
        seasonality_payload.market_state_codes.len(),
        expected_event_count,
    )?;

    validate_event_vector_length(
        "open_event_business_time_delta_exposure_hours_by_event",
        seasonality_payload
            .open_event_business_time_delta_exposure_hours_by_event
            .len(),
        expected_event_count,
    )?;

    validate_timestamp_order(seasonality_payload.timestamp_utc_nanos.as_slice())?;
    validate_market_state_codes(seasonality_payload.market_state_codes.as_slice())?;

    validate_open_event_delta_exposures(
        seasonality_payload
            .open_event_business_time_delta_exposure_hours_by_event
            .as_slice(),
    )
}

fn validate_event_vector_length(
    vector_name: &str,
    vector_length: usize,
    expected_event_count: usize,
) -> Result<(), String> {
    if vector_length != expected_event_count {
        return Err(format!(
            "seasonality {vector_name} length {vector_length} must match timestamp event count {expected_event_count}"
        ));
    }

    Ok(())
}

fn validate_timestamp_order(timestamp_utc_nanos: &[i64]) -> Result<(), String> {
    for (event_position, timestamp_pair) in timestamp_utc_nanos.windows(2).enumerate() {
        let [previous_timestamp_utc_nanos, timestamp_utc_nanos] = timestamp_pair else {
            continue;
        };

        if timestamp_utc_nanos < previous_timestamp_utc_nanos {
            return Err(format!(
                "seasonality timestamp_utc_nanos must be nondecreasing, event {} has timestamp {} before previous timestamp {}",
                event_position.saturating_add(1),
                timestamp_utc_nanos,
                previous_timestamp_utc_nanos
            ));
        }
    }

    Ok(())
}

fn validate_market_state_codes(market_state_codes: &[u8]) -> Result<(), String> {
    for (event_position, market_state_code) in market_state_codes.iter().copied().enumerate() {
        match market_state_code {
            OPEN_MARKET_STATE_CODE | CLOSED_MARKET_STATE_CODE => {}
            _ => {
                return Err(format!(
                    "seasonality market_state_codes[{event_position}] must be {OPEN_MARKET_STATE_CODE} for open or {CLOSED_MARKET_STATE_CODE} for closed, found {market_state_code}"
                ));
            }
        }
    }

    Ok(())
}

fn validate_open_event_delta_exposures(
    delta_exposure_hours_by_event: &[f64],
) -> Result<(), String> {
    for (event_position, delta_exposure_hours) in
        delta_exposure_hours_by_event.iter().copied().enumerate()
    {
        if !delta_exposure_hours.is_finite() || delta_exposure_hours < 0.0 {
            return Err(format!(
                "seasonality open_event_business_time_delta_exposure_hours_by_event[{event_position}] must be finite and non-negative, found {delta_exposure_hours}"
            ));
        }
    }

    Ok(())
}
