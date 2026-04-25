use serde::Serialize;

pub use janiform_affine_oracle::sufficient_statistics::{
    SufficientStatisticsIntervalFactors, SufficientStatisticsModelInputs,
};

use super::constants::{
    DEFAULT_INTRADAY_SUFFICIENT_STATISTICS_INTERVAL_DELTA,
    SUFFICIENT_STATISTICS_RUN_REQUEST_SCHEMA_VERSION,
};
use super::errors::SufficientStatisticsRunRequestError;

const MEAN_REVERSION_ALIGNMENT_TOLERANCE: f64 = 1.0e-12;

/// Public run request contract for sufficient-statistics simulation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsRunRequest {
    pub seed: u64,
    pub horizon_interval_count: usize,
    pub interval_delta: f64,
    pub model_inputs: SufficientStatisticsModelInputs,
    pub interval_factors: SufficientStatisticsIntervalFactors,
}

impl SufficientStatisticsRunRequest {
    pub fn try_new(
        seed: u64,
        horizon_interval_count: usize,
        interval_delta: f64,
        model_inputs: SufficientStatisticsModelInputs,
        interval_factors: SufficientStatisticsIntervalFactors,
    ) -> Result<Self, SufficientStatisticsRunRequestError> {
        let run_request = Self {
            seed,
            horizon_interval_count,
            interval_delta,
            model_inputs,
            interval_factors,
        };
        run_request.validate()?;
        Ok(run_request)
    }

    pub fn validate(&self) -> Result<(), SufficientStatisticsRunRequestError> {
        if self.horizon_interval_count == 0 {
            return Err(
                SufficientStatisticsRunRequestError::InvalidHorizonIntervalCount { value: 0 },
            );
        }

        if !self.interval_delta.is_finite() || self.interval_delta <= 0.0 {
            return Err(SufficientStatisticsRunRequestError::InvalidIntervalDelta {
                value: self.interval_delta,
            });
        }

        self.model_inputs
            .validate()
            .map_err(|source| SufficientStatisticsRunRequestError::InvalidModelInputs { source })?;

        self.interval_factors.validate().map_err(|source| {
            SufficientStatisticsRunRequestError::InvalidIntervalFactors { source }
        })?;

        let resolved_factor_interval_delta = self.interval_factors.resolved_interval_delta();
        if self.interval_delta != resolved_factor_interval_delta {
            return Err(
                SufficientStatisticsRunRequestError::IntervalDeltaFactorMismatch {
                    requested_interval_delta: self.interval_delta,
                    available_factor_interval_delta: resolved_factor_interval_delta,
                },
            );
        }

        let available_interval_count = self.interval_factors.s_h.len();
        if self.horizon_interval_count > available_interval_count {
            return Err(
                SufficientStatisticsRunRequestError::HorizonExceedsAvailableIntervals {
                    requested_interval_count: self.horizon_interval_count,
                    available_interval_count,
                },
            );
        }

        let runtime_interval_anchors = &self.model_inputs.mean_reversion_runtime_interval_anchors;
        if runtime_interval_anchors.end_anchor_values.len() != self.horizon_interval_count {
            return Err(
                SufficientStatisticsRunRequestError::MeanReversionRuntimeAnchorIntervalCountMismatch {
                    expected_interval_count: self.horizon_interval_count,
                    observed_interval_count: runtime_interval_anchors.end_anchor_values.len(),
                },
            );
        }

        for (interval_index, observed_interval_delta) in runtime_interval_anchors
            .interval_deltas
            .iter()
            .copied()
            .enumerate()
        {
            if (observed_interval_delta - self.interval_delta).abs()
                > MEAN_REVERSION_ALIGNMENT_TOLERANCE
            {
                return Err(
                    SufficientStatisticsRunRequestError::MeanReversionRuntimeAnchorIntervalDeltaMismatch {
                        interval_index,
                        expected_interval_delta: self.interval_delta,
                        observed_interval_delta,
                    },
                );
            }

            if interval_index + 1 == self.horizon_interval_count {
                break;
            }
        }

        Ok(())
    }

    pub const fn schema_version() -> &'static str {
        SUFFICIENT_STATISTICS_RUN_REQUEST_SCHEMA_VERSION
    }

    pub const fn default_interval_delta() -> f64 {
        DEFAULT_INTRADAY_SUFFICIENT_STATISTICS_INTERVAL_DELTA
    }
}
