use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::interval_factor_grids::IntervalFactorGridError;

/// Structured provider failures for missing inputs and invalid request ranges.
#[derive(Debug, Clone, PartialEq)]
pub enum IntervalFactorGridProviderError {
    MissingSeasonalitySource,
    MissingNewsIntensitySource,
    UnsupportedRange { source: IntervalFactorGridError },
    SeasonalitySourceError { source: IntervalFactorGridError },
    NewsIntensitySourceError { source: IntervalFactorGridError },
    ContractValidationError { source: IntervalFactorGridError },
}

impl Display for IntervalFactorGridProviderError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSeasonalitySource => write!(
                formatter,
                "seasonality_source is required, provide explicit `seasonality_factor` values or materialization input",
            ),
            Self::MissingNewsIntensitySource => write!(
                formatter,
                "news_intensity_source is required, provide explicit `news_intensity` values or materialization input",
            ),
            Self::UnsupportedRange { source } => {
                write!(
                    formatter,
                    "interval factor grid provider request range is unsupported: {source}"
                )
            }
            Self::SeasonalitySourceError { source } => {
                write!(formatter, "failed to resolve seasonality source: {source}")
            }
            Self::NewsIntensitySourceError { source } => {
                write!(
                    formatter,
                    "failed to resolve news intensity source: {source}"
                )
            }
            Self::ContractValidationError { source } => {
                write!(
                    formatter,
                    "failed to build interval factor grid contract: {source}"
                )
            }
        }
    }
}

impl Error for IntervalFactorGridProviderError {}
