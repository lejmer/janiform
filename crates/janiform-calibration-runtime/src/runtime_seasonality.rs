use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Serialize;

use crate::stages::seasonality::{
    FourierBusinessTimeClockModel, RuntimeIntervalSeasonalityFactorsBuildInput,
    build_runtime_interval_seasonality_factors_from_components,
};
use janiform_market_time::calendar::ExchangeCalendarRegistry;

/// Materialization method label for structural runtime seasonality interval factors.
pub const RUNTIME_COMPACT_SEASONALITY_MATERIALIZATION_METHOD: &str =
    "runtime_compact_seasonality_parameters_open_time_mean_renormalized.v1";

/// Rust-native input for compact structural runtime seasonality materialization.
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeStructuralSeasonalityMaterializationInput {
    /// Exchange calendar identifier used to resolve open-market exposure.
    pub exchange_code: String,
    /// Fitted Fourier business-time clock model to materialize.
    pub clock_model: FourierBusinessTimeClockModel,
    /// Inclusive UTC nanosecond start of the requested interval grid.
    pub range_start_utc_nanos: i64,
    /// Exclusive UTC nanosecond end of the requested interval grid.
    pub range_end_utc_nanos: i64,
    /// Fixed UTC interval width in nanoseconds.
    pub interval_duration_nanos: i64,
}

/// Rust-native output for compact structural runtime seasonality materialization.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RuntimeStructuralSeasonalityMaterializationOutput {
    /// Materialization method label attached to the interval factors.
    pub materialization_method: &'static str,
    /// UTC nanosecond start timestamp for each materialized interval.
    pub interval_start_utc_nanos: Vec<i64>,
    /// Open-time mean-renormalized structural seasonality factor per interval.
    pub seasonality_factors: Vec<f64>,
}

/// Errors raised while materializing structural runtime seasonality factors.
#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeStructuralSeasonalityMaterializationError {
    /// The shared business-time clock materializer rejected the input.
    FactorMaterializationFailed {
        /// Human-readable validation or materialization failure reason.
        reason: String,
    },
    /// Adding the interval duration overflowed the UTC nanosecond grid.
    IntervalGridOverflow {
        /// Last successfully considered interval start.
        interval_start_utc_nanos: i64,
        /// Requested fixed interval duration in nanoseconds.
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

/// Materialize structural Fourier business-time seasonality onto a fixed UTC interval grid.
pub fn materialize_runtime_structural_seasonality(
    calendar_registry: &ExchangeCalendarRegistry,
    input: RuntimeStructuralSeasonalityMaterializationInput,
) -> Result<
    RuntimeStructuralSeasonalityMaterializationOutput,
    RuntimeStructuralSeasonalityMaterializationError,
> {
    let seasonality_factors = build_runtime_interval_seasonality_factors_from_components(
        RuntimeIntervalSeasonalityFactorsBuildInput {
            calendar_registry,
            exchange_code: input.exchange_code.as_str(),
            clock_model: &input.clock_model,
            range_start_utc_nanos: input.range_start_utc_nanos,
            range_end_utc_nanos: input.range_end_utc_nanos,
            interval_duration_nanos: input.interval_duration_nanos,
        },
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
        materialization_method: RUNTIME_COMPACT_SEASONALITY_MATERIALIZATION_METHOD,
        interval_start_utc_nanos,
        seasonality_factors,
    })
}
