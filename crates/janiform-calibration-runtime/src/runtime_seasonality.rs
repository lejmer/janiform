use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Serialize;

use crate::calendar::ExchangeCalendarRegistry;
use crate::stages::seasonality::build_runtime_hourly_seasonality_factors_from_components;

/// Rust-native input for compact structural runtime seasonality materialization.
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeStructuralSeasonalityMaterializationInput {
    pub exchange_code: String,
    pub intraday_minutes_per_bin: u16,
    pub intraday_factor_by_bin: Vec<f64>,
    pub day_of_week_factor: Vec<f64>,
    pub day_of_year_factor: Vec<f64>,
    pub range_start_utc_nanos: i64,
    pub range_end_utc_nanos: i64,
    pub interval_duration_nanos: i64,
}

/// Rust-native output for compact structural runtime seasonality materialization.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RuntimeStructuralSeasonalityMaterializationOutput {
    pub interval_start_utc_nanos: Vec<i64>,
    pub seasonality_factors: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeStructuralSeasonalityMaterializationError {
    FactorMaterializationFailed {
        reason: String,
    },
    IntervalGridOverflow {
        interval_start_utc_nanos: i64,
        interval_duration_nanos: i64,
    },
}

impl Display for RuntimeStructuralSeasonalityMaterializationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FactorMaterializationFailed { reason } => write!(
                formatter,
                "runtime structural seasonality materialization failed: {reason}"
            ),
            Self::IntervalGridOverflow {
                interval_start_utc_nanos,
                interval_duration_nanos,
            } => write!(
                formatter,
                "runtime structural seasonality interval grid overflowed after interval_start_utc_nanos={interval_start_utc_nanos} with interval_duration_nanos={interval_duration_nanos}"
            ),
        }
    }
}

impl Error for RuntimeStructuralSeasonalityMaterializationError {}

pub fn materialize_runtime_structural_seasonality(
    calendar_registry: &ExchangeCalendarRegistry,
    input: RuntimeStructuralSeasonalityMaterializationInput,
) -> Result<
    RuntimeStructuralSeasonalityMaterializationOutput,
    RuntimeStructuralSeasonalityMaterializationError,
> {
    let seasonality_factors = build_runtime_hourly_seasonality_factors_from_components(
        calendar_registry,
        input.exchange_code.as_str(),
        input.intraday_minutes_per_bin,
        input.intraday_factor_by_bin.as_slice(),
        input.day_of_week_factor.as_slice(),
        input.day_of_year_factor.as_slice(),
        input.range_start_utc_nanos,
        input.range_end_utc_nanos,
        input.interval_duration_nanos,
    )
    .map_err(|reason| {
        RuntimeStructuralSeasonalityMaterializationError::FactorMaterializationFailed { reason }
    })?;

    let mut interval_start_utc_nanos: Vec<i64> = Vec::with_capacity(seasonality_factors.len());
    let mut current_interval_start_utc_nanos = input.range_start_utc_nanos;

    while current_interval_start_utc_nanos < input.range_end_utc_nanos {
        interval_start_utc_nanos.push(current_interval_start_utc_nanos);
        current_interval_start_utc_nanos = current_interval_start_utc_nanos
            .checked_add(input.interval_duration_nanos)
            .ok_or(
                RuntimeStructuralSeasonalityMaterializationError::IntervalGridOverflow {
                    interval_start_utc_nanos: current_interval_start_utc_nanos,
                    interval_duration_nanos: input.interval_duration_nanos,
                },
            )?;
    }

    Ok(RuntimeStructuralSeasonalityMaterializationOutput {
        interval_start_utc_nanos,
        seasonality_factors,
    })
}
