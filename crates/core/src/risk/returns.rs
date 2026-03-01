use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Serialize;

use crate::calendar::MarketState;
use crate::nig::{NigBuySellDriverParameters, NigTradingTimeIncrementSamplingError};
use crate::randomness::SimulationRandomnessStream;
use crate::return_decomposition::{
    ReturnDecomposition, ReturnDecompositionInput, ReturnDecompositionValidationError,
};

use super::transform_sampling::{
    SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_BREAKDOWN_POLICY,
    SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_ROUTING_POLICY,
    SUFFICIENT_STATISTICS_INTERVAL_RETURN_GENERATION_POLICY,
    SufficientStatisticsIntervalStatistics, SufficientStatisticsIntervalStatisticsError,
};

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SufficientStatisticsRiskReturnComponents {
    pub buy_side_return: f64,
    pub sell_side_return: f64,
    pub trading_time_return: f64,
    pub open_jump_return: f64,
    pub closed_gap_return: f64,
    pub total_return: f64,
}

impl SufficientStatisticsRiskReturnComponents {
    pub fn try_new(
        buy_side_return: f64,
        sell_side_return: f64,
        trading_time_return: f64,
        open_jump_return: f64,
        closed_gap_return: f64,
        total_return: f64,
    ) -> Result<Self, SufficientStatisticsRiskReturnComponentsError> {
        let return_components = Self {
            buy_side_return,
            sell_side_return,
            trading_time_return,
            open_jump_return,
            closed_gap_return,
            total_return,
        };
        return_components.validate()?;
        Ok(return_components)
    }

    pub fn validate(self) -> Result<(), SufficientStatisticsRiskReturnComponentsError> {
        validate_finite_component("buy_side_return", self.buy_side_return)?;
        validate_finite_component("sell_side_return", self.sell_side_return)?;
        validate_finite_component("trading_time_return", self.trading_time_return)?;
        validate_finite_component("open_jump_return", self.open_jump_return)?;
        validate_finite_component("closed_gap_return", self.closed_gap_return)?;
        validate_finite_component("total_return", self.total_return)?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsRiskReturnComponentsError {
    InvalidFiniteComponent { field: &'static str, value: f64 },
}

impl Display for SufficientStatisticsRiskReturnComponentsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFiniteComponent { field, value } => {
                write!(formatter, "{field} must be finite, found {value}")
            }
        }
    }
}

impl Error for SufficientStatisticsRiskReturnComponentsError {}

/// Optional controls for interval return generation from sampled sufficient statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SufficientStatisticsRiskReturnGenerationOptions {
    pub include_component_breakdown: bool,
}

impl SufficientStatisticsRiskReturnGenerationOptions {
    pub const fn component_breakdown_policy() -> &'static str {
        SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_BREAKDOWN_POLICY
    }
}

impl Default for SufficientStatisticsRiskReturnGenerationOptions {
    fn default() -> Self {
        Self {
            include_component_breakdown: false,
        }
    }
}

/// One generated interval return with optional component breakdown.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsGeneratedIntervalReturn {
    pub total_return: f64,
    pub component_breakdown: Option<SufficientStatisticsRiskReturnComponents>,
}

impl SufficientStatisticsGeneratedIntervalReturn {
    pub const fn generation_policy() -> &'static str {
        SUFFICIENT_STATISTICS_INTERVAL_RETURN_GENERATION_POLICY
    }

    pub const fn component_routing_policy() -> &'static str {
        SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_ROUTING_POLICY
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsRiskReturnGenerationError {
    InvalidSampledStatistics {
        source: SufficientStatisticsIntervalStatisticsError,
    },
    InvalidOpenJumpReturn {
        value: f64,
    },
    InvalidClosedGapReturn {
        value: f64,
    },
    InvalidReturnDecomposition {
        source: ReturnDecompositionValidationError,
    },
    TradingTimeSamplingFailure {
        source: NigTradingTimeIncrementSamplingError,
    },
    InvalidReturnComponents {
        source: SufficientStatisticsRiskReturnComponentsError,
    },
    InvalidTotalReturn {
        value: f64,
    },
}

impl Display for SufficientStatisticsRiskReturnGenerationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSampledStatistics { source } => {
                write!(formatter, "sampled_statistics is invalid: {source}")
            }
            Self::InvalidOpenJumpReturn { value } => {
                write!(
                    formatter,
                    "open_jump_return input must be finite, found {value}"
                )
            }
            Self::InvalidClosedGapReturn { value } => {
                write!(
                    formatter,
                    "closed_gap_return input must be finite, found {value}"
                )
            }
            Self::InvalidReturnDecomposition { source } => {
                write!(
                    formatter,
                    "return decomposition construction failed: {source}"
                )
            }
            Self::TradingTimeSamplingFailure { source } => {
                write!(
                    formatter,
                    "trading-time increment sampling failed: {source}"
                )
            }
            Self::InvalidReturnComponents { source } => {
                write!(
                    formatter,
                    "generated return components are invalid: {source}"
                )
            }
            Self::InvalidTotalReturn { value } => {
                write!(
                    formatter,
                    "generated total_return must be finite, found {value}"
                )
            }
        }
    }
}

