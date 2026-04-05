use serde::{Deserialize, Serialize};

use super::constants::{
    HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION, HOURLY_FACTOR_FALL_BACK_POLICY,
    HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY, HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
    HOURLY_FACTOR_SPRING_FORWARD_POLICY, HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
};
use super::error::HourlyFactorContractError;
use super::types::{
    HourlyFactorContractDiagnostics, HourlyFactorContractOutput,
    HourlyFactorMaterializationMetadata,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HourlyFactorContractPayload {
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
    pub s_h: Vec<f64>,
    pub known_component_nu_h: Vec<f64>,
    pub surprise_component_nu_h: Vec<f64>,
    pub nu_h: Vec<f64>,
    pub interval_count: usize,
    pub first_interval_start_utc_nanos: i64,
    pub last_interval_end_utc_nanos: i64,
    pub skipped_local_hour_interval_count: usize,
    pub repeated_local_hour_interval_count: usize,
    pub interval_boundary_policy: String,
    pub interval_alignment_policy: String,
    pub timezone_transition_policy: String,
    pub spring_forward_policy: String,
    pub fall_back_policy: String,
    pub seasonality_materialization_method: String,
    pub news_intensity_materialization_method: String,
}

pub fn load_hourly_factor_contract(
    payload: HourlyFactorContractPayload,
) -> Result<HourlyFactorContractOutput, HourlyFactorContractError> {
    validate_exact_contract_field(
        "schema_version",
        payload.schema_version.as_str(),
        HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
    )?;
    validate_exact_contract_field(
        "interval_boundary_policy",
        payload.interval_boundary_policy.as_str(),
        HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
    )?;
    validate_exact_contract_field(
        "interval_alignment_policy",
        payload.interval_alignment_policy.as_str(),
        HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY,
    )?;
    validate_exact_contract_field(
        "timezone_transition_policy",
        payload.timezone_transition_policy.as_str(),
        HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
    )?;
    validate_exact_contract_field(
        "spring_forward_policy",
        payload.spring_forward_policy.as_str(),
        HOURLY_FACTOR_SPRING_FORWARD_POLICY,
    )?;
    validate_exact_contract_field(
        "fall_back_policy",
        payload.fall_back_policy.as_str(),
        HOURLY_FACTOR_FALL_BACK_POLICY,
    )?;

    if payload.exchange_code.trim().is_empty() {
        return Err(HourlyFactorContractError::EmptyExchangeCode);
    }

    if payload.seasonality_materialization_method.trim().is_empty() {
        return Err(HourlyFactorContractError::EmptySeasonalityMaterializationMethod);
    }

    if payload
        .news_intensity_materialization_method
        .trim()
        .is_empty()
    {
        return Err(HourlyFactorContractError::EmptyNewsIntensityMaterializationMethod);
    }

    if payload.interval_duration_nanos <= 0 {
        return Err(HourlyFactorContractError::InvalidIntervalDurationNanos {
            value: payload.interval_duration_nanos,
        });
    }

    if payload.interval_count == 0 {
        if payload.request_start_utc_nanos > payload.request_end_utc_nanos {
            return Err(HourlyFactorContractError::NonIncreasingRange {
                range_start_utc_nanos: payload.request_start_utc_nanos,
                range_end_utc_nanos: payload.request_end_utc_nanos,
            });
        }
    } else if payload.request_start_utc_nanos >= payload.request_end_utc_nanos {
        return Err(HourlyFactorContractError::NonIncreasingRange {
            range_start_utc_nanos: payload.request_start_utc_nanos,
            range_end_utc_nanos: payload.request_end_utc_nanos,
        });
    }

    validate_interval_field_lengths(payload.interval_count, &payload)?;
    validate_interval_grid(&payload)?;
    validate_interval_values(&payload)?;

    Ok(HourlyFactorContractOutput {
        schema_version: HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
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
        s_h: payload.s_h,
        known_component_nu_h: payload.known_component_nu_h,
        surprise_component_nu_h: payload.surprise_component_nu_h,
        nu_h: payload.nu_h,
        metadata: HourlyFactorMaterializationMetadata {
            seasonality_materialization_method: payload.seasonality_materialization_method,
            news_intensity_materialization_method: payload.news_intensity_materialization_method,
            interval_boundary_policy: HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
            interval_alignment_policy: HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY,
            timezone_transition_policy: HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
            spring_forward_policy: HOURLY_FACTOR_SPRING_FORWARD_POLICY,
            fall_back_policy: HOURLY_FACTOR_FALL_BACK_POLICY,
        },
        diagnostics: HourlyFactorContractDiagnostics {
            interval_count: payload.interval_count,
            interval_duration_nanos: payload.interval_duration_nanos,
            first_interval_start_utc_nanos: payload.first_interval_start_utc_nanos,
            last_interval_end_utc_nanos: payload.last_interval_end_utc_nanos,
            skipped_local_hour_interval_count: payload.skipped_local_hour_interval_count,
            repeated_local_hour_interval_count: payload.repeated_local_hour_interval_count,
        },
    })
}

fn validate_exact_contract_field(
    field: &'static str,
    observed: &str,
    expected: &'static str,
) -> Result<(), HourlyFactorContractError> {
    if observed != expected {
        return Err(HourlyFactorContractError::UnexpectedContractFieldValue {
            field,
            expected,
            observed: observed.to_string(),
        });
    }

    Ok(())
}

fn validate_interval_field_lengths(
    interval_count: usize,
    payload: &HourlyFactorContractPayload,
) -> Result<(), HourlyFactorContractError> {
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
        ("s_h", payload.s_h.len()),
        ("known_component_nu_h", payload.known_component_nu_h.len()),
        (
            "surprise_component_nu_h",
            payload.surprise_component_nu_h.len(),
        ),
        ("nu_h", payload.nu_h.len()),
    ] {
        if actual_length != interval_count {
            return Err(HourlyFactorContractError::IntervalFieldLengthMismatch {
                field,
                expected_length: interval_count,
                actual_length,
            });
        }
    }

    Ok(())
}

