use std::path::PathBuf;

use chrono::{DateTime, Utc};
use janiform::hawkes::HawkesExcitationState;
use janiform::pricing::{
    PricingIntervalSchedulePolicy, VanillaOptionPayoffType,
    VanillaOptionPricingOptionalOutputSelection,
};
use janiform_foundations::time::deserialize_utc_datetime;
use serde::Deserialize;
use serde_json::Value;

/// Continuation observations used by fitted-model pricing.
pub use crate::model::ModelStateObservationData as FittedModelPricingContinuationData;

/// Explicit fitted-model single-option pricing request.
#[derive(Debug, Deserialize)]
pub struct FittedModelSingleOptionPricingRequest {
    /// Persisted `JaniformModel` payload.
    pub model_payload: Value,
    /// Optional stable pricing run id.
    pub run_id: Option<String>,
    /// Pricing valuation timestamp.
    #[serde(deserialize_with = "deserialize_utc_datetime")]
    pub valuation_time_utc: DateTime<Utc>,
    /// Current spot price.
    pub spot: f64,
    /// Vanilla option strike.
    pub strike: f64,
    /// Exact option expiry timestamp.
    pub expiry_timestamp_utc: Option<String>,
    /// Option expiry date in YYYY-MM-DD form.
    pub expiry_date: Option<String>,
    /// Relative option tenor.
    pub tenor: Option<String>,
    /// Vanilla payoff side.
    pub payoff_type: VanillaOptionPayoffType,
    /// Continuously compounded risk-free rate.
    pub risk_free_rate: f64,
    /// Continuously compounded dividend yield.
    pub dividend_yield: f64,
    /// Optional pricing output selections.
    #[serde(default)]
    pub optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    /// Pricing interval schedule policy.
    #[serde(default)]
    pub pricing_interval_schedule_policy: PricingIntervalSchedulePolicy,
    /// Optional continuation observations used to estimate the pricing boundary state.
    pub continuation_data: Option<FittedModelPricingContinuationData>,
    /// Optional explicit initial Hawkes state.
    pub initial_excitation_state: Option<HawkesExcitationState>,
    /// Primary output path.
    pub output_path: Option<PathBuf>,
}

/// Workflow request for fitted-model single-option pricing.
#[derive(Debug)]
pub struct FittedModelSingleOptionPricingWorkflowRequest {
    /// Pricing execution request.
    pub pricing_request: FittedModelSingleOptionPricingRequest,
    /// Optional persisted model path when the boundary loaded the artifact from disk.
    pub model_path: Option<PathBuf>,
}

impl FittedModelSingleOptionPricingWorkflowRequest {
    /// Build a workflow request from a pricing request with no boundary model path.
    pub fn from_pricing_request(pricing_request: FittedModelSingleOptionPricingRequest) -> Self {
        Self {
            pricing_request,
            model_path: None,
        }
    }
}
