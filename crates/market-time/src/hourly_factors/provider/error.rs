use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::hourly_factors::HourlyFactorContractError;

/// Structured provider failures for missing inputs and invalid request ranges.
#[derive(Debug, Clone, PartialEq)]
pub enum HourlyFactorProviderError {
    MissingSeasonalitySource,
    MissingNewsIntensitySource,
    UnsupportedRange { source: HourlyFactorContractError },
    SeasonalitySourceError { source: HourlyFactorContractError },
    NewsIntensitySourceError { source: HourlyFactorContractError },
    ContractValidationError { source: HourlyFactorContractError },
}

impl Display for HourlyFactorProviderError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSeasonalitySource => write!(
                formatter,
                "seasonality_source is required, provide explicit `s_h` values or materialization input",
            ),
            Self::MissingNewsIntensitySource => write!(
                formatter,
                "news_intensity_source is required, provide explicit `nu_h` values or materialization input",
            ),
            Self::UnsupportedRange { source } => {
                write!(
                    formatter,
                    "hourly factor provider request range is unsupported: {source}"
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
                    "failed to build hourly factor contract: {source}"
                )
            }
        }
    }
}

impl Error for HourlyFactorProviderError {}
