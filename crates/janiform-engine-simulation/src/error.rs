//! Seeded simulation error contract.

use std::error::Error;
use std::fmt::{Display, Formatter};

use chrono::{DateTime, Utc};

use crate::calendar::ExchangeCalendarResolveError;
use crate::risk::SufficientStatisticsRiskReturnComponentsError;
use janiform_model_kernels::hawkes::HawkesTransitionError;
use janiform_model_kernels::mean_reversion::{
    MeanReversionBoundaryStateValidationError, MeanReversionFittedParametersValidationError,
    MeanReversionRuntimeIntervalAnchorsValidationError,
};
use janiform_model_kernels::news::NewsProcessConfigurationError;
use janiform_model_kernels::news::{NewsMarkInterfaceError, SharedNewsEventDispatchError};
use janiform_model_kernels::nig::NigSharedPlusShiftConfigurationError;
use janiform_model_kernels::return_decomposition::ReturnDecompositionValidationError;

/// Structured simulator run failure with explicit validation diagnostics.
#[derive(Debug, Clone, PartialEq)]
pub enum SeededTickSimulationRunError {
    InvalidHorizonIntervalCount {
        value: usize,
    },
    MissingHourlyFactorIntervals,
    HourlyFactorFieldLengthMismatch {
        field_name: &'static str,
        expected_interval_count: usize,
        observed_interval_count: usize,
    },
    HourlyFactorDiagnosticsIntervalCountMismatch {
        expected_interval_count: usize,
        diagnostics_interval_count: usize,
    },
    HorizonExceedsAvailableIntervals {
        requested_interval_count: usize,
        available_interval_count: usize,
    },
    StartTimeOutOfNanosecondRange {
        value: DateTime<Utc>,
    },
    StartTimeDoesNotMatchHourlyFactorCoverage {
        start_time_utc_nanos: i64,
        first_interval_start_utc_nanos: i64,
    },
    InvalidHourlyFactorIntervalBounds {
        interval_index: usize,
        interval_start_utc_nanos: i64,
        interval_end_utc_nanos: i64,
    },
    BufferAllocationFailed {
        buffer_name: &'static str,
        requested_capacity: usize,
        detail: String,
    },
    NonConsecutiveHourlyFactorCoverage {
        previous_interval_index: usize,
        previous_interval_end_utc_nanos: i64,
        current_interval_index: usize,
        current_interval_start_utc_nanos: i64,
    },
    SeasonalityScaledEndogenousInstability {
        max_seasonality_factor: f64,
        spectral_radius_alpha_over_beta: f64,
        effective_branching_ratio: f64,
    },
    InvalidDebugTraceSamplingStride {
        value: u64,
    },
    InvalidDebugTraceChunkSize {
        value: usize,
    },
    InvalidKnownScheduledNewsTimestampUtcNanos {
        value: i64,
    },
    KnownScheduledNewsTimestampsNotStrictlyIncreasing {
        previous_timestamp_utc_nanos: i64,
        current_timestamp_utc_nanos: i64,
    },
    InvalidMeanReversionFittedParameters {
        source: MeanReversionFittedParametersValidationError,
    },
    InvalidMeanReversionRuntimeIntervalAnchors {
        source: MeanReversionRuntimeIntervalAnchorsValidationError,
    },
    InvalidMeanReversionBoundaryState {
        source: MeanReversionBoundaryStateValidationError,
    },
    MeanReversionRuntimeAnchorIntervalCountMismatch {
        expected_interval_count: usize,
        observed_interval_count: usize,
    },
    MeanReversionRuntimeAnchorAlignmentPolicyMismatch {
        expected_policy: String,
        observed_policy: String,
    },
    MeanReversionBoundaryStateStartAnchorMismatch {
        boundary_anchor_value: f64,
        runtime_start_anchor_value: f64,
    },
    MeanReversionRuntimeAnchorIntervalDeltaMismatch {
        interval_index: usize,
        expected_interval_delta: f64,
        observed_interval_delta: f64,
    },
    InvalidNigSharedPlusShiftConfiguration {
        source: NigSharedPlusShiftConfigurationError,
    },
    InvalidNewsProcessConfiguration {
        source: NewsProcessConfigurationError,
    },
    InvalidGeneratedNewsActivityCouplingMark {
        value: f64,
    },
    InvalidGeneratedNewsReturnJumpMark {
        value: f64,
    },
    NewsMarkSamplingFailure {
        interval_index: usize,
        source: NewsMarkInterfaceError,
    },
    SharedNewsDispatchFailure {
        interval_index: usize,
        source: SharedNewsEventDispatchError,
    },
    NewsMarketStateResolutionFailed {
        exchange_code: String,
        timestamp_utc_nanos: i64,
        source: ExchangeCalendarResolveError,
    },
    CalendarSessionBoundaryResolutionFailed {
        exchange_code: String,
        timezone_name: String,
        local_date: String,
        local_time: String,
    },
    InvalidAccumulatedNewsRoutingTotal {
        field: &'static str,
        value: f64,
    },
    InvalidReturnDecomposition {
        interval_index: usize,
        source: ReturnDecompositionValidationError,
    },
    TradingTimeSamplingFailure {
        interval_index: usize,
        source: janiform_model_kernels::nig::NigTradingTimeIncrementSamplingError,
    },
    InvalidReturnComponents {
        interval_index: usize,
        source: SufficientStatisticsRiskReturnComponentsError,
    },
    InvalidTransitionStep {
        source: HawkesTransitionError,
    },
}

