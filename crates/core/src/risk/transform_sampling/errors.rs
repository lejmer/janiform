use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_affine_oracle::affine_oracle::{
    ExponentialAffineTransformGuardFailureCode, ExponentialAffineTransformInputError,
    ExponentialAffineTransformIntegrationStatus, ExponentialAffineTransformNewsIntensityInputError,
    ExponentialAffineTransformSolverError,
};
use janiform_foundations::complex_math::ComplexScalar;
use janiform_model_kernels::hawkes::{HawkesEventSide, HawkesTransitionError};

use super::contracts::{
    SufficientStatisticsBoundaryStateUpdateSnapshot, SufficientStatisticsTransformSliceContext,
};

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsTransformSliceContextError {
    InvalidSeasonalityFactor {
        interval_index: usize,
        value: f64,
    },
    InvalidNewsIntensity {
        interval_index: usize,
        source: ExponentialAffineTransformNewsIntensityInputError,
    },
    InvalidIntervalDelta {
        interval_index: usize,
        value: f64,
    },
    InvalidInitialState {
        interval_index: usize,
        field: &'static str,
        value: f64,
    },
}

impl Display for SufficientStatisticsTransformSliceContextError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSeasonalityFactor {
                interval_index,
                value,
            } => write!(
                formatter,
                "interval {interval_index} has invalid s_h, expected finite non-negative value, found {value}"
            ),
            Self::InvalidNewsIntensity {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} has invalid news_intensity: {source}"
            ),
            Self::InvalidIntervalDelta {
                interval_index,
                value,
            } => write!(
                formatter,
                "interval {interval_index} has invalid delta, expected finite strictly positive value, found {value}"
            ),
            Self::InvalidInitialState {
                interval_index,
                field,
                value,
            } => write!(
                formatter,
                "interval {interval_index} has invalid initial_excitation_state.{field}, expected finite non-negative value, found {value}"
            ),
        }
    }
}

impl Error for SufficientStatisticsTransformSliceContextError {}
#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsTransformSliceSettingsError {
    InvalidAPlusU { value: f64 },
    InvalidAMinusU { value: f64 },
    InvalidNPlusZ { value: f64 },
    InvalidNMinusZ { value: f64 },
}

impl Display for SufficientStatisticsTransformSliceSettingsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidAPlusU { value } => write!(
                formatter,
                "a_plus_u must be finite and strictly positive, found {value}"
            ),
            Self::InvalidAMinusU { value } => write!(
                formatter,
                "a_minus_u must be finite and strictly positive, found {value}"
            ),
            Self::InvalidNPlusZ { value } => write!(
                formatter,
                "n_plus_z must be finite, strictly positive, and different from 1.0, found {value}"
            ),
            Self::InvalidNMinusZ { value } => write!(
                formatter,
                "n_minus_z must be finite, strictly positive, and different from 1.0, found {value}"
            ),
        }
    }
}

impl Error for SufficientStatisticsTransformSliceSettingsError {}
#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsTransformSlicesError {
    InvalidContext {
        source: SufficientStatisticsTransformSliceContextError,
    },
    InvalidSettings {
        source: SufficientStatisticsTransformSliceSettingsError,
    },
    InvalidTransformValue {
        field: &'static str,
        value: ComplexScalar,
    },
}

impl Display for SufficientStatisticsTransformSlicesError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidContext { source } => {
                write!(formatter, "transform-slice context is invalid: {source}")
            }
            Self::InvalidSettings { source } => {
                write!(formatter, "transform-slice settings are invalid: {source}")
            }
            Self::InvalidTransformValue { field, value } => write!(
                formatter,
                "{field} must be finite as a complex scalar, found {value:?}"
            ),
        }
    }
}

impl Error for SufficientStatisticsTransformSlicesError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidContext { source } => Some(source),
            Self::InvalidSettings { source } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsTransformSliceSolveError {
    InvalidContext {
        source: SufficientStatisticsTransformSliceContextError,
    },
    InvalidSettings {
        source: SufficientStatisticsTransformSliceSettingsError,
    },
    InvalidTransformInput {
        interval_index: usize,
        slice_name: &'static str,
        source: ExponentialAffineTransformInputError,
    },
    SolverFailure {
        interval_index: usize,
        slice_name: &'static str,
        source: ExponentialAffineTransformSolverError,
    },
    IncompleteSolverOutput {
        interval_index: usize,
        slice_name: &'static str,
        status: ExponentialAffineTransformIntegrationStatus,
        guard_failure_code: Option<ExponentialAffineTransformGuardFailureCode>,
    },
    InvalidSolvedSlices {
        interval_index: usize,
        source: SufficientStatisticsTransformSlicesError,
    },
}

