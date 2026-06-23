use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_affine_oracle::sufficient_statistics::{
    SufficientStatisticsReturnComponentsError, SufficientStatisticsTransformSliceSamplingError,
};
use janiform_foundations::cancellation::CancellationError;
use janiform_model_kernels::hawkes::HawkesTransitionError;
use janiform_model_kernels::long_variance::LongVarianceParametersError;
use janiform_model_kernels::market_pressure::MarketPressureStateEvolutionError;
use janiform_model_kernels::news::{
    NewsActivityLoadingRateError, NewsMarkInterfaceError, NewsProcessConfigurationError,
    SharedNewsEventDispatchError,
};
use janiform_model_kernels::return_clock::EffectiveReturnClocksBuildError;

use crate::error::SeededTickSimulationRunError;
use crate::long_variance::LongVarianceTransitionSamplingError;

use super::super::contracts::{
    CoarseSimulationPathError, CoarseSimulationPathRecordError, CoarseSimulationRunRequestError,
    IntervalDeltaGridError,
};

#[derive(Debug, Clone, PartialEq)]
pub enum CoarseSimulationRunError {
    Cancelled {
        source: CancellationError,
    },
    InvalidRunRequest {
        source: Box<CoarseSimulationRunRequestError>,
    },
    CalendarRegistryLoadFailure {
        source: Box<janiform_market_time::calendar::ExchangeCalendarRegistryLoadError>,
    },
    InvalidNigDriverConfiguration {
        source: Box<janiform_model_kernels::nig::NigSharedPlusShiftConfigurationError>,
    },
    InvalidLongVarianceParameters {
        source: Box<LongVarianceParametersError>,
    },
    InvalidLongVarianceStationaryLevel {
        source: Box<
            janiform_model_kernels::long_variance::LongVarianceStationaryLevelForHawkesKernelError,
        >,
    },
    InvalidLongVarianceStationaryEnergyRate {
        interval_index: usize,
    },
    InvalidNewsActivityLoadingRate {
        interval_index: usize,
        source: Box<NewsActivityLoadingRateError>,
    },
    InvalidNewsTargetSourceRates {
        interval_index: usize,
    },
    InvalidIntervalDelta {
        source: Box<IntervalDeltaGridError>,
    },
    IntervalBoundaryOverflow {
        interval_index: usize,
    },
    EndogenousFirstMomentProjectionFailure {
        interval_index: usize,
        message: String,
    },
    TransformSliceSamplingFailure {
        interval_index: usize,
        source: Box<SufficientStatisticsTransformSliceSamplingError>,
    },
    KnownNewsFactorAlignmentFailure {
        interval_index: usize,
        expected_known_news_intensity: f64,
        derived_known_news_intensity: f64,
    },
    NewsProcessConfigurationFailure {
        interval_index: usize,
        source: Box<NewsProcessConfigurationError>,
    },
    NewsMarkSamplingFailure {
        interval_index: usize,
        source: Box<NewsMarkInterfaceError>,
    },
    SharedNewsDispatchFailure {
        interval_index: usize,
        source: Box<SharedNewsEventDispatchError>,
    },
    InvalidReturnDecomposition {
        interval_index: usize,
        source:
            Box<janiform_model_kernels::return_decomposition::ReturnDecompositionValidationError>,
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
    MarketPressureTransitionFailure {
        interval_index: usize,
        source: Box<MarketPressureStateEvolutionError>,
    },
    TickGridProjectionFailure {
        interval_index: usize,
        source: Box<SeededTickSimulationRunError>,
    },
    InvalidReturnComponents {
        interval_index: usize,
        source: Box<SufficientStatisticsReturnComponentsError>,
    },
    InvalidPathRecord {
        interval_index: usize,
        source: Box<CoarseSimulationPathRecordError>,
    },
    OpenMarketExposureResolutionFailure {
        source: Box<SeededTickSimulationRunError>,
    },
    NewsBoundaryStateUpdateFailure {
        interval_index: usize,
        source: Box<HawkesTransitionError>,
    },
    InvalidPathArtifact {
        source: Box<CoarseSimulationPathError>,
    },
}

impl Display for CoarseSimulationRunError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled { source } => write!(formatter, "{source}"),
            Self::InvalidRunRequest { source } => {
                write!(formatter, "run_request is invalid: {source}")
            }
            Self::CalendarRegistryLoadFailure { source } => {
                write!(
                    formatter,
                    "failed to load calendar registry from run_request.model_inputs: {source}"
                )
            }
            Self::InvalidNigDriverConfiguration { source } => {
                write!(
                    formatter,
                    "nig_shared_plus_shift_configuration is invalid: {source}"
                )
            }
            Self::InvalidLongVarianceParameters { source } => {
                write!(formatter, "long_variance_parameters are invalid: {source}")
            }
            Self::InvalidLongVarianceStationaryLevel { source } => write!(
                formatter,
                "long_variance_parameters imply an invalid stationary mean-reversion level: {source}"
            ),
            Self::InvalidLongVarianceStationaryEnergyRate { interval_index } => write!(
                formatter,
                "interval {interval_index} return generator implies an invalid stationary return-energy rate for long variance"
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
            Self::InvalidIntervalDelta { source } => {
                write!(formatter, "interval_delta is invalid: {source}")
            }
            Self::IntervalBoundaryOverflow { interval_index } => {
                write!(
                    formatter,
                    "interval {interval_index} boundary timestamps overflowed i64"
                )
            }
            Self::EndogenousFirstMomentProjectionFailure {
                interval_index,
                message,
            } => write!(
                formatter,
                "interval {interval_index} endogenous first-moment projection failed: {message}"
            ),
            Self::TransformSliceSamplingFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} sufficient-statistics sampling failed: {source}"
            ),
            Self::KnownNewsFactorAlignmentFailure {
                interval_index,
                expected_known_news_intensity,
                derived_known_news_intensity,
            } => write!(
                formatter,
                "interval {interval_index} known-news factor mismatch, expected known_news_intensity={expected_known_news_intensity} from interval_factor_grids and derived {derived_known_news_intensity} from scheduled news timestamps"
            ),
            Self::NewsProcessConfigurationFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} news_process_configuration is invalid: {source}"
            ),
            Self::NewsMarkSamplingFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} shared news mark sampling failed: {source}"
            ),
            Self::SharedNewsDispatchFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} shared news dispatch failed: {source}"
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
                "interval {interval_index} trading-time increment sampling failed: {source}"
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
            Self::MarketPressureTransitionFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} market-pressure transition failed: {source}"
            ),
            Self::TickGridProjectionFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} tick-grid observation projection failed: {source}"
            ),
            Self::InvalidReturnComponents {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} return components are invalid: {source}"
            ),
            Self::InvalidPathRecord {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} path record is invalid: {source}"
            ),
            Self::OpenMarketExposureResolutionFailure { source } => write!(
                formatter,
                "failed to resolve coarse open-market exposure: {source}"
            ),
            Self::NewsBoundaryStateUpdateFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} news activity-coupling boundary update failed: {source}"
            ),
            Self::InvalidPathArtifact { source } => {
                write!(
                    formatter,
                    "coarse simulation path artifact is invalid: {source}"
                )
            }
        }
    }
}

