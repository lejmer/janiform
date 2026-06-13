use serde::{Deserialize, Serialize};

use super::constants::{
    INTERVAL_FACTOR_GRID_FALL_BACK_POLICY, INTERVAL_FACTOR_GRID_INTERVAL_ALIGNMENT_POLICY,
    INTERVAL_FACTOR_GRID_INTERVAL_BOUNDARY_POLICY, INTERVAL_FACTOR_GRID_SCHEMA_VERSION,
    INTERVAL_FACTOR_GRID_SPRING_FORWARD_POLICY, INTERVAL_FACTOR_GRID_TIMEZONE_TRANSITION_POLICY,
};
use super::error::IntervalFactorGridError;
use super::types::{IntervalFactorGrid, IntervalFactorGridDiagnostics, IntervalFactorGridMetadata};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntervalFactorGridPayload {
    pub schema_version: String,
    pub exchange_code: String,
    pub exchange_timezone_name: String,
    pub request_start_utc_nanos: i64,
    pub request_end_utc_nanos: i64,
    pub interval_duration_nanos: i64,
    pub interval_indices: Vec<u64>,
    pub interval_start_utc_nanos: Vec<i64>,
    pub interval_end_utc_nanos: Vec<i64>,
    pub interval_start_exchange_local: Vec<String>,
    pub interval_end_exchange_local: Vec<String>,
    pub seasonality_factor: Vec<f64>,
    pub known_news_intensity: Vec<f64>,
    pub surprise_news_intensity: Vec<f64>,
    pub news_intensity: Vec<f64>,
    pub interval_count: usize,
    pub first_interval_start_utc_nanos: i64,
    pub last_interval_end_utc_nanos: i64,
    pub skipped_exchange_local_interval_count: usize,
    pub repeated_exchange_local_interval_count: usize,
    pub interval_boundary_policy: String,
    pub interval_alignment_policy: String,
    pub timezone_transition_policy: String,
    pub spring_forward_policy: String,
    pub fall_back_policy: String,
    pub seasonality_materialization_method: String,
    pub news_intensity_materialization_method: String,
}

