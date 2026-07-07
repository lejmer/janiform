use serde::{Deserialize, Serialize};

use crate::affine_oracle::{
    ExponentialAffineTransformEquationSpecification, ExponentialAffineTransformNewsIntensityInput,
};
use janiform_foundations::complex_math::ComplexScalar;
use janiform_model_kernels::hawkes::HawkesExcitationState;
use janiform_model_kernels::market_pressure::MarketPressureState;

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
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsTransformSliceContext {
    pub interval_index: usize,
    pub seasonality_factor: f64,
    pub news_intensity: ExponentialAffineTransformNewsIntensityInput,
    pub delta: f64,
    pub initial_excitation_state: HawkesExcitationState,
    pub initial_market_pressure_state: MarketPressureState,
}

impl SufficientStatisticsTransformSliceContext {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        interval_index: usize,
        seasonality_factor: f64,
        news_intensity: ExponentialAffineTransformNewsIntensityInput,
        delta: f64,
        initial_excitation_state: HawkesExcitationState,
        initial_market_pressure_state: MarketPressureState,
    ) -> Result<Self, SufficientStatisticsTransformSliceContextError> {
        let context = Self {
            interval_index,
            seasonality_factor,
            news_intensity,
            delta,
            initial_excitation_state,
            initial_market_pressure_state,
        };

        context.validate()?;

        Ok(context)
    }

    pub fn validate(&self) -> Result<(), SufficientStatisticsTransformSliceContextError> {
        if !self.seasonality_factor.is_finite() || self.seasonality_factor < 0.0 {
            return Err(
                SufficientStatisticsTransformSliceContextError::InvalidSeasonalityFactor {
                    interval_index: self.interval_index,
                    value: self.seasonality_factor,
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

        self.initial_market_pressure_state
            .validate()
            .map_err(|source| {
                SufficientStatisticsTransformSliceContextError::InvalidInitialMarketPressureState {
                    interval_index: self.interval_index,
                    error_message: source.to_string(),
                }
            })?;

        Ok(())
    }
}

/// Probe settings for transform-slice extraction used by the sufficient-statistics sampler.
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
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsTransformSlices {
    pub equation_specification: ExponentialAffineTransformEquationSpecification,
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
    pub expected_baseline_a_plus: f64,
    pub expected_endogenous_a_plus: f64,
    pub expected_baseline_a_minus: f64,
    pub expected_endogenous_a_minus: f64,
    pub expected_n_plus: f64,
    pub expected_n_minus: f64,
}

impl SufficientStatisticsExpectedIntervalStatistics {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        expected_baseline_a_plus: f64,
        expected_endogenous_a_plus: f64,
        expected_baseline_a_minus: f64,
        expected_endogenous_a_minus: f64,
        expected_n_plus: f64,
        expected_n_minus: f64,
    ) -> Result<Self, SufficientStatisticsIntervalStatisticsError> {
        let expected_a_plus = expected_baseline_a_plus + expected_endogenous_a_plus;
        let expected_a_minus = expected_baseline_a_minus + expected_endogenous_a_minus;

        let statistics = Self {
            expected_a_plus,
            expected_a_minus,
            expected_baseline_a_plus,
            expected_endogenous_a_plus,
            expected_baseline_a_minus,
            expected_endogenous_a_minus,
            expected_n_plus,
            expected_n_minus,
        };

        statistics.validate()?;

        Ok(statistics)
    }

    pub fn validate(self) -> Result<(), SufficientStatisticsIntervalStatisticsError> {
        validate_non_negative_finite_statistic("expected_a_plus", self.expected_a_plus)?;
        validate_non_negative_finite_statistic("expected_a_minus", self.expected_a_minus)?;

        validate_non_negative_finite_statistic(
            "expected_baseline_a_plus",
            self.expected_baseline_a_plus,
        )?;

        validate_non_negative_finite_statistic(
            "expected_endogenous_a_plus",
            self.expected_endogenous_a_plus,
        )?;

        validate_non_negative_finite_statistic(
            "expected_baseline_a_minus",
            self.expected_baseline_a_minus,
        )?;

        validate_non_negative_finite_statistic(
            "expected_endogenous_a_minus",
            self.expected_endogenous_a_minus,
        )?;

        validate_non_negative_finite_statistic("expected_n_plus", self.expected_n_plus)?;
        validate_non_negative_finite_statistic("expected_n_minus", self.expected_n_minus)?;

        Ok(())
    }
}

/// Sampled buy and sell event counts for one interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SufficientStatisticsSampledEventCounts {
    pub n_plus: u64,
    pub n_minus: u64,
}

