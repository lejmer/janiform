use serde::{Deserialize, Serialize};

use crate::affine_oracle::ExponentialAffineTransformNewsIntensityInput;
use janiform_foundations::complex_math::ComplexScalar;
use janiform_model_kernels::hawkes::HawkesExcitationState;

use super::constants::{
    DEFAULT_STREAMING_MAX_INTERVALS_PER_CHUNK, DEFAULT_TRANSFORM_SLICE_A_MINUS_U,
    DEFAULT_TRANSFORM_SLICE_A_PLUS_U, DEFAULT_TRANSFORM_SLICE_N_MINUS_Z,
    DEFAULT_TRANSFORM_SLICE_N_PLUS_Z, SUFFICIENT_STATISTICS_BOUNDARY_STATE_UPDATE_POLICY,
    SUFFICIENT_STATISTICS_STREAMING_CHUNKING_POLICY,
    SUFFICIENT_STATISTICS_TRANSFORM_SLICE_SETTINGS_POLICY,
};
use super::errors::{
    SufficientStatisticsIntervalStatisticsError,
    SufficientStatisticsStreamingChunkingSettingsError,
    SufficientStatisticsTransformSliceContextError,
    SufficientStatisticsTransformSliceSettingsError, SufficientStatisticsTransformSlicesError,
};
use super::validation::validate_excitation_state;

/// One-interval context used to solve and sample sufficient-statistics transform slices.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SufficientStatisticsTransformSliceContext {
    pub interval_index: usize,
    pub s_h: f64,
    pub news_intensity: ExponentialAffineTransformNewsIntensityInput,
    pub delta: f64,
    pub initial_excitation_state: HawkesExcitationState,
}

impl SufficientStatisticsTransformSliceContext {
    pub fn try_new(
        interval_index: usize,
        s_h: f64,
        news_intensity: ExponentialAffineTransformNewsIntensityInput,
        delta: f64,
        initial_excitation_state: HawkesExcitationState,
    ) -> Result<Self, SufficientStatisticsTransformSliceContextError> {
        let context = Self {
            interval_index,
            s_h,
            news_intensity,
            delta,
            initial_excitation_state,
        };
        context.validate()?;
        Ok(context)
    }

    pub fn validate(self) -> Result<(), SufficientStatisticsTransformSliceContextError> {
        if !self.s_h.is_finite() || self.s_h < 0.0 {
            return Err(
                SufficientStatisticsTransformSliceContextError::InvalidSeasonalityFactor {
                    interval_index: self.interval_index,
                    value: self.s_h,
                },
            );
        }
        self.news_intensity.validate().map_err(|source| {
            SufficientStatisticsTransformSliceContextError::InvalidNewsIntensity {
                interval_index: self.interval_index,
                source,
            }
        })?;
        if !self.delta.is_finite() || self.delta <= 0.0 {
            return Err(
                SufficientStatisticsTransformSliceContextError::InvalidIntervalDelta {
                    interval_index: self.interval_index,
                    value: self.delta,
                },
            );
        }

        validate_excitation_state(self.initial_excitation_state).map_err(|(field, value)| {
            SufficientStatisticsTransformSliceContextError::InvalidInitialState {
                interval_index: self.interval_index,
                field,
                value,
            }
        })?;

        Ok(())
    }
}

/// Probe settings for transform-slice extraction used by the risk sampler.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SufficientStatisticsTransformSliceSettings {
    pub a_plus_u: f64,
    pub a_minus_u: f64,
    pub n_plus_z: f64,
    pub n_minus_z: f64,
}

impl SufficientStatisticsTransformSliceSettings {
    pub fn try_new(
        a_plus_u: f64,
        a_minus_u: f64,
        n_plus_z: f64,
        n_minus_z: f64,
    ) -> Result<Self, SufficientStatisticsTransformSliceSettingsError> {
        let settings = Self {
            a_plus_u,
            a_minus_u,
            n_plus_z,
            n_minus_z,
        };
        settings.validate()?;
        Ok(settings)
    }

    pub fn validate(self) -> Result<(), SufficientStatisticsTransformSliceSettingsError> {
        if !self.a_plus_u.is_finite() || self.a_plus_u <= 0.0 {
            return Err(
                SufficientStatisticsTransformSliceSettingsError::InvalidAPlusU {
                    value: self.a_plus_u,
                },
            );
        }
        if !self.a_minus_u.is_finite() || self.a_minus_u <= 0.0 {
            return Err(
                SufficientStatisticsTransformSliceSettingsError::InvalidAMinusU {
                    value: self.a_minus_u,
                },
            );
        }
        if !self.n_plus_z.is_finite() || self.n_plus_z <= 0.0 || self.n_plus_z == 1.0 {
            return Err(
                SufficientStatisticsTransformSliceSettingsError::InvalidNPlusZ {
                    value: self.n_plus_z,
                },
            );
        }
        if !self.n_minus_z.is_finite() || self.n_minus_z <= 0.0 || self.n_minus_z == 1.0 {
            return Err(
                SufficientStatisticsTransformSliceSettingsError::InvalidNMinusZ {
                    value: self.n_minus_z,
                },
            );
        }

        Ok(())
    }