pub fn load_interval_factor_grid(
    payload: IntervalFactorGridPayload,
) -> Result<IntervalFactorGrid, IntervalFactorGridError> {
    validate_exact_contract_field(
        "schema_version",
        payload.schema_version.as_str(),
        INTERVAL_FACTOR_GRID_SCHEMA_VERSION,
    )?;

    validate_exact_contract_field(
        "interval_boundary_policy",
        payload.interval_boundary_policy.as_str(),
        INTERVAL_FACTOR_GRID_INTERVAL_BOUNDARY_POLICY,
    )?;

    validate_exact_contract_field(
        "interval_alignment_policy",
        payload.interval_alignment_policy.as_str(),
        INTERVAL_FACTOR_GRID_INTERVAL_ALIGNMENT_POLICY,
    )?;

    validate_exact_contract_field(
        "timezone_transition_policy",
        payload.timezone_transition_policy.as_str(),
        INTERVAL_FACTOR_GRID_TIMEZONE_TRANSITION_POLICY,
    )?;

    validate_exact_contract_field(
        "spring_forward_policy",
        payload.spring_forward_policy.as_str(),
        INTERVAL_FACTOR_GRID_SPRING_FORWARD_POLICY,
    )?;

    validate_exact_contract_field(
        "fall_back_policy",
        payload.fall_back_policy.as_str(),
        INTERVAL_FACTOR_GRID_FALL_BACK_POLICY,
    )?;

    if payload.exchange_code.trim().is_empty() {
        return Err(IntervalFactorGridError::EmptyExchangeCode);
    }

    if payload.seasonality_materialization_method.trim().is_empty() {
        return Err(IntervalFactorGridError::EmptySeasonalityMaterializationMethod);
    }

    if payload
        .news_intensity_materialization_method
        .trim()
        .is_empty()
    {
        return Err(IntervalFactorGridError::EmptyNewsIntensityMaterializationMethod);
    }

    if payload.interval_duration_nanos <= 0 {
        return Err(IntervalFactorGridError::InvalidIntervalDurationNanos {
            value: payload.interval_duration_nanos,
        });
    }

    if payload.interval_count == 0 {
        if payload.request_start_utc_nanos > payload.request_end_utc_nanos {
            return Err(IntervalFactorGridError::NonIncreasingRange {
                range_start_utc_nanos: payload.request_start_utc_nanos,
                range_end_utc_nanos: payload.request_end_utc_nanos,
            });
        }
    } else if payload.request_start_utc_nanos >= payload.request_end_utc_nanos {
        return Err(IntervalFactorGridError::NonIncreasingRange {
            range_start_utc_nanos: payload.request_start_utc_nanos,
            range_end_utc_nanos: payload.request_end_utc_nanos,
        });
    }

    validate_interval_field_lengths(payload.interval_count, &payload)?;
    validate_interval_grid(&payload)?;
    validate_interval_values(&payload)?;

    Ok(IntervalFactorGrid {
        schema_version: INTERVAL_FACTOR_GRID_SCHEMA_VERSION,
        exchange_code: payload.exchange_code,
        exchange_timezone_name: payload.exchange_timezone_name,
        request_start_utc_nanos: payload.request_start_utc_nanos,
        request_end_utc_nanos: payload.request_end_utc_nanos,
        interval_duration_nanos: payload.interval_duration_nanos,
        interval_indices: payload.interval_indices,
        interval_start_utc_nanos: payload.interval_start_utc_nanos,
        interval_end_utc_nanos: payload.interval_end_utc_nanos,
        interval_start_exchange_local: payload.interval_start_exchange_local,
        interval_end_exchange_local: payload.interval_end_exchange_local,
        seasonality_factor: payload.seasonality_factor,
        known_news_intensity: payload.known_news_intensity,
        surprise_news_intensity: payload.surprise_news_intensity,
        news_intensity: payload.news_intensity,
        metadata: IntervalFactorGridMetadata {
            seasonality_materialization_method: payload.seasonality_materialization_method,
            news_intensity_materialization_method: payload.news_intensity_materialization_method,
            interval_boundary_policy: INTERVAL_FACTOR_GRID_INTERVAL_BOUNDARY_POLICY,
            interval_alignment_policy: INTERVAL_FACTOR_GRID_INTERVAL_ALIGNMENT_POLICY,
            timezone_transition_policy: INTERVAL_FACTOR_GRID_TIMEZONE_TRANSITION_POLICY,
            spring_forward_policy: INTERVAL_FACTOR_GRID_SPRING_FORWARD_POLICY,
            fall_back_policy: INTERVAL_FACTOR_GRID_FALL_BACK_POLICY,
        },
        diagnostics: IntervalFactorGridDiagnostics {
            interval_count: payload.interval_count,
            interval_duration_nanos: payload.interval_duration_nanos,
            first_interval_start_utc_nanos: payload.first_interval_start_utc_nanos,
            last_interval_end_utc_nanos: payload.last_interval_end_utc_nanos,
            skipped_exchange_local_interval_count: payload.skipped_exchange_local_interval_count,
            repeated_exchange_local_interval_count: payload.repeated_exchange_local_interval_count,
        },
    })
}

fn validate_exact_contract_field(
    field: &'static str,
    observed: &str,
    expected: &'static str,
) -> Result<(), IntervalFactorGridError> {
    if observed != expected {
        return Err(IntervalFactorGridError::UnexpectedContractFieldValue {
            field,
            expected,
            observed: observed.to_string(),
        });
    }

    Ok(())
}

fn validate_interval_field_lengths(
    interval_count: usize,
    payload: &IntervalFactorGridPayload,
) -> Result<(), IntervalFactorGridError> {
    for (field, actual_length) in [
        ("interval_indices", payload.interval_indices.len()),
        (
            "interval_start_utc_nanos",
            payload.interval_start_utc_nanos.len(),
        ),
        (
            "interval_end_utc_nanos",
            payload.interval_end_utc_nanos.len(),
        ),
        (
            "interval_start_exchange_local",
            payload.interval_start_exchange_local.len(),
        ),
        (
            "interval_end_exchange_local",
            payload.interval_end_exchange_local.len(),
        ),
        ("seasonality_factor", payload.seasonality_factor.len()),
        ("known_news_intensity", payload.known_news_intensity.len()),
        (
            "surprise_news_intensity",
            payload.surprise_news_intensity.len(),
        ),
        ("news_intensity", payload.news_intensity.len()),
    ] {
        if actual_length != interval_count {
            return Err(IntervalFactorGridError::IntervalFieldLengthMismatch {
                field,
                expected_length: interval_count,
                actual_length,
            });
        }
    }

    Ok(())
}