impl Display for SeededTickSimulationRunError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidHorizonIntervalCount { value } => write!(
                formatter,
                "horizon_interval_count must be greater than 0, found {value}"
            ),
            Self::MissingHourlyFactorIntervals => write!(
                formatter,
                "hourly_factor_contract.interval_count must be greater than 0"
            ),
            Self::HourlyFactorFieldLengthMismatch {
                field_name,
                expected_interval_count,
                observed_interval_count,
            } => write!(
                formatter,
                "hourly_factor_contract.{field_name} length must match interval_count ({expected_interval_count}), found {observed_interval_count}"
            ),
            Self::HourlyFactorDiagnosticsIntervalCountMismatch {
                expected_interval_count,
                diagnostics_interval_count,
            } => write!(
                formatter,
                "hourly_factor_contract.diagnostics.interval_count must match interval_count ({expected_interval_count}), found {diagnostics_interval_count}"
            ),
            Self::HorizonExceedsAvailableIntervals {
                requested_interval_count,
                available_interval_count,
            } => write!(
                formatter,
                "horizon_interval_count ({requested_interval_count}) exceeds hourly factor interval_count ({available_interval_count})"
            ),
            Self::StartTimeOutOfNanosecondRange { value } => write!(
                formatter,
                "start_time_utc cannot be represented in nanoseconds, found {value}"
            ),
            Self::StartTimeDoesNotMatchHourlyFactorCoverage {
                start_time_utc_nanos,
                first_interval_start_utc_nanos,
            } => write!(
                formatter,
                "start_time_utc nanos ({start_time_utc_nanos}) must match hourly factor first interval start ({first_interval_start_utc_nanos})"
            ),
            Self::InvalidHourlyFactorIntervalBounds {
                interval_index,
                interval_start_utc_nanos,
                interval_end_utc_nanos,
            } => write!(
                formatter,
                "hourly_factor_contract interval {interval_index} has invalid bounds, start={interval_start_utc_nanos}, end={interval_end_utc_nanos}"
            ),
            Self::BufferAllocationFailed {
                buffer_name,
                requested_capacity,
                detail,
            } => write!(
                formatter,
                "failed to reserve buffer `{buffer_name}` with requested_capacity={requested_capacity}: {detail}"
            ),
            Self::NonConsecutiveHourlyFactorCoverage {
                previous_interval_index,
                previous_interval_end_utc_nanos,
                current_interval_index,
                current_interval_start_utc_nanos,
            } => write!(
                formatter,
                "hourly_factor_contract interval coverage must be consecutive, interval {previous_interval_index} end ({previous_interval_end_utc_nanos}) must equal interval {current_interval_index} start ({current_interval_start_utc_nanos})"
            ),
            Self::SeasonalityScaledEndogenousInstability {
                max_seasonality_factor,
                spectral_radius_alpha_over_beta,
                effective_branching_ratio,
            } => write!(
                formatter,
                "seasonality-scaled endogenous branching must remain below 1.0, found max_s_h={max_seasonality_factor}, spectral_radius_alpha_over_beta={spectral_radius_alpha_over_beta}, effective_branching_ratio={effective_branching_ratio}"
            ),
            Self::InvalidDebugTraceSamplingStride { value } => write!(
                formatter,
                "debug trace sample_every_nth_transition must be greater than 0, found {value}"
            ),
            Self::InvalidDebugTraceChunkSize { value } => write!(
                formatter,
                "debug trace max_records_per_chunk must be greater than 0, found {value}"
            ),
            Self::InvalidKnownScheduledNewsTimestampUtcNanos { value } => write!(
                formatter,
                "known_scheduled_news_event_timestamps_utc_nanos must map to valid UTC timestamp nanos, found {value}"
            ),
            Self::KnownScheduledNewsTimestampsNotStrictlyIncreasing {
                previous_timestamp_utc_nanos,
                current_timestamp_utc_nanos,
            } => write!(
                formatter,
                "known_scheduled_news_event_timestamps_utc_nanos must be strictly increasing, found previous={previous_timestamp_utc_nanos} and current={current_timestamp_utc_nanos}"
            ),
            Self::InvalidMeanReversionFittedParameters { source } => write!(
                formatter,
                "mean_reversion_fitted_parameters are invalid: {source}"
            ),
            Self::InvalidMeanReversionRuntimeIntervalAnchors { source } => write!(
                formatter,
                "mean_reversion_runtime_interval_anchors are invalid: {source}"
            ),
            Self::InvalidMeanReversionBoundaryState { source } => write!(
                formatter,
                "initial_mean_reversion_boundary_state is invalid: {source}"
            ),
            Self::MeanReversionRuntimeAnchorIntervalCountMismatch {
                expected_interval_count,
                observed_interval_count,
            } => write!(
                formatter,
                "mean_reversion_runtime_interval_anchors.end_anchor_values length must match horizon_interval_count ({expected_interval_count}), found {observed_interval_count}"
            ),
            Self::MeanReversionRuntimeAnchorAlignmentPolicyMismatch {
                expected_policy,
                observed_policy,
            } => write!(
                formatter,
                "mean_reversion_runtime_interval_anchors.interval_alignment_policy must match hourly_factor_contract.metadata.interval_alignment_policy, expected `{expected_policy}`, found `{observed_policy}`"
            ),
            Self::MeanReversionBoundaryStateStartAnchorMismatch {
                boundary_anchor_value,
                runtime_start_anchor_value,
            } => write!(
                formatter,
                "initial_mean_reversion_boundary_state.anchor_value must match mean_reversion_runtime_interval_anchors.start_anchor_value, found boundary_anchor_value={boundary_anchor_value} runtime_start_anchor_value={runtime_start_anchor_value}"
            ),
            Self::MeanReversionRuntimeAnchorIntervalDeltaMismatch {
                interval_index,
                expected_interval_delta,
                observed_interval_delta,
            } => write!(
                formatter,
                "mean_reversion_runtime_interval_anchors.interval_deltas[{interval_index}] must match the hourly factor interval delta in hours, expected {expected_interval_delta}, found {observed_interval_delta}"
            ),
            Self::InvalidNigSharedPlusShiftConfiguration { source } => write!(
                formatter,
                "nig_shared_plus_shift_configuration is invalid: {source}"
            ),
            Self::InvalidNewsProcessConfiguration { source } => {
                write!(formatter, "news_process_configuration is invalid: {source}")
            }
            Self::InvalidGeneratedNewsActivityCouplingMark { value } => write!(
                formatter,
                "generated exogenous news activity-coupling mark must be finite and non-negative, found {value}"
            ),
            Self::InvalidGeneratedNewsReturnJumpMark { value } => write!(
                formatter,
                "generated exogenous news return-jump mark must be finite, found {value}"
            ),
            Self::NewsMarkSamplingFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} news mark sampling failed: {source}"
            ),
            Self::SharedNewsDispatchFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} shared news dispatch failed: {source}"
            ),
            Self::NewsMarketStateResolutionFailed {
                exchange_code,
                timestamp_utc_nanos,
                source,
            } => write!(
                formatter,
                "failed to resolve market state for exchange `{exchange_code}` at timestamp {timestamp_utc_nanos}: {source}"
            ),
            Self::CalendarSessionBoundaryResolutionFailed {
                exchange_code,
                timezone_name,
                local_date,
                local_time,
            } => write!(
                formatter,
                "failed to resolve session boundary for exchange `{exchange_code}` in timezone `{timezone_name}` at local {local_date} {local_time}"
            ),
            Self::InvalidAccumulatedNewsRoutingTotal { field, value } => write!(
                formatter,
                "accumulated news routing total {field} must remain finite, found {value}"
            ),
            Self::InvalidReturnDecomposition {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} return decomposition is invalid: {source}"
            ),
            Self::TradingTimeSamplingFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} trading-time sampling failed: {source}"
            ),
            Self::InvalidReturnComponents {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} return components are invalid: {source}"
            ),
            Self::InvalidTransitionStep { source } => {
                write!(
                    formatter,
                    "failed to apply canonical Hawkes transition step: {source}"
                )
            }
        }
    }
}

impl Error for SeededTickSimulationRunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidMeanReversionFittedParameters { source } => Some(source),
            Self::InvalidMeanReversionRuntimeIntervalAnchors { source } => Some(source),
            Self::InvalidMeanReversionBoundaryState { source } => Some(source),
            Self::InvalidNigSharedPlusShiftConfiguration { source } => Some(source),
            Self::InvalidNewsProcessConfiguration { source } => Some(source),
            Self::NewsMarkSamplingFailure { source, .. } => Some(source),
            Self::SharedNewsDispatchFailure { source, .. } => Some(source),
            Self::NewsMarketStateResolutionFailed { source, .. } => Some(source),
            Self::InvalidReturnDecomposition { source, .. } => Some(source),
            Self::TradingTimeSamplingFailure { source, .. } => Some(source),
            Self::InvalidReturnComponents { source, .. } => Some(source),
            Self::InvalidTransitionStep { source } => Some(source),
            Self::CalendarSessionBoundaryResolutionFailed { .. } => None,
            _ => None,
        }
    }
}
