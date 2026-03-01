use std::error::Error;
use std::fmt::{Display, Formatter};

use super::validation::NigParameterConstraint;

/// Structured validation failure for `NigParameters::try_new`.
#[derive(Debug, Clone, PartialEq)]
pub enum NigParameterError {
    DomainViolation {
        field: &'static str,
        constraint: NigParameterConstraint,
        value: f64,
        comparison_field: Option<&'static str>,
        comparison_value: Option<f64>,
    },
}

impl Display for NigParameterError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DomainViolation {
                field,
                constraint,
                value,
                comparison_field,
                comparison_value,
            } => match constraint {
                NigParameterConstraint::Finite => {
                    write!(formatter, "{field} must be finite, found {value}")
                }
                NigParameterConstraint::FiniteAndStrictlyPositive => write!(
                    formatter,
                    "{field} must be finite and strictly positive, found {value}"
                ),
                NigParameterConstraint::StrictlyGreaterThanAbsoluteComparisonField => {
                    let comparison_field_name = comparison_field.unwrap_or("comparison_field");
                    let comparison_absolute_value = comparison_value.unwrap_or(f64::NAN);
                    write!(
                        formatter,
                        "{field} must be strictly greater than abs({comparison_field_name}), found {field}={value} and abs({comparison_field_name})={comparison_absolute_value}"
                    )
                }
            },
        }
    }
}

impl Error for NigParameterError {}

/// Structured validation failure for `NigSharedPlusShiftConfiguration::try_new`.
#[derive(Debug, Clone, PartialEq)]
pub enum NigSharedPlusShiftConfigurationError {
    DeltaBetaDomainViolation {
        value: f64,
    },
    BuySideParameterDomainViolation {
        source: NigParameterError,
    },
    SellSideParameterDomainViolation {
        beta_0: f64,
        delta_beta: f64,
        derived_sell_beta: f64,
        source: NigParameterError,
    },
}

impl Display for NigSharedPlusShiftConfigurationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DeltaBetaDomainViolation { value } => write!(
                formatter,
                "delta_beta must be finite and strictly negative, found {value}"
            ),
            Self::BuySideParameterDomainViolation { source } => {
                write!(formatter, "buy-side NIG parameters are invalid: {source}")
            }
            Self::SellSideParameterDomainViolation {
                beta_0,
                delta_beta,
                derived_sell_beta,
                source,
            } => write!(
                formatter,
                "sell-side NIG parameters are invalid for beta_0={beta_0}, delta_beta={delta_beta}, derived_sell_beta={derived_sell_beta}: {source}"
            ),
        }
    }
}

impl Error for NigSharedPlusShiftConfigurationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::DeltaBetaDomainViolation { .. } => None,
            Self::BuySideParameterDomainViolation { source } => Some(source),
            Self::SellSideParameterDomainViolation { source, .. } => Some(source),
        }
    }
}

/// Structured failure modes for NIG increment sampling.
#[derive(Debug, Clone, PartialEq)]
pub enum NigIncrementSamplingError {
    InvalidBusinessTimeIncrementScale { value: f64 },
    NonFiniteSubordinatorDraw { business_time_increment_scale: f64 },
    NonFiniteStandardNormalDraw { business_time_increment_scale: f64 },
    NonFiniteIncrementDraw { business_time_increment_scale: f64 },
}

impl Display for NigIncrementSamplingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidBusinessTimeIncrementScale { value } => write!(
                formatter,
                "business_time_increment_scale must be finite and non-negative, found {value}"
            ),
            Self::NonFiniteSubordinatorDraw {
                business_time_increment_scale,
            } => write!(
                formatter,
                "inverse-gaussian subordinator sampling produced a non-finite value for business_time_increment_scale={business_time_increment_scale}"
            ),
            Self::NonFiniteStandardNormalDraw {
                business_time_increment_scale,
            } => write!(
                formatter,
                "standard-normal draw produced a non-finite value for business_time_increment_scale={business_time_increment_scale}"
            ),
            Self::NonFiniteIncrementDraw {
                business_time_increment_scale,
            } => write!(
                formatter,
                "NIG increment sampling produced a non-finite value for business_time_increment_scale={business_time_increment_scale}"
            ),
        }
    }
}

impl Error for NigIncrementSamplingError {}

/// Structured failure modes for trading-time increment composition sampling.
#[derive(Debug, Clone, PartialEq)]
pub enum NigTradingTimeIncrementSamplingError {
    BuySideSamplingFailure {
        integrated_buy_intensity: f64,
        source: NigIncrementSamplingError,
    },
    SellSideSamplingFailure {
        integrated_sell_intensity: f64,
        source: NigIncrementSamplingError,
    },
}

impl Display for NigTradingTimeIncrementSamplingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BuySideSamplingFailure {
                integrated_buy_intensity,
                source,
            } => write!(
                formatter,
                "buy-side trading-time increment sampling failed for integrated_buy_intensity={integrated_buy_intensity}: {source}"
            ),
            Self::SellSideSamplingFailure {
                integrated_sell_intensity,
                source,
            } => write!(
                formatter,
                "sell-side trading-time increment sampling failed for integrated_sell_intensity={integrated_sell_intensity}: {source}"
            ),
        }
    }
}

impl Error for NigTradingTimeIncrementSamplingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::BuySideSamplingFailure { source, .. } => Some(source),
            Self::SellSideSamplingFailure { source, .. } => Some(source),
        }
    }
}
