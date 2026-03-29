use janiform_foundations::numeric::validate_finite_number;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsRiskReturnComponentsError {
    InvalidFiniteComponent { field: &'static str, value: f64 },
}

impl std::fmt::Display for SufficientStatisticsRiskReturnComponentsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFiniteComponent { field, value } => {
                write!(formatter, "{field} must be finite, found {value}")
            }
        }
    }
}

impl std::error::Error for SufficientStatisticsRiskReturnComponentsError {}

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

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsRiskReturnComponentsPayload {
    buy_side_return: f64,
    sell_side_return: f64,
    trading_time_return: f64,
    open_jump_return: f64,
    closed_gap_return: f64,
    total_return: f64,
}

impl<'de> Deserialize<'de> for SufficientStatisticsRiskReturnComponents {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = SufficientStatisticsRiskReturnComponentsPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.buy_side_return,
            payload.sell_side_return,
            payload.trading_time_return,
            payload.open_jump_return,
            payload.closed_gap_return,
            payload.total_return,
        )
        .map_err(serde::de::Error::custom)
    }
}

fn validate_finite_component(
    field: &'static str,
    value: f64,
) -> Result<(), SufficientStatisticsRiskReturnComponentsError> {
    validate_finite_number(value, field).map_err(|_| {
        SufficientStatisticsRiskReturnComponentsError::InvalidFiniteComponent { field, value }
    })
}