impl Error for CoarseSimulationRunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled { source } => Some(source),
            Self::InvalidRunRequest { source } => Some(source.as_ref()),
            Self::CalendarRegistryLoadFailure { source } => Some(source.as_ref()),
            Self::InvalidNigDriverConfiguration { source } => Some(source.as_ref()),
            Self::InvalidLongVarianceParameters { source } => Some(source.as_ref()),
            Self::InvalidLongVarianceStationaryLevel { source } => Some(source.as_ref()),
            Self::InvalidNewsActivityLoadingRate { source, .. } => Some(source.as_ref()),
            Self::InvalidIntervalDelta { source } => Some(source.as_ref()),
            Self::TransformSliceSamplingFailure { source, .. } => Some(source.as_ref()),
            Self::NewsProcessConfigurationFailure { source, .. } => Some(source.as_ref()),
            Self::NewsMarkSamplingFailure { source, .. } => Some(source.as_ref()),
            Self::SharedNewsDispatchFailure { source, .. } => Some(source.as_ref()),
            Self::InvalidReturnDecomposition { source, .. } => Some(source.as_ref()),
            Self::TradingTimeSamplingFailure { source, .. } => Some(source.as_ref()),
            Self::EffectiveReturnClockBuildFailure { source, .. } => Some(source.as_ref()),
            Self::LongVarianceTransitionFailure { source, .. } => Some(source.as_ref()),
            Self::MarketPressureTransitionFailure { source, .. } => Some(source.as_ref()),
            Self::TickGridProjectionFailure { source, .. } => Some(source.as_ref()),
            Self::InvalidReturnComponents { source, .. } => Some(source.as_ref()),
            Self::InvalidPathRecord { source, .. } => Some(source.as_ref()),
            Self::OpenMarketExposureResolutionFailure { source } => Some(source.as_ref()),
            Self::NewsBoundaryStateUpdateFailure { source, .. } => Some(source.as_ref()),
            Self::InvalidPathArtifact { source } => Some(source.as_ref()),
            Self::IntervalBoundaryOverflow { .. }
            | Self::EndogenousFirstMomentProjectionFailure { .. }
            | Self::KnownNewsFactorAlignmentFailure { .. }
            | Self::InvalidLongVarianceStationaryEnergyRate { .. }
            | Self::InvalidNewsTargetSourceRates { .. } => None,
        }
    }
}

impl From<CancellationError> for CoarseSimulationRunError {
    fn from(source: CancellationError) -> Self {
        Self::Cancelled { source }
    }
}
