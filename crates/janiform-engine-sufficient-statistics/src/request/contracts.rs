use serde::Serialize;

use janiform_affine_oracle::sufficient_statistics::{
    DEFAULT_INTRADAY_SUFFICIENT_STATISTICS_INTERVAL_DELTA, SufficientStatisticsIntervalFactors,
    SufficientStatisticsModelInputs,
};
use janiform_foundations::time::duration_hours_to_nanos;

use super::constants::SUFFICIENT_STATISTICS_RUN_REQUEST_SCHEMA_VERSION;
use super::errors::SufficientStatisticsRunRequestError;

/// Public run request contract for sufficient-statistics simulation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsRunRequest {
    pub seed: u64,
    pub horizon_interval_count: usize,
    pub interval_delta: f64,
    pub model_inputs: SufficientStatisticsModelInputs,
    pub interval_factor_grids: SufficientStatisticsIntervalFactors,
}

impl SufficientStatisticsRunRequest {
    pub fn try_new(
        seed: u64,
        horizon_interval_count: usize,
        interval_delta: f64,
        model_inputs: SufficientStatisticsModelInputs,
        interval_factor_grids: SufficientStatisticsIntervalFactors,
    ) -> Result<Self, SufficientStatisticsRunRequestError> {
        let run_request = Self {
            seed,
            horizon_interval_count,
            interval_delta,
            model_inputs,
            interval_factor_grids,
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

        let interval_delta_nanos = duration_hours_to_nanos(self.interval_delta).map_err(|_| {
            SufficientStatisticsRunRequestError::InvalidIntervalDelta {
                value: self.interval_delta,
            }
        })?;

        self.model_inputs
            .validate()
            .map_err(|source| SufficientStatisticsRunRequestError::InvalidModelInputs { source })?;

        self.interval_factor_grids.validate().map_err(|source| {
            SufficientStatisticsRunRequestError::InvalidIntervalFactors { source }
        })?;

        let resolved_factor_interval_delta = self.interval_factor_grids.resolved_interval_delta();
        if duration_hours_to_nanos(resolved_factor_interval_delta) != Ok(interval_delta_nanos) {
            return Err(
                SufficientStatisticsRunRequestError::IntervalDeltaFactorMismatch {
                    requested_interval_delta: self.interval_delta,
                    available_factor_interval_delta: resolved_factor_interval_delta,
                },
            );
        }

        let available_interval_count = self.interval_factor_grids.seasonality_factor.len();
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
            if duration_hours_to_nanos(observed_interval_delta) != Ok(interval_delta_nanos) {
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