impl SufficientStatisticsSampledEventCounts {
    pub const fn new(n_plus: u64, n_minus: u64) -> Self {
        Self { n_plus, n_minus }
    }
}

/// Hidden-placement transition implied by sampled sufficient-statistics event counts.
#[derive(Debug, Clone, PartialEq)]
pub struct SufficientStatisticsSampledIntervalTransition {
    /// Interval-end Hawkes excitation state after hidden event placements are applied.
    pub terminal_excitation_state: HawkesExcitationState,

    /// Interval-end market-pressure state after hidden event placements are applied.
    pub terminal_market_pressure_state: MarketPressureState,

    /// Market-pressure anchor increment integrated over the interval.
    pub market_pressure_anchor_increment: f64,
}

impl SufficientStatisticsTransformSlices {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        equation_specification: ExponentialAffineTransformEquationSpecification,
        context: SufficientStatisticsTransformSliceContext,
        settings: SufficientStatisticsTransformSliceSettings,
        baseline_transform: ComplexScalar,
        a_plus_transform: ComplexScalar,
        a_minus_transform: ComplexScalar,
        n_plus_transform: ComplexScalar,
        n_minus_transform: ComplexScalar,
    ) -> Result<Self, SufficientStatisticsTransformSlicesError> {
        let slices = Self {
            equation_specification,
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

    pub fn validate(&self) -> Result<(), SufficientStatisticsTransformSlicesError> {
        self.equation_specification.validate().map_err(|source| {
            SufficientStatisticsTransformSlicesError::InvalidEquationSpecification { source }
        })?;

        self.context.validate().map_err(|source| {
            SufficientStatisticsTransformSlicesError::InvalidContext { source }
        })?;
        self.context
            .initial_market_pressure_state
            .validate_for_parameters(&self.equation_specification.market_pressure_parameters)
            .map_err(|source| {
                SufficientStatisticsTransformSlicesError::InvalidContext {
                    source: SufficientStatisticsTransformSliceContextError::InvalidInitialMarketPressureState {
                        interval_index: self.context.interval_index,
                        error_message: source.to_string(),
                    },
                }
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
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsIntervalStatistics {
    pub a_plus: f64,
    pub a_minus: f64,
    pub baseline_a_plus: f64,
    pub endogenous_a_plus: f64,
    pub baseline_a_minus: f64,
    pub endogenous_a_minus: f64,
    pub n_plus: u64,
    pub n_minus: u64,
    pub terminal_excitation_state: HawkesExcitationState,
    pub terminal_market_pressure_state: MarketPressureState,
    pub market_pressure_anchor_increment: f64,
}

impl SufficientStatisticsIntervalStatistics {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        baseline_a_plus: f64,
        endogenous_a_plus: f64,
        baseline_a_minus: f64,
        endogenous_a_minus: f64,
        n_plus: u64,
        n_minus: u64,
        terminal_excitation_state: HawkesExcitationState,
        terminal_market_pressure_state: MarketPressureState,
        market_pressure_anchor_increment: f64,
    ) -> Result<Self, SufficientStatisticsIntervalStatisticsError> {
        let a_plus = baseline_a_plus + endogenous_a_plus;
        let a_minus = baseline_a_minus + endogenous_a_minus;

        let interval_statistics = Self {
            a_plus,
            a_minus,
            baseline_a_plus,
            endogenous_a_plus,
            baseline_a_minus,
            endogenous_a_minus,
            n_plus,
            n_minus,
            terminal_excitation_state,
            terminal_market_pressure_state,
            market_pressure_anchor_increment,
        };

        interval_statistics.validate()?;

        Ok(interval_statistics)
    }

    pub fn validate(&self) -> Result<(), SufficientStatisticsIntervalStatisticsError> {
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

        validate_non_negative_finite_statistic("baseline_a_plus", self.baseline_a_plus)?;
        validate_non_negative_finite_statistic("endogenous_a_plus", self.endogenous_a_plus)?;
        validate_non_negative_finite_statistic("baseline_a_minus", self.baseline_a_minus)?;
        validate_non_negative_finite_statistic("endogenous_a_minus", self.endogenous_a_minus)?;

        validate_activity_total(
            "a_plus",
            self.a_plus,
            self.baseline_a_plus,
            self.endogenous_a_plus,
        )?;

        validate_activity_total(
            "a_minus",
            self.a_minus,
            self.baseline_a_minus,
            self.endogenous_a_minus,
        )?;

        validate_excitation_state(self.terminal_excitation_state).map_err(|(field, value)| {
            SufficientStatisticsIntervalStatisticsError::InvalidTerminalExcitationState {
                field,
                value,
            }
        })?;

        self.terminal_market_pressure_state
            .validate()
            .map_err(|source| {
                SufficientStatisticsIntervalStatisticsError::InvalidTerminalMarketPressureState {
                    error_message: source.to_string(),
                }
            })?;

        validate_finite_statistic(
            "market_pressure_anchor_increment",
            self.market_pressure_anchor_increment,
        )?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsIntervalStatisticsPayload {
    a_plus: f64,
    a_minus: f64,
    baseline_a_plus: f64,
    endogenous_a_plus: f64,
    baseline_a_minus: f64,
    endogenous_a_minus: f64,
    n_plus: u64,
    n_minus: u64,
    terminal_excitation_state: HawkesExcitationState,
    terminal_market_pressure_state: MarketPressureState,
    market_pressure_anchor_increment: f64,
}

impl<'de> Deserialize<'de> for SufficientStatisticsIntervalStatistics {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = SufficientStatisticsIntervalStatisticsPayload::deserialize(deserializer)?;

        let statistics = Self::try_new(
            payload.baseline_a_plus,
            payload.endogenous_a_plus,
            payload.baseline_a_minus,
            payload.endogenous_a_minus,
            payload.n_plus,
            payload.n_minus,
            payload.terminal_excitation_state,
            payload.terminal_market_pressure_state,
            payload.market_pressure_anchor_increment,
        )
        .map_err(serde::de::Error::custom)?;

        if statistics.a_plus != payload.a_plus {
            return Err(serde::de::Error::custom(
                SufficientStatisticsIntervalStatisticsError::InconsistentActivityTotal {
                    total_field: "a_plus",
                    total: payload.a_plus,
                    baseline: payload.baseline_a_plus,
                    endogenous: payload.endogenous_a_plus,
                },
            ));
        }

        if statistics.a_minus != payload.a_minus {
            return Err(serde::de::Error::custom(
                SufficientStatisticsIntervalStatisticsError::InconsistentActivityTotal {
                    total_field: "a_minus",
                    total: payload.a_minus,
                    baseline: payload.baseline_a_minus,
                    endogenous: payload.endogenous_a_minus,
                },
            ));
        }

        Ok(statistics)
    }
}

fn validate_activity_total(
    total_field: &'static str,
    total: f64,
    baseline: f64,
    endogenous: f64,
) -> Result<(), SufficientStatisticsIntervalStatisticsError> {
    if total != baseline + endogenous {
        return Err(
            SufficientStatisticsIntervalStatisticsError::InconsistentActivityTotal {
                total_field,
                total,
                baseline,
                endogenous,
            },
        );
    }

    Ok(())
}

fn validate_non_negative_finite_statistic(
    field: &'static str,
    value: f64,
) -> Result<(), SufficientStatisticsIntervalStatisticsError> {
    validate_finite_statistic(field, value)?;

    if value < 0.0 {
        return Err(
            SufficientStatisticsIntervalStatisticsError::NegativeStatistic { field, value },
        );
    }

    Ok(())
}

fn validate_finite_statistic(
    field: &'static str,
    value: f64,
) -> Result<(), SufficientStatisticsIntervalStatisticsError> {
    if !value.is_finite() {
        return Err(
            SufficientStatisticsIntervalStatisticsError::NonFiniteStatistic { field, value },
        );
    }

    Ok(())
}

/// Boundary state carried between sufficient-statistics intervals.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsBoundaryState {
    pub excitation_state: HawkesExcitationState,
    pub market_pressure_state: MarketPressureState,
}

impl SufficientStatisticsBoundaryState {
    pub fn new(
        excitation_state: HawkesExcitationState,
        market_pressure_state: MarketPressureState,
    ) -> Self {
        Self {
            excitation_state,
            market_pressure_state,
        }
    }
}

/// Snapshot metadata for one deterministic boundary-state update step.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsBoundaryStateUpdateSnapshot {
    pub interval_index: usize,
    pub interval_delta: f64,
    pub state_before_update: SufficientStatisticsBoundaryState,
    pub sampled_statistics: SufficientStatisticsIntervalStatistics,
}

impl SufficientStatisticsBoundaryStateUpdateSnapshot {
    pub fn new(
        interval_index: usize,
        interval_delta: f64,
        state_before_update: SufficientStatisticsBoundaryState,
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
