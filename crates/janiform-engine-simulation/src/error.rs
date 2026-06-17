//! Seeded simulation error contract.

use std::error::Error;
use std::fmt::{Display, Formatter};

use chrono::{DateTime, Utc};
use janiform_foundations::cancellation::CancellationError;

use crate::long_variance::LongVarianceTransitionSamplingError;
use janiform_affine_oracle::sufficient_statistics::SufficientStatisticsReturnComponentsError;
use janiform_market_time::calendar::ExchangeCalendarResolveError;
use janiform_model_kernels::hawkes::HawkesTransitionError;
use janiform_model_kernels::long_variance::LongVarianceStationaryLevelForHawkesKernelError;
use janiform_model_kernels::long_variance::{LongVarianceParametersError, LongVarianceStateError};
use janiform_model_kernels::mean_reversion::{
    MeanReversionBoundaryStateValidationError, MeanReversionFittedParametersValidationError,
    MeanReversionRuntimeIntervalAnchorsValidationError,
};
use janiform_model_kernels::news::{NewsActivityLoadingRateError, NewsProcessConfigurationError};
use janiform_model_kernels::news::{NewsMarkInterfaceError, SharedNewsEventDispatchError};
use janiform_model_kernels::nig::NigSharedPlusShiftConfigurationError;
use janiform_model_kernels::no_move_event::NoMoveEventModelError;
use janiform_model_kernels::return_clock::{
    EffectiveReturnClocksBuildError, ReturnClockExposureParametersError,
};
use janiform_model_kernels::return_decomposition::ReturnDecompositionValidationError;
use janiform_model_kernels::timestamp_observation::TimestampBatchingObservationModelError;