impl Display for SufficientStatisticsTransformSliceSolveError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidContext { source } => {
                write!(
                    formatter,
                    "transform-slice solve context is invalid: {source}"
                )
            }
            Self::InvalidSettings { source } => {
                write!(
                    formatter,
                    "transform-slice solve settings are invalid: {source}"
                )
            }
            Self::InvalidTransformInput {
                interval_index,
                slice_name,
                source,
            } => write!(
                formatter,
                "interval {interval_index} transform input for slice {slice_name} is invalid: {source}"
            ),
            Self::SolverFailure {
                interval_index,
                slice_name,
                source,
            } => write!(
                formatter,
                "interval {interval_index} solver failed for slice {slice_name}: {source}"
            ),
            Self::IncompleteSolverOutput {
                interval_index,
                slice_name,
                status,
                guard_failure_code,
            } => write!(
                formatter,
                "interval {interval_index} solver did not produce completed output for slice {slice_name}, status={status:?}, guard_failure_code={guard_failure_code:?}"
            ),
            Self::InvalidSolvedSlices {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} solved transform slices are invalid: {source}"
            ),
        }
    }
}

impl Error for SufficientStatisticsTransformSliceSolveError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidContext { source } => Some(source),
            Self::InvalidSettings { source } => Some(source),
            Self::InvalidTransformInput { source, .. } => Some(source),
            Self::SolverFailure { source, .. } => Some(source),
            Self::InvalidSolvedSlices { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsTransformSliceSamplingError {
    InvalidSlices {
        source: SufficientStatisticsTransformSlicesError,
    },
    InvalidChunkingSettings {
        source: SufficientStatisticsStreamingChunkingSettingsError,
    },
    NonRealTransformSlice {
        interval_index: usize,
        field: &'static str,
        value: ComplexScalar,
        tolerance: f64,
    },
    InvalidExpectedStatistic {
        interval_index: usize,
        statistic: &'static str,
        value: f64,
        numerator: f64,
        denominator: f64,
        context: SufficientStatisticsTransformSliceContext,
    },
    InvalidSampledStatistics {
        interval_index: usize,
        source: SufficientStatisticsIntervalStatisticsError,
    },
}

impl Display for SufficientStatisticsTransformSliceSamplingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSlices { source } => {
                write!(formatter, "invalid transform slices for sampling: {source}")
            }
            Self::InvalidChunkingSettings { source } => {
                write!(
                    formatter,
                    "invalid batch chunking settings for sampling: {source}"
                )
            }
            Self::NonRealTransformSlice {
                interval_index,
                field,
                value,
                tolerance,
            } => write!(
                formatter,
                "interval {interval_index} {field} must be real-valued within imaginary tolerance {tolerance}, found {value:?}"
            ),
            Self::InvalidExpectedStatistic {
                interval_index,
                statistic,
                value,
                numerator,
                denominator,
                context,
            } => write!(
                formatter,
                "interval {interval_index} derived expected {statistic} is invalid (value={value}, numerator={numerator}, denominator={denominator}, s_h={}, total_nu_h={}, known_nu_h={}, surprise_nu_h={}, delta={}, initial_state={:?})",
                context.s_h,
                context.news_intensity.total_nu_h(),
                context.news_intensity.known_nu_h,
                context.news_intensity.surprise_nu_h,
                context.delta,
                context.initial_excitation_state
            ),
            Self::InvalidSampledStatistics {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} sampled sufficient statistics are invalid: {source}"
            ),
        }
    }
}

impl Error for SufficientStatisticsTransformSliceSamplingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidSlices { source } => Some(source),
            Self::InvalidChunkingSettings { source } => Some(source),
            Self::InvalidSampledStatistics { source, .. } => Some(source),
            _ => None,
        }
    }
}
/// Validation errors for streaming chunking settings.
#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsStreamingChunkingSettingsError {
    InvalidMaxIntervalsPerChunk { value: usize },
}

impl Display for SufficientStatisticsStreamingChunkingSettingsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMaxIntervalsPerChunk { value } => write!(
                formatter,
                "max_intervals_per_chunk must be greater than 0, found {value}"
            ),
        }
    }
}

impl Error for SufficientStatisticsStreamingChunkingSettingsError {}
/// Errors emitted by streaming and batch transform-slice execution adapters.
#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsTransformSliceExecutionError {
    InvalidChunkingSettings {
        source: SufficientStatisticsStreamingChunkingSettingsError,
    },
    SamplingFailure {
        source: SufficientStatisticsTransformSliceSamplingError,
    },
    PathSamplingFailure {
        path_index: usize,
        source: SufficientStatisticsTransformSliceSamplingError,
    },
}

