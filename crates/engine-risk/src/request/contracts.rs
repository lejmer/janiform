use serde::Serialize;

pub use janiform_affine_oracle::sufficient_statistics::{
    SufficientStatisticsRiskIntervalFactors, SufficientStatisticsRiskModelInputs,
};

use super::constants::{
    DEFAULT_INTRADAY_RISK_INTERVAL_DELTA, SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION,
};
use super::errors::SufficientStatisticsRiskRunRequestError;

/// Public run request contract for sufficient-statistics risk simulation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsRiskRunRequest {
    pub seed: u64,
    pub horizon_interval_count: usize,
    pub interval_delta: f64,
    pub model_inputs: SufficientStatisticsRiskModelInputs,
    pub interval_factors: SufficientStatisticsRiskIntervalFactors,
}

impl SufficientStatisticsRiskRunRequest {
    pub fn try_new(
        seed: u64,
        horizon_interval_count: usize,
        interval_delta: f64,
        model_inputs: SufficientStatisticsRiskModelInputs,
        interval_factors: SufficientStatisticsRiskIntervalFactors,
    ) -> Result<Self, SufficientStatisticsRiskRunRequestError> {
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

    pub fn validate(&self) -> Result<(), SufficientStatisticsRiskRunRequestError> {
        if self.horizon_interval_count == 0 {
            return Err(
                SufficientStatisticsRiskRunRequestError::InvalidHorizonIntervalCount { value: 0 },
            );
        }

        if !self.interval_delta.is_finite() || self.interval_delta <= 0.0 {
            return Err(
                SufficientStatisticsRiskRunRequestError::InvalidIntervalDelta {
                    value: self.interval_delta,
                },
            );
        }

        self.model_inputs.validate().map_err(|source| {
            SufficientStatisticsRiskRunRequestError::InvalidModelInputs { source }
        })?;

        self.interval_factors.validate().map_err(|source| {
            SufficientStatisticsRiskRunRequestError::InvalidIntervalFactors { source }
        })?;

        let resolved_factor_interval_delta = self.interval_factors.resolved_interval_delta();
        if self.interval_delta != resolved_factor_interval_delta {
            return Err(
                SufficientStatisticsRiskRunRequestError::IntervalDeltaFactorMismatch {
                    requested_interval_delta: self.interval_delta,
                    available_factor_interval_delta: resolved_factor_interval_delta,
                },
            );
        }

        let available_interval_count = self.interval_factors.s_h.len();
        if self.horizon_interval_count > available_interval_count {
            return Err(
                SufficientStatisticsRiskRunRequestError::HorizonExceedsAvailableIntervals {
                    requested_interval_count: self.horizon_interval_count,
                    available_interval_count,
                },
            );
        }

        Ok(())
    }

    pub const fn schema_version() -> &'static str {
        SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION
    }

    pub const fn default_interval_delta() -> f64 {
        DEFAULT_INTRADAY_RISK_INTERVAL_DELTA
    }
}