/// Structured simulator run failure with explicit validation diagnostics.
#[derive(Debug, Clone, PartialEq)]
pub enum SeededTickSimulationRunError {
    Cancelled {
        source: CancellationError,
    },
    InvalidHorizonIntervalCount {
        value: usize,
    },
    MissingIntervalFactorGridIntervals,
    IntervalFactorGridFieldLengthMismatch {
        field_name: &'static str,
        expected_interval_count: usize,
        observed_interval_count: usize,
    },
    IntervalFactorGridDiagnosticsIntervalCountMismatch {
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
    StartTimeDoesNotMatchIntervalFactorGridCoverage {
        start_time_utc_nanos: i64,
        first_interval_start_utc_nanos: i64,
    },
    InvalidIntervalFactorGridIntervalBounds {
        interval_index: usize,
        interval_start_utc_nanos: i64,
        interval_end_utc_nanos: i64,
    },
    BufferAllocationFailed {
        buffer_name: &'static str,
        requested_capacity: usize,
        detail: String,
    },
    NonConsecutiveIntervalFactorGridCoverage {
        previous_interval_index: usize,
        previous_interval_end_utc_nanos: i64,
        current_interval_index: usize,
        current_interval_start_utc_nanos: i64,
    },
    SeasonalityScaledEndogenousInstability {
        spectral_radius_alpha_over_beta: f64,
        effective_branching_ratio: f64,
    },
    InvalidReturnClockExposureParameters {
        source: Box<ReturnClockExposureParametersError>,
    },
    InvalidNoMoveEventModel {
        source: Box<NoMoveEventModelError>,
    },
    InvalidLongVarianceParameters {
        source: Box<LongVarianceParametersError>,
    },
    InvalidInitialLongVarianceState {
        source: Box<LongVarianceStateError>,
    },
    InvalidLongVarianceStationaryLevel {
        source: Box<LongVarianceStationaryLevelForHawkesKernelError>,
    },
    InvalidNewsActivityLoadingRate {
        interval_index: usize,
        source: Box<NewsActivityLoadingRateError>,
    },
    InvalidNewsTargetSourceRates {
        interval_index: usize,
    },
    InvalidObservedPriceTickSize {
        value: f64,
    },
    InvalidTimestampBatchingObservationModel {
        source: Box<TimestampBatchingObservationModelError>,
    },
    TickGridProjectionFailed {
        interval_index: usize,
        previous_observed_log_price: f64,
        raw_total_return: f64,
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
        source: Box<MeanReversionFittedParametersValidationError>,
    },
    InvalidMeanReversionRuntimeIntervalAnchors {
        source: Box<MeanReversionRuntimeIntervalAnchorsValidationError>,
    },
    InvalidMeanReversionBoundaryState {
        source: Box<MeanReversionBoundaryStateValidationError>,
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
        source: Box<NigSharedPlusShiftConfigurationError>,
    },
    InvalidNewsProcessConfiguration {
        source: Box<NewsProcessConfigurationError>,
    },
    InvalidGeneratedNewsActivityCouplingMark {
        value: f64,
    },
    InvalidGeneratedNewsReturnJumpMark {
        value: f64,
    },
    NewsMarkSamplingFailure {
        interval_index: usize,
        source: Box<NewsMarkInterfaceError>,
    },
    SharedNewsDispatchFailure {
        interval_index: usize,
        source: Box<SharedNewsEventDispatchError>,
    },
    NewsMarketStateResolutionFailed {
        exchange_code: String,
        timestamp_utc_nanos: i64,
        source: Box<ExchangeCalendarResolveError>,
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
        source: Box<ReturnDecompositionValidationError>,
    },
    TradingTimeSamplingFailure {
        interval_index: usize,
        source: Box<janiform_model_kernels::nig::NigTradingTimeIncrementSamplingError>,
    },
    EffectiveReturnClockBuildFailure {
        interval_index: usize,
        source: Box<EffectiveReturnClocksBuildError>,
    },
    LongVarianceTransitionFailure {
        interval_index: usize,
        source: Box<LongVarianceTransitionSamplingError>,
    },
    InvalidReturnComponents {
        interval_index: usize,
        source: Box<SufficientStatisticsReturnComponentsError>,
    },
    InvalidTransitionStep {
        source: Box<HawkesTransitionError>,
    },
    InternalInvariantViolation {
        message: &'static str,
    },
}

impl Display for SeededTickSimulationRunError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled { source } => write!(formatter, "{source}"),
            Self::InvalidHorizonIntervalCount { value } => write!(
                formatter,
                "horizon_interval_count must be greater than 0, found {value}"
            ),
            Self::MissingIntervalFactorGridIntervals => write!(
                formatter,
                "interval_factor_grid.interval_count must be greater than 0"
            ),
            Self::IntervalFactorGridFieldLengthMismatch {
                field_name,
                expected_interval_count,
                observed_interval_count,
            } => write!(
                formatter,
                "interval_factor_grid.{field_name} length must match interval_count ({expected_interval_count}), found {observed_interval_count}"
            ),
            Self::IntervalFactorGridDiagnosticsIntervalCountMismatch {
                expected_interval_count,
                diagnostics_interval_count,
            } => write!(
                formatter,
                "interval_factor_grid.diagnostics.interval_count must match interval_count ({expected_interval_count}), found {diagnostics_interval_count}"
            ),
            Self::HorizonExceedsAvailableIntervals {
                requested_interval_count,
                available_interval_count,
            } => write!(
                formatter,
                "horizon_interval_count ({requested_interval_count}) exceeds interval factor grid interval_count ({available_interval_count})"
            ),
            Self::StartTimeOutOfNanosecondRange { value } => write!(
                formatter,
                "start_time_utc cannot be represented in nanoseconds, found {value}"
            ),
            Self::StartTimeDoesNotMatchIntervalFactorGridCoverage {
                start_time_utc_nanos,
                first_interval_start_utc_nanos,
            } => write!(
                formatter,
                "start_time_utc nanos ({start_time_utc_nanos}) must match interval factor grid first interval start ({first_interval_start_utc_nanos})"
            ),
            Self::InvalidIntervalFactorGridIntervalBounds {
                interval_index,
                interval_start_utc_nanos,
                interval_end_utc_nanos,
            } => write!(
                formatter,
                "interval_factor_grid interval {interval_index} has invalid bounds, start={interval_start_utc_nanos}, end={interval_end_utc_nanos}"
            ),
            Self::BufferAllocationFailed {
                buffer_name,
                requested_capacity,
                detail,
            } => write!(
                formatter,
                "failed to reserve buffer `{buffer_name}` with requested_capacity={requested_capacity}: {detail}"
            ),
            Self::NonConsecutiveIntervalFactorGridCoverage {
                previous_interval_index,
                previous_interval_end_utc_nanos,
                current_interval_index,
                current_interval_start_utc_nanos,
            } => write!(
                formatter,
                "interval_factor_grid interval coverage must be consecutive, interval {previous_interval_index} end ({previous_interval_end_utc_nanos}) must equal interval {current_interval_index} start ({current_interval_start_utc_nanos})"
            ),
            Self::SeasonalityScaledEndogenousInstability {
                spectral_radius_alpha_over_beta,
                effective_branching_ratio,
            } => write!(
                formatter,
                "endogenous Hawkes branching must remain below 1.0, found spectral_radius_alpha_over_beta={spectral_radius_alpha_over_beta}, effective_branching_ratio={effective_branching_ratio}"
            ),
            Self::InvalidReturnClockExposureParameters { source } => {
                write!(
                    formatter,
                    "return_clock_exposure_parameters are invalid: {source}"
                )
            }
            Self::InvalidNoMoveEventModel { source } => {
                write!(formatter, "no_move_event_model is invalid: {source}")
            }
            Self::InvalidLongVarianceParameters { source } => {
                write!(formatter, "long_variance_parameters are invalid: {source}")
            }
            Self::InvalidInitialLongVarianceState { source } => {
                write!(
                    formatter,
                    "initial_long_variance_state is invalid: {source}"
                )
            }
            Self::InvalidLongVarianceStationaryLevel { source } => write!(
                formatter,
                "long_variance_parameters imply an invalid stationary mean-reversion level: {source}"
            ),
            Self::InvalidNewsActivityLoadingRate {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} news activity-loading rate is invalid: {source}"
            ),
            Self::InvalidNewsTargetSourceRates { interval_index } => write!(
                formatter,
                "interval {interval_index} news activity-loading rate produces invalid Hawkes target source rates"
            ),
            Self::InvalidObservedPriceTickSize { value } => write!(
                formatter,
                "observed_price_tick_size must be finite and strictly positive when provided, found {value}"
            ),
            Self::InvalidTimestampBatchingObservationModel { source } => write!(
                formatter,
                "timestamp_batching_observation_model is invalid: {source}"
            ),
            Self::TickGridProjectionFailed {
                interval_index,
                previous_observed_log_price,
                raw_total_return,
            } => write!(
                formatter,
                "interval {interval_index} tick-grid observation projection failed for previous_observed_log_price={previous_observed_log_price} and raw_total_return={raw_total_return}"
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
                "mean_reversion_runtime_interval_anchors.interval_alignment_policy must match interval_factor_grid.metadata.interval_alignment_policy, expected `{expected_policy}`, found `{observed_policy}`"
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
                "mean_reversion_runtime_interval_anchors.interval_deltas[{interval_index}] must match the interval factor grid interval delta in hours, expected {expected_interval_delta}, found {observed_interval_delta}"
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
            Self::EffectiveReturnClockBuildFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} effective return-clock construction failed: {source}"
            ),
            Self::LongVarianceTransitionFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} long-variance transition failed: {source}"
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
            Self::InternalInvariantViolation { message } => {
                write!(
                    formatter,
                    "seeded simulation internal invariant violation: {message}"
                )
            }
        }
    }
}