impl Display for SufficientStatisticsTransformSliceExecutionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidChunkingSettings { source } => {
                write!(
                    formatter,
                    "streaming chunking settings are invalid: {source}"
                )
            }
            Self::SamplingFailure { source } => write!(
                formatter,
                "transform-slice sampling failed during streaming execution: {source}"
            ),
            Self::PathSamplingFailure { path_index, source } => write!(
                formatter,
                "path {path_index} sampling failed during batch execution: {source}"
            ),
        }
    }
}

impl Error for SufficientStatisticsTransformSliceExecutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidChunkingSettings { source } => Some(source),
            Self::SamplingFailure { source } => Some(source),
            Self::PathSamplingFailure { source, .. } => Some(source),
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsIntervalStatisticsError {
    InvalidAPlus { value: f64 },
    InvalidAMinus { value: f64 },
}

impl Display for SufficientStatisticsIntervalStatisticsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidAPlus { value } => {
                write!(
                    formatter,
                    "a_plus must be finite and non-negative, found {value}"
                )
            }
            Self::InvalidAMinus { value } => {
                write!(
                    formatter,
                    "a_minus must be finite and non-negative, found {value}"
                )
            }
        }
    }
}

impl Error for SufficientStatisticsIntervalStatisticsError {}
#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsBoundaryStateUpdateError {
    InvalidIntervalDelta {
        snapshot: SufficientStatisticsBoundaryStateUpdateSnapshot,
        value: f64,
    },
    InvalidStateBeforeUpdate {
        snapshot: SufficientStatisticsBoundaryStateUpdateSnapshot,
        field: &'static str,
        value: f64,
    },
    InvalidSampledStatistics {
        snapshot: SufficientStatisticsBoundaryStateUpdateSnapshot,
        source: SufficientStatisticsIntervalStatisticsError,
    },
    InvalidScaledKernelLoading {
        snapshot: SufficientStatisticsBoundaryStateUpdateSnapshot,
        side: HawkesEventSide,
        event_count: u64,
        field: &'static str,
        value: f64,
    },
    TransitionFailure {
        snapshot: SufficientStatisticsBoundaryStateUpdateSnapshot,
        source: HawkesTransitionError,
    },
    InvalidStateAfterUpdate {
        snapshot: SufficientStatisticsBoundaryStateUpdateSnapshot,
        field: &'static str,
        value: f64,
    },
}

impl Display for SufficientStatisticsBoundaryStateUpdateError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidIntervalDelta { snapshot, value } => write!(
                formatter,
                "boundary-state update failed for interval {}: interval_delta must be finite and strictly positive, found {value}; state_before_update={:?}, sampled_statistics={:?}",
                snapshot.interval_index, snapshot.state_before_update, snapshot.sampled_statistics
            ),
            Self::InvalidStateBeforeUpdate {
                snapshot,
                field,
                value,
            } => write!(
                formatter,
                "boundary-state update failed for interval {}: state_before_update.{field} must be finite and non-negative, found {value}; state_before_update={:?}, sampled_statistics={:?}",
                snapshot.interval_index, snapshot.state_before_update, snapshot.sampled_statistics
            ),
            Self::InvalidSampledStatistics { snapshot, source } => write!(
                formatter,
                "boundary-state update failed for interval {}: sampled_statistics are invalid: {source}; state_before_update={:?}, sampled_statistics={:?}",
                snapshot.interval_index, snapshot.state_before_update, snapshot.sampled_statistics
            ),
            Self::InvalidScaledKernelLoading {
                snapshot,
                side,
                event_count,
                field,
                value,
            } => write!(
                formatter,
                "boundary-state update failed for interval {}: scaled loading {field} for side {side:?} with event_count {} must be finite and non-negative, found {value}; state_before_update={:?}, sampled_statistics={:?}",
                snapshot.interval_index,
                event_count,
                snapshot.state_before_update,
                snapshot.sampled_statistics
            ),
            Self::TransitionFailure { snapshot, source } => write!(
                formatter,
                "boundary-state update failed for interval {} during canonical transition composition: {source}; state_before_update={:?}, sampled_statistics={:?}",
                snapshot.interval_index, snapshot.state_before_update, snapshot.sampled_statistics
            ),
            Self::InvalidStateAfterUpdate {
                snapshot,
                field,
                value,
            } => write!(
                formatter,
                "boundary-state update failed for interval {}: state_after_update.{field} must be finite and non-negative, found {value}; state_before_update={:?}, sampled_statistics={:?}",
                snapshot.interval_index, snapshot.state_before_update, snapshot.sampled_statistics
            ),
        }
    }
}

impl Error for SufficientStatisticsBoundaryStateUpdateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidSampledStatistics { source, .. } => Some(source),
            Self::TransitionFailure { source, .. } => Some(source),
            _ => None,
        }
    }
}