    pub const fn policy() -> &'static str {
        SUFFICIENT_STATISTICS_TRANSFORM_SLICE_SETTINGS_POLICY
    }
}

impl Default for SufficientStatisticsTransformSliceSettings {
    fn default() -> Self {
        Self {
            a_plus_u: DEFAULT_TRANSFORM_SLICE_A_PLUS_U,
            a_minus_u: DEFAULT_TRANSFORM_SLICE_A_MINUS_U,
            n_plus_z: DEFAULT_TRANSFORM_SLICE_N_PLUS_Z,
            n_minus_z: DEFAULT_TRANSFORM_SLICE_N_MINUS_Z,
        }
    }
}

/// One interval's transform-slice payload used to sample `A` and `N` sufficient statistics.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SufficientStatisticsTransformSlices {
    pub context: SufficientStatisticsTransformSliceContext,
    pub settings: SufficientStatisticsTransformSliceSettings,
    pub baseline_transform: ComplexScalar,
    pub a_plus_transform: ComplexScalar,
    pub a_minus_transform: ComplexScalar,
    pub n_plus_transform: ComplexScalar,
    pub n_minus_transform: ComplexScalar,
}

/// Closed-form one-interval expectations extracted from solved transform slices.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SufficientStatisticsExpectedIntervalStatistics {
    pub expected_a_plus: f64,
    pub expected_a_minus: f64,
    pub expected_n_plus: f64,
    pub expected_n_minus: f64,
}

impl SufficientStatisticsExpectedIntervalStatistics {
    pub fn try_new(
        expected_a_plus: f64,
        expected_a_minus: f64,
        expected_n_plus: f64,
        expected_n_minus: f64,
    ) -> Result<Self, SufficientStatisticsIntervalStatisticsError> {
        let statistics = Self {
            expected_a_plus,
            expected_a_minus,
            expected_n_plus,
            expected_n_minus,
        };

        statistics.validate()?;

        Ok(statistics)
    }

    pub fn validate(self) -> Result<(), SufficientStatisticsIntervalStatisticsError> {
        validate_non_negative_finite_statistic("expected_a_plus", self.expected_a_plus)?;
        validate_non_negative_finite_statistic("expected_a_minus", self.expected_a_minus)?;
        validate_non_negative_finite_statistic("expected_n_plus", self.expected_n_plus)?;
        validate_non_negative_finite_statistic("expected_n_minus", self.expected_n_minus)?;

        Ok(())
    }
}

impl SufficientStatisticsTransformSlices {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        context: SufficientStatisticsTransformSliceContext,
        settings: SufficientStatisticsTransformSliceSettings,
        baseline_transform: ComplexScalar,
        a_plus_transform: ComplexScalar,
        a_minus_transform: ComplexScalar,
        n_plus_transform: ComplexScalar,
        n_minus_transform: ComplexScalar,
    ) -> Result<Self, SufficientStatisticsTransformSlicesError> {
        let slices = Self {
            context,
            settings,
            baseline_transform,
            a_plus_transform,
            a_minus_transform,
            n_plus_transform,
            n_minus_transform,
        };

        slices.validate()?;

        Ok(slices)
    }

    pub fn validate(self) -> Result<(), SufficientStatisticsTransformSlicesError> {
        self.context.validate().map_err(|source| {
            SufficientStatisticsTransformSlicesError::InvalidContext { source }
        })?;
        self.settings.validate().map_err(|source| {
            SufficientStatisticsTransformSlicesError::InvalidSettings { source }
        })?;

        ensure_finite_transform_slice("baseline_transform", self.baseline_transform)?;
        ensure_finite_transform_slice("a_plus_transform", self.a_plus_transform)?;
        ensure_finite_transform_slice("a_minus_transform", self.a_minus_transform)?;
        ensure_finite_transform_slice("n_plus_transform", self.n_plus_transform)?;
        ensure_finite_transform_slice("n_minus_transform", self.n_minus_transform)?;

        Ok(())
    }
}

/// Chunking settings for streaming sufficient-statistics interval emission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SufficientStatisticsStreamingChunkingSettings {
    pub max_intervals_per_chunk: usize,
}

impl SufficientStatisticsStreamingChunkingSettings {
    pub fn try_new(
        max_intervals_per_chunk: usize,
    ) -> Result<Self, SufficientStatisticsStreamingChunkingSettingsError> {
        let settings = Self {
            max_intervals_per_chunk,
        };

        settings.validate()?;

        Ok(settings)
    }