impl Error for SeededTickSimulationRunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled { source } => Some(source),
            Self::InvalidMeanReversionFittedParameters { source } => Some(source.as_ref()),
            Self::InvalidMeanReversionRuntimeIntervalAnchors { source } => Some(source.as_ref()),
            Self::InvalidMeanReversionBoundaryState { source } => Some(source.as_ref()),
            Self::InvalidNigSharedPlusShiftConfiguration { source } => Some(source.as_ref()),
            Self::InvalidNewsProcessConfiguration { source } => Some(source.as_ref()),
            Self::InvalidTimestampBatchingObservationModel { source } => Some(source.as_ref()),
            Self::InvalidReturnClockExposureParameters { source } => Some(source.as_ref()),
            Self::InvalidNoMoveEventModel { source } => Some(source.as_ref()),
            Self::InvalidLongVarianceParameters { source } => Some(source.as_ref()),
            Self::InvalidInitialLongVarianceState { source } => Some(source.as_ref()),
            Self::InvalidLongVarianceStationaryLevel { source } => Some(source.as_ref()),
            Self::InvalidNewsActivityLoadingRate { source, .. } => Some(source.as_ref()),
            Self::NewsMarkSamplingFailure { source, .. } => Some(source.as_ref()),
            Self::SharedNewsDispatchFailure { source, .. } => Some(source.as_ref()),
            Self::NewsMarketStateResolutionFailed { source, .. } => Some(source.as_ref()),
            Self::InvalidReturnDecomposition { source, .. } => Some(source.as_ref()),
            Self::TradingTimeSamplingFailure { source, .. } => Some(source.as_ref()),
            Self::EffectiveReturnClockBuildFailure { source, .. } => Some(source.as_ref()),
            Self::LongVarianceTransitionFailure { source, .. } => Some(source.as_ref()),
            Self::InvalidReturnComponents { source, .. } => Some(source.as_ref()),
            Self::InvalidTransitionStep { source } => Some(source.as_ref()),
            Self::CalendarSessionBoundaryResolutionFailed { .. } => None,
            Self::InternalInvariantViolation { .. } => None,
            _ => None,
        }
    }
}

impl From<CancellationError> for SeededTickSimulationRunError {
    fn from(source: CancellationError) -> Self {
        Self::Cancelled { source }
    }
}
