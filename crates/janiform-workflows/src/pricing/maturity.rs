use chrono::{DateTime, Utc};
use janiform::model::factors::hourly::HourlyFactorContractOutput;
use janiform::pricing::{
    HighLevelPricingMaturityResolution, HighLevelPricingMaturityResolutionRequest,
    PricingIntervalSchedule, PricingIntervalSchedulePolicy,
    derive_activity_compressed_pricing_interval_schedule,
    resolve_high_level_pricing_maturity_inputs,
};
use janiform_foundations::time::{checked_ceil_to_hour_utc_nanos, checked_floor_to_hour_utc_nanos};

use crate::error::WorkflowError;

use super::FittedModelSingleOptionPricingRequest;
use super::validation::pricing_validation_error;

pub(super) fn resolve_single_maturity(
    request: &FittedModelSingleOptionPricingRequest,
) -> Result<HighLevelPricingMaturityResolution, WorkflowError> {
    let pricing_interval_schedule_policy = match &request.pricing_interval_schedule_policy {
        PricingIntervalSchedulePolicy::AutoActivityCompressed => {
            PricingIntervalSchedulePolicy::AutoExactSparse
        }
        // Intentional copy because maturity resolution owns the policy value.
        other => other.clone(),
    };

    resolve_high_level_pricing_maturity_inputs(HighLevelPricingMaturityResolutionRequest {
        valuation_time_utc: request.valuation_time_utc.to_rfc3339(),
        expiry_timestamps_utc: request.expiry_timestamp_utc.as_ref().map(|value| {
            // Intentional copy because the native maturity request owns axes.
            vec![Some(value.clone())]
        }),
        expiry_dates: request.expiry_date.as_ref().map(|value| {
            // Intentional copy because the native maturity request owns axes.
            vec![Some(value.clone())]
        }),
        tenors: request.tenor.as_ref().map(|value| {
            // Intentional copy because the native maturity request owns axes.
            vec![Some(value.clone())]
        }),
        pricing_interval_schedule_policy,
    })
    .map_err(|source| pricing_validation_error("maturity_input", source.to_string()))
}

pub(super) fn finalize_single_maturity_resolution(
    request: &FittedModelSingleOptionPricingRequest,
    maturity_resolution: HighLevelPricingMaturityResolution,
    hourly_factor_contract: &HourlyFactorContractOutput,
) -> Result<HighLevelPricingMaturityResolution, WorkflowError> {
    finalize_maturity_resolution(
        &request.pricing_interval_schedule_policy,
        maturity_resolution,
        hourly_factor_contract,
    )
}

pub(super) fn finalize_maturity_resolution(
    pricing_interval_schedule_policy: &PricingIntervalSchedulePolicy,
    maturity_resolution: HighLevelPricingMaturityResolution,
    hourly_factor_contract: &HourlyFactorContractOutput,
) -> Result<HighLevelPricingMaturityResolution, WorkflowError> {
    if !matches!(
        pricing_interval_schedule_policy,
        PricingIntervalSchedulePolicy::AutoActivityCompressed
    ) {
        return Ok(maturity_resolution);
    }

    let expiry_timestamps_utc_nanos = maturity_expiry_timestamps_utc_nanos(&maturity_resolution)?;

    let derived_schedule = derive_activity_compressed_pricing_interval_schedule(
        maturity_resolution
            .pricing_interval_schedule
            .valuation_time_utc_nanos,
        &expiry_timestamps_utc_nanos,
        hourly_factor_contract,
    )
    .map_err(|source| pricing_validation_error("maturity_input", source.to_string()))?;

    let maturity_intervals = derived_schedule
        .maturity_terminal_interval_indices
        .iter()
        .copied()
        .map(|terminal_interval_index| {
            derived_schedule
                .schedule
                .maturity_interval_hours(terminal_interval_index)
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| pricing_validation_error("maturity_input", source.to_string()))?;

    Ok(HighLevelPricingMaturityResolution {
        maturity_intervals,
        resolved_expiry_timestamps_utc: maturity_resolution.resolved_expiry_timestamps_utc,
        pricing_interval_schedule: derived_schedule.schedule,
        maturity_terminal_interval_indices: derived_schedule.maturity_terminal_interval_indices,
    })
}

pub(super) fn runtime_schedule_hourly_factor_contract_coverage(
    valuation_time_utc: DateTime<Utc>,
    runtime_interval_schedule: &PricingIntervalSchedule,
) -> Result<(DateTime<Utc>, DateTime<Utc>), WorkflowError> {
    let valuation_time_utc_nanos = valuation_time_utc.timestamp_nanos_opt().ok_or_else(|| {
        pricing_validation_error(
            "valuation_time_utc",
            "timestamp is outside the supported nanosecond range",
        )
    })?;

    let terminal_interval_end_utc_nanos = runtime_interval_schedule
        .interval_end_timestamps_utc_nanos
        .last()
        .copied()
        .ok_or_else(|| {
            pricing_validation_error("runtime_interval_schedule", "must not be empty")
        })?;

    let coverage_start_utc_nanos = checked_floor_to_hour_utc_nanos(valuation_time_utc_nanos)
        .ok_or_else(|| pricing_validation_error("timestamp_utc_nanos", "hour floor overflowed"))?;

    let coverage_end_utc_nanos = checked_ceil_to_hour_utc_nanos(terminal_interval_end_utc_nanos)
        .ok_or_else(|| {
            pricing_validation_error("timestamp_utc_nanos", "hour ceiling overflowed")
        })?;

    Ok((
        DateTime::<Utc>::from_timestamp_nanos(coverage_start_utc_nanos),
        DateTime::<Utc>::from_timestamp_nanos(coverage_end_utc_nanos),
    ))
}

fn maturity_expiry_timestamps_utc_nanos(
    maturity_resolution: &HighLevelPricingMaturityResolution,
) -> Result<Vec<i64>, WorkflowError> {
    maturity_resolution
        .maturity_terminal_interval_indices
        .iter()
        .copied()
        .map(|terminal_interval_index| {
            maturity_resolution
                .pricing_interval_schedule
                .interval_end_utc_nanos(terminal_interval_index)
                .ok_or_else(|| {
                    pricing_validation_error("maturity_input", "terminal interval index is invalid")
                })
        })
        .collect()
}
