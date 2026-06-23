use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_affine_oracle::sufficient_statistics::{
    SufficientStatisticsIntervalStatistics, SufficientStatisticsIntervalStatisticsError,
    SufficientStatisticsReturnComponents, SufficientStatisticsReturnComponentsError,
};
use janiform_model_kernels::mean_reversion::MeanReversionBoundaryStateValidationError;
use serde::{Deserialize, Serialize};

use super::constants::COARSE_SIMULATION_PATH_RECORD_SCHEMA_VERSION;
use super::time_index::{
    CoarseSimulationIntervalBoundsError, ExactStringValidationError, validate_exact_string,
    validate_interval_bounds,
};

/// One time-indexed interval record in a coarse simulation path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoarseSimulationPathRecord {
    pub schema_version: String,
    pub interval_index: usize,
    pub interval_start_utc_nanos: i64,
    pub interval_end_utc_nanos: i64,
    pub sufficient_statistics: SufficientStatisticsIntervalStatistics,
    pub return_components: SufficientStatisticsReturnComponents,
    pub core_total_return: f64,
    pub observed_interval_return: f64,
    pub observed_log_price_end: f64,
    pub anchor_value_end: f64,
    pub deviation_from_anchor_end: f64,
}

impl CoarseSimulationPathRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        interval_index: usize,
        interval_start_utc_nanos: i64,
        interval_end_utc_nanos: i64,
        sufficient_statistics: SufficientStatisticsIntervalStatistics,
        return_components: SufficientStatisticsReturnComponents,
        core_total_return: f64,
        observed_interval_return: f64,
        observed_log_price_end: f64,
        anchor_value_end: f64,
        deviation_from_anchor_end: f64,
    ) -> Result<Self, CoarseSimulationPathRecordError> {
        let record = Self {
            schema_version: COARSE_SIMULATION_PATH_RECORD_SCHEMA_VERSION.to_string(),
            interval_index,
            interval_start_utc_nanos,
            interval_end_utc_nanos,
            sufficient_statistics,
            return_components,
            core_total_return,
            observed_interval_return,
            observed_log_price_end,
            anchor_value_end,
            deviation_from_anchor_end,
        };

        record.validate()?;

        Ok(record)
    }

    pub fn validate(&self) -> Result<(), CoarseSimulationPathRecordError> {
        validate_exact_string(
            "schema_version",
            &self.schema_version,
            COARSE_SIMULATION_PATH_RECORD_SCHEMA_VERSION,
        )
        .map_err(CoarseSimulationPathRecordError::InvalidSchemaVersion)?;

        validate_interval_bounds(self.interval_start_utc_nanos, self.interval_end_utc_nanos)
            .map_err(CoarseSimulationPathRecordError::InvalidIntervalBounds)?;

        self.sufficient_statistics.validate().map_err(|source| {
            CoarseSimulationPathRecordError::InvalidSufficientStatistics { source }
        })?;

        self.return_components.validate().map_err(|source| {
            CoarseSimulationPathRecordError::InvalidReturnComponents { source }
        })?;

        if self.core_total_return != self.return_components.total_return {
            return Err(CoarseSimulationPathRecordError::CoreTotalReturnMismatch {
                expected_core_total_return: self.return_components.total_return,
                observed_core_total_return: self.core_total_return,
            });
        }

        if !self.observed_interval_return.is_finite() {
            return Err(
                CoarseSimulationPathRecordError::InvalidObservedIntervalReturn {
                    value: self.observed_interval_return,
                },
            );
        }

        janiform_model_kernels::mean_reversion::MeanReversionBoundaryState::try_new(
            self.observed_log_price_end,
            self.anchor_value_end,
            self.deviation_from_anchor_end,
        )
        .map_err(CoarseSimulationPathRecordError::InvalidObservedBoundaryState)?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoarseSimulationPathRecordError {
    InvalidSchemaVersion(ExactStringValidationError),
    InvalidIntervalBounds(CoarseSimulationIntervalBoundsError),
    InvalidSufficientStatistics {
        source: SufficientStatisticsIntervalStatisticsError,
    },
    InvalidReturnComponents {
        source: SufficientStatisticsReturnComponentsError,
    },
    CoreTotalReturnMismatch {
        expected_core_total_return: f64,
        observed_core_total_return: f64,
    },
    InvalidObservedIntervalReturn {
        value: f64,
    },
    InvalidObservedBoundaryState(MeanReversionBoundaryStateValidationError),
}

impl Display for CoarseSimulationPathRecordError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSchemaVersion(source) => {
                write!(formatter, "schema_version is invalid: {source}")
            }
            Self::InvalidIntervalBounds(source) => {
                write!(
                    formatter,
                    "interval boundary timestamps are invalid: {source}"
                )
            }
            Self::InvalidSufficientStatistics { source } => {
                write!(formatter, "sufficient_statistics is invalid: {source}")
            }
            Self::InvalidReturnComponents { source } => {
                write!(formatter, "return_components is invalid: {source}")
            }
            Self::CoreTotalReturnMismatch {
                expected_core_total_return,
                observed_core_total_return,
            } => write!(
                formatter,
                "core_total_return must equal return_components.total_return ({expected_core_total_return}), found {observed_core_total_return}"
            ),
            Self::InvalidObservedIntervalReturn { value } => write!(
                formatter,
                "observed_interval_return must be finite, found {value}"
            ),
            Self::InvalidObservedBoundaryState(source) => {
                write!(
                    formatter,
                    "observed end-of-interval boundary state is invalid: {source}"
                )
            }
        }
    }
}

impl Error for CoarseSimulationPathRecordError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidSchemaVersion(source) => Some(source),
            Self::InvalidIntervalBounds(source) => Some(source),
            Self::InvalidSufficientStatistics { source } => Some(source),
            Self::InvalidReturnComponents { source } => Some(source),
            Self::InvalidObservedBoundaryState(source) => Some(source),
            Self::CoreTotalReturnMismatch { .. } | Self::InvalidObservedIntervalReturn { .. } => {
                None
            }
        }
    }
}