fn validate_interval_grid(
    payload: &IntervalFactorGridPayload,
) -> Result<(), IntervalFactorGridError> {
    if payload.interval_count == 0 {
        validate_interval_start_match(
            "first_interval_start_utc_nanos",
            payload.request_start_utc_nanos,
            payload.first_interval_start_utc_nanos,
        )?;

        validate_interval_start_match(
            "last_interval_end_utc_nanos",
            payload.request_end_utc_nanos,
            payload.last_interval_end_utc_nanos,
        )?;

        return Ok(());
    }

    validate_interval_start_match(
        "first_interval_start_utc_nanos",
        payload.first_interval_start_utc_nanos,
        payload.interval_start_utc_nanos[0],
    )?;

    validate_interval_start_match(
        "last_interval_end_utc_nanos",
        payload.last_interval_end_utc_nanos,
        payload.interval_end_utc_nanos[payload.interval_count - 1],
    )?;

    validate_interval_start_match(
        "request_start_utc_nanos",
        payload.request_start_utc_nanos,
        payload.interval_start_utc_nanos[0],
    )?;

    validate_interval_start_match(
        "request_end_utc_nanos",
        payload.request_end_utc_nanos,
        payload.interval_end_utc_nanos[payload.interval_count - 1],
    )?;

    for interval_position in 0..payload.interval_count {
        let expected_index = interval_position as u64;
        let actual_index = payload.interval_indices[interval_position];

        if actual_index != expected_index {
            return Err(IntervalFactorGridError::IntervalIndexMismatch {
                interval_position,
                expected_index,
                actual_index,
            });
        }

        let start_utc_nanos = payload.interval_start_utc_nanos[interval_position];
        let end_utc_nanos = payload.interval_end_utc_nanos[interval_position];

        if start_utc_nanos >= end_utc_nanos
            || end_utc_nanos - start_utc_nanos != payload.interval_duration_nanos
        {
            return Err(IntervalFactorGridError::InvalidIntervalBounds {
                interval_position,
                start_utc_nanos,
                end_utc_nanos,
                interval_duration_nanos: payload.interval_duration_nanos,
            });
        }

        if interval_position > 0 {
            let previous_interval_end_utc_nanos =
                payload.interval_end_utc_nanos[interval_position - 1];
            if previous_interval_end_utc_nanos != start_utc_nanos {
                return Err(IntervalFactorGridError::NonContiguousIntervals {
                    previous_interval_end_utc_nanos,
                    current_interval_start_utc_nanos: start_utc_nanos,
                    interval_position,
                });
            }
        }
    }

    Ok(())
}

fn validate_interval_start_match(
    field: &'static str,
    expected: i64,
    actual: i64,
) -> Result<(), IntervalFactorGridError> {
    if expected != actual {
        return Err(IntervalFactorGridError::IntervalStartMismatch {
            field,
            expected,
            actual,
        });
    }

    Ok(())
}

fn validate_interval_values(
    payload: &IntervalFactorGridPayload,
) -> Result<(), IntervalFactorGridError> {
    for (interval_index, seasonality_factor) in
        payload.seasonality_factor.iter().copied().enumerate()
    {
        if !seasonality_factor.is_finite() || seasonality_factor < 0.0 {
            return Err(IntervalFactorGridError::InvalidSeasonalityFactor {
                interval_index,
                value: seasonality_factor,
            });
        }
    }

    for (interval_index, value) in payload.known_news_intensity.iter().copied().enumerate() {
        if !value.is_finite() || value < 0.0 {
            return Err(
                IntervalFactorGridError::InvalidKnownComponentNewsIntensityFactor {
                    interval_index,
                    value,
                },
            );
        }
    }

    for (interval_index, value) in payload.surprise_news_intensity.iter().copied().enumerate() {
        if !value.is_finite() || value < 0.0 {
            return Err(
                IntervalFactorGridError::InvalidSurpriseComponentNewsIntensityFactor {
                    interval_index,
                    value,
                },
            );
        }
    }

    for (interval_index, value) in payload.news_intensity.iter().copied().enumerate() {
        if !value.is_finite() || value < 0.0 {
            return Err(IntervalFactorGridError::InvalidNewsIntensityFactor {
                interval_index,
                value,
            });
        }
    }

    Ok(())
}