    pub fn validate(self) -> Result<(), SufficientStatisticsStreamingChunkingSettingsError> {
        if self.max_intervals_per_chunk == 0 {
            return Err(
                SufficientStatisticsStreamingChunkingSettingsError::InvalidMaxIntervalsPerChunk {
                    value: self.max_intervals_per_chunk,
                },
            );
        }

        Ok(())
    }

    pub const fn policy() -> &'static str {
        SUFFICIENT_STATISTICS_STREAMING_CHUNKING_POLICY
    }
}

impl Default for SufficientStatisticsStreamingChunkingSettings {
    fn default() -> Self {
        Self {
            max_intervals_per_chunk: DEFAULT_STREAMING_MAX_INTERVALS_PER_CHUNK,
        }
    }
}
/// One emitted chunk of sampled interval statistics for a single path.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsIntervalStatisticsChunk {
    pub chunk_index: usize,
    pub start_interval_index: usize,
    pub records: Vec<SufficientStatisticsIntervalStatistics>,
}

/// One emitted chunk of sampled interval statistics for one path in a multi-path batch.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsIntervalStatisticsPathChunk {
    pub path_index: usize,
    pub path_seed: u64,
    pub interval_chunk: SufficientStatisticsIntervalStatisticsChunk,
}
fn ensure_finite_transform_slice(
    field: &'static str,
    value: ComplexScalar,
) -> Result<(), SufficientStatisticsTransformSlicesError> {
    if !value.is_finite() {
        return Err(
            SufficientStatisticsTransformSlicesError::InvalidTransformValue { field, value },
        );
    }

    Ok(())
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SufficientStatisticsIntervalStatistics {
    pub a_plus: f64,
    pub a_minus: f64,
    pub n_plus: u64,
    pub n_minus: u64,
}

impl SufficientStatisticsIntervalStatistics {
    pub fn try_new(
        a_plus: f64,
        a_minus: f64,
        n_plus: u64,
        n_minus: u64,
    ) -> Result<Self, SufficientStatisticsIntervalStatisticsError> {
        let interval_statistics = Self {
            a_plus,
            a_minus,
            n_plus,
            n_minus,
        };

        interval_statistics.validate()?;

        Ok(interval_statistics)
    }

    pub fn validate(self) -> Result<(), SufficientStatisticsIntervalStatisticsError> {
        if !self.a_plus.is_finite() || self.a_plus < 0.0 {
            return Err(SufficientStatisticsIntervalStatisticsError::InvalidAPlus {
                value: self.a_plus,
            });
        }
        if !self.a_minus.is_finite() || self.a_minus < 0.0 {
            return Err(SufficientStatisticsIntervalStatisticsError::InvalidAMinus {
                value: self.a_minus,
            });
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsIntervalStatisticsPayload {
    a_plus: f64,
    a_minus: f64,
    n_plus: u64,
    n_minus: u64,
}

impl<'de> Deserialize<'de> for SufficientStatisticsIntervalStatistics {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = SufficientStatisticsIntervalStatisticsPayload::deserialize(deserializer)?;

        Self::try_new(
            payload.a_plus,
            payload.a_minus,
            payload.n_plus,
            payload.n_minus,
        )
        .map_err(serde::de::Error::custom)
    }
}

fn validate_non_negative_finite_statistic(
    field: &'static str,
    value: f64,
) -> Result<(), SufficientStatisticsIntervalStatisticsError> {
    if !value.is_finite() {
        return Err(
            SufficientStatisticsIntervalStatisticsError::NonFiniteStatistic { field, value },
        );
    }
    if value < 0.0 {
        return Err(
            SufficientStatisticsIntervalStatisticsError::NegativeStatistic { field, value },
        );
    }

    Ok(())
}
/// Snapshot metadata for one deterministic boundary-state update step.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SufficientStatisticsBoundaryStateUpdateSnapshot {
    pub interval_index: usize,
    pub interval_delta: f64,
    pub state_before_update: HawkesExcitationState,
    pub sampled_statistics: SufficientStatisticsIntervalStatistics,
}

impl SufficientStatisticsBoundaryStateUpdateSnapshot {
    pub const fn new(
        interval_index: usize,
        interval_delta: f64,
        state_before_update: HawkesExcitationState,
        sampled_statistics: SufficientStatisticsIntervalStatistics,
    ) -> Self {
        Self {
            interval_index,
            interval_delta,
            state_before_update,
            sampled_statistics,
        }
    }

    pub const fn policy() -> &'static str {
        SUFFICIENT_STATISTICS_BOUNDARY_STATE_UPDATE_POLICY
    }
}
