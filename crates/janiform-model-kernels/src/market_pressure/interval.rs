use serde::{Deserialize, Serialize};

use super::contracts::{MarketPressureParameters, MarketPressureState};

/// Inputs for resolving one interval's latent side-balance transition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarketPressureIntervalTransitionInput<'a> {
    /// Latent side-balance parameters.
    pub parameters: &'a MarketPressureParameters,
    /// Boundary state at the interval start.
    pub initial_state: &'a MarketPressureState,
    /// Interval length in the market-pressure clock.
    pub interval_delta: f64,
    /// Non-negative exposure scale multiplying signed side-balance time in the anchor drift.
    pub signed_balance_exposure_scale: f64,
}

/// Conditional first moments of one latent side-balance interval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketPressureIntervalTransitionMoments {
    /// Conditional terminal state weights.
    pub terminal_state_mean: MarketPressureState,
    /// Conditional mean of `∫ b(Z_s) ds`.
    pub integrated_side_imbalance_mean: f64,
    /// Conditional mean of the scaled signed side-balance exposure.
    pub signed_balance_exposure_mean: f64,
    /// Conditional mean of the anchor drift increment.
    pub anchor_increment_mean: f64,
}

/// Resolved interval transition using the conditional mean side-balance path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketPressureIntervalTransition {
    /// Terminal market-pressure state.
    pub terminal_state: MarketPressureState,
    /// Conditional mean of `∫ b(Z_s) ds`.
    pub integrated_side_imbalance: f64,
    /// Conditional mean of the scaled signed side-balance exposure.
    pub signed_balance_exposure: f64,
    /// Log-anchor increment induced by the integrated signed exposure.
    pub anchor_increment: f64,
}

/// Sampled interval transition from one latent CTMC path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketPressureSampledIntervalTransition {
    /// Sampled terminal market-pressure state.
    pub terminal_state: MarketPressureState,
    /// Sampled integral of signed side imbalance over the interval.
    pub integrated_side_imbalance: f64,
    /// Sampled scaled signed side-balance exposure.
    pub signed_balance_exposure: f64,
    /// Sampled anchor drift increment.
    pub anchor_increment: f64,
}