fn validate_interval_grid(
    payload: &HourlyFactorContractPayload,
) -> Result<(), HourlyFactorContractError> {
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
            return Err(HourlyFactorContractError::IntervalIndexMismatch {
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
            return Err(HourlyFactorContractError::InvalidIntervalBounds {
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
                return Err(HourlyFactorContractError::NonContiguousIntervals {
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
) -> Result<(), HourlyFactorContractError> {
    if expected != actual {
        return Err(HourlyFactorContractError::IntervalStartMismatch {
            field,
            expected,
            actual,
        });
    }

    Ok(())
}

fn validate_interval_values(
    payload: &HourlyFactorContractPayload,
) -> Result<(), HourlyFactorContractError> {
    for (interval_index, seasonality_factor) in payload.s_h.iter().copied().enumerate() {
        if !seasonality_factor.is_finite() || seasonality_factor < 0.0 {
            return Err(HourlyFactorContractError::InvalidSeasonalityFactor {
                interval_index,
                value: seasonality_factor,
            });
        }
    }

    for (interval_index, value) in payload.known_component_nu_h.iter().copied().enumerate() {
        if !value.is_finite() || value < 0.0 {
            return Err(
                HourlyFactorContractError::InvalidKnownComponentNewsIntensityFactor {
                    interval_index,
                    value,
                },
            );
        }
    }

    for (interval_index, value) in payload.surprise_component_nu_h.iter().copied().enumerate() {
        if !value.is_finite() || value < 0.0 {
            return Err(
                HourlyFactorContractError::InvalidSurpriseComponentNewsIntensityFactor {
                    interval_index,
                    value,
                },
            );
        }
    }

    for (interval_index, value) in payload.nu_h.iter().copied().enumerate() {
        if !value.is_finite() || value < 0.0 {
            return Err(HourlyFactorContractError::InvalidNewsIntensityFactor {
                interval_index,
                value,
            });
        }
    }

    Ok(())
}