impl Error for SufficientStatisticsRiskReturnGenerationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidSampledStatistics { source } => Some(source),
            Self::InvalidReturnDecomposition { source } => Some(source),
            Self::TradingTimeSamplingFailure { source } => Some(source),
            Self::InvalidReturnComponents { source } => Some(source),
            _ => None,
        }
    }
}

/// Generate one interval return increment from sampled sufficient statistics.
///
/// Composition:
/// - samples trading-time increment via the shared return-decomposition API
/// - routes open-jump and closed-gap inputs by market state
/// - emits optional component breakdown for diagnostics
pub fn generate_interval_return_from_sampled_statistics<R>(
    driver_parameters: NigBuySellDriverParameters,
    sampled_statistics: SufficientStatisticsIntervalStatistics,
    market_state: MarketState,
    open_jump_return: f64,
    closed_gap_return: f64,
    randomness_stream: &mut R,
    options: Option<SufficientStatisticsRiskReturnGenerationOptions>,
) -> Result<
    SufficientStatisticsGeneratedIntervalReturn,
    SufficientStatisticsRiskReturnGenerationError,
>
where
    R: SimulationRandomnessStream,
{
    sampled_statistics.validate().map_err(|source| {
        SufficientStatisticsRiskReturnGenerationError::InvalidSampledStatistics { source }
    })?;

    if !open_jump_return.is_finite() {
        return Err(
            SufficientStatisticsRiskReturnGenerationError::InvalidOpenJumpReturn {
                value: open_jump_return,
            },
        );
    }
    if !closed_gap_return.is_finite() {
        return Err(
            SufficientStatisticsRiskReturnGenerationError::InvalidClosedGapReturn {
                value: closed_gap_return,
            },
        );
    }

    let (effective_open_jump_return, effective_closed_gap_return) =
        market_state_routed_return_jumps(market_state, open_jump_return, closed_gap_return);
    let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: sampled_statistics.a_plus,
        a_minus: sampled_statistics.a_minus,
        open_jump: effective_open_jump_return,
        closed_gap: effective_closed_gap_return,
    })
    .map_err(|source| {
        SufficientStatisticsRiskReturnGenerationError::InvalidReturnDecomposition { source }
    })?;

    let trading_time_contributions = return_decomposition
        .sample_nig_trading_time_increment(driver_parameters, randomness_stream)
        .map_err(|source| {
            SufficientStatisticsRiskReturnGenerationError::TradingTimeSamplingFailure { source }
        })?;

    let buy_side_return = trading_time_contributions.buy_side_increment;
    let sell_side_return = -trading_time_contributions.sell_side_increment;
    let trading_time_return = trading_time_contributions.combined_increment;
    let total_return =
        trading_time_return + effective_open_jump_return + effective_closed_gap_return;
    if !total_return.is_finite() {
        return Err(
            SufficientStatisticsRiskReturnGenerationError::InvalidTotalReturn {
                value: total_return,
            },
        );
    }

    let include_component_breakdown = options.unwrap_or_default().include_component_breakdown;
    let component_breakdown = if include_component_breakdown {
        Some(
            SufficientStatisticsRiskReturnComponents::try_new(
                buy_side_return,
                sell_side_return,
                trading_time_return,
                effective_open_jump_return,
                effective_closed_gap_return,
                total_return,
            )
            .map_err(|source| {
                SufficientStatisticsRiskReturnGenerationError::InvalidReturnComponents { source }
            })?,
        )
    } else {
        None
    };

    Ok(SufficientStatisticsGeneratedIntervalReturn {
        total_return,
        component_breakdown,
    })
}

fn market_state_routed_return_jumps(
    market_state: MarketState,
    open_jump_return: f64,
    closed_gap_return: f64,
) -> (f64, f64) {
    match market_state {
        MarketState::Open => (open_jump_return, 0.0),
        MarketState::Closed => (0.0, closed_gap_return),
    }
}

fn validate_finite_component(
    field: &'static str,
    value: f64,
) -> Result<(), SufficientStatisticsRiskReturnComponentsError> {
    if !value.is_finite() {
        return Err(
            SufficientStatisticsRiskReturnComponentsError::InvalidFiniteComponent { field, value },
        );
    }
    Ok(())
}
