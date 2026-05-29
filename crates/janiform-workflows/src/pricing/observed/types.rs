use std::path::PathBuf;

use chrono::{DateTime, Utc};
use janiform::calibration::market_data::{
    MarketDataInputFileReference, MarketDataNormalizationPolicy,
    MarketDataOptionSurfaceColumnMapping, MarketDataTickDataColumnMapping,
};
use janiform::model::factors::hourly::NANOS_PER_HOUR;
use janiform::model::hawkes::HawkesEventSide;
use janiform::pricing::{
    PricingIntervalSchedulePolicy, VanillaOptionPayoffType,
    VanillaOptionPricingOptionalOutputSelection,
};
use janiform_calibration_runtime::CurrentStateEstimationOutput;
use janiform_engine_pricing::{VanillaOptionPricingFailureStage, VanillaOptionPricingStatus};
use janiform_foundations::time::{deserialize_optional_utc_datetime, serialize_utc_datetime};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::WorkflowError;

use super::super::artifacts::PricingArtifactPaths;
use super::physical_distribution::{
    ObservedOptionsPhysicalDistributionDiagnosticsOutput,
    ObservedOptionsPhysicalDistributionDiagnosticsRequest,
};

pub(super) const DEFAULT_PRICING_INTERVAL_DURATION_NANOS: i64 = NANOS_PER_HOUR;
pub(super) const LOAD_OBSERVED_DATA_STAGE: &str = "load observed data";
pub(super) const PREPARE_VALUATION_STATES_STAGE: &str = "prepare valuation states";
pub(super) const PRICE_VALUATION_SURFACE_STAGE: &str = "price valuation surface";
pub(super) const PHYSICAL_DISTRIBUTION_DIAGNOSTICS_STAGE: &str = "diagnose physical distribution";
pub(super) const WRITE_ARTIFACTS_STAGE: &str = "write artifacts";
pub(super) const SURFACE_TASK_FINALIZATION_UNIT: usize = 1;

/// Observed-options pricing scope.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ObservedOptionsPricingScope {
    /// Price the latest observed snapshot at one valuation time.
    Snapshot,
    /// Price a surface panel for one expiry, or fall back to a one-time snapshot for multiple expiries.
    Surface,
}

/// Observed option type filter.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ObservedOptionTypeFilter {
    Call,
    Put,
}

impl ObservedOptionTypeFilter {
    pub(super) fn payoff_type(self) -> VanillaOptionPayoffType {
        match self {
            Self::Call => VanillaOptionPayoffType::Call,
            Self::Put => VanillaOptionPayoffType::Put,
        }
    }
}

/// Rust-owned observed-options pricing request.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedOptionsPricingRequest {
    /// Persisted `JaniformModel` payload.
    pub model_payload: Value,
    /// Optional stable pricing run id.
    pub run_id: Option<String>,
    /// Observed tick source.
    pub tick_data_source: MarketDataInputFileReference,
    /// Tick source column mapping.
    pub tick_data_column_mapping: MarketDataTickDataColumnMapping,
    /// Observed option surface source.
    pub option_surface_source: MarketDataInputFileReference,
    /// Option source column mapping.
    pub option_surface_column_mapping: MarketDataOptionSurfaceColumnMapping,
    /// Shared market-data normalization policy.
    pub normalization_policy: MarketDataNormalizationPolicy,
    /// Optional valuation timestamp. Required when snapshot inputs contain more than one quote time.
    #[serde(default, deserialize_with = "deserialize_optional_utc_datetime")]
    pub valuation_time_utc: Option<DateTime<Utc>>,
    /// Optional option type filter.
    pub option_type: Option<ObservedOptionTypeFilter>,
    /// Pricing workflow scope.
    pub scope: ObservedOptionsPricingScope,
    /// Continuously compounded risk-free rate.
    pub risk_free_rate: f64,
    /// Continuously compounded dividend yield.
    pub dividend_yield: f64,
    /// Surface valuation count selection.
    pub valuation_count: Option<usize>,
    /// Include every retained valuation time for single-expiry surface pricing.
    #[serde(default)]
    pub use_all_valuation_times: bool,
    /// Optional pricer CPU count.
    pub cpu_count: Option<usize>,
    /// Optional pricing output selections.
    #[serde(default)]
    pub optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    /// Pricing interval schedule policy.
    #[serde(default)]
    pub pricing_interval_schedule_policy: PricingIntervalSchedulePolicy,
    /// Optional Fourier-COS collocation node count override.
    pub spectral_collocation_node_count_override: Option<usize>,
    /// Optional model-P terminal distribution diagnostics for observed option nodes.
    #[serde(default)]
    pub physical_distribution_diagnostics:
        Option<ObservedOptionsPhysicalDistributionDiagnosticsRequest>,
    /// Primary output path.
    pub output_path: Option<PathBuf>,
}

/// Completed observed-options pricing workflow.
#[derive(Debug, Serialize)]
pub struct ObservedOptionsPricingOutput {
    pub schema_version: &'static str,
    pub run_id: String,
    pub scope: ObservedOptionsPricingScope,
    #[serde(serialize_with = "serialize_utc_datetime")]
    pub valuation_time_utc: DateTime<Utc>,
    pub nodes: Vec<ObservedOptionsPricingNode>,
    pub snapshot_results: Vec<ObservedOptionsSnapshotResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physical_distribution: Option<ObservedOptionsPhysicalDistributionDiagnosticsOutput>,
    pub artifact_paths: Option<PricingArtifactPaths>,
    pub execution_duration_seconds: f64,
}

impl ObservedOptionsPricingOutput {
    /// Serialize the completion payload for machine output.
    pub fn completion_payload(&self, model_path: &std::path::Path) -> Result<Value, WorkflowError> {
        let response = serde_json::to_value(self)
            .map_err(|source| WorkflowError::OutputSerialize { source })?;

        Ok(json!({
            "schema_version": "janiform.price_observed_options_run.v1",
            "command_id": "price observed-options",
            "status": "completed",
            "run_id": self.run_id,
            "model_path": model_path,
            "primary_output_path": self.artifact_paths.as_ref().map(|paths| &paths.primary_output_path),
            "report_path": self.artifact_paths.as_ref().map(|paths| &paths.report_path),
            "manifest_path": self.artifact_paths.as_ref().map(|paths| &paths.manifest_path),
            "physical_distribution_nodes_path": self.artifact_paths.as_ref().and_then(|paths| paths.physical_distribution_nodes_path.as_ref()),
            "physical_distribution_report_path": self.artifact_paths.as_ref().and_then(|paths| paths.physical_distribution_report_path.as_ref()),
            "run_directory": self.artifact_paths.as_ref().map(|paths| &paths.run_directory),
            "priced_node_count": self.nodes.iter().filter(|node| node.model_price.is_some()).count(),
            "failed_node_count": self.nodes.iter().filter(|node| node.model_price.is_none()).count(),
            "physical_distribution_node_count": self.physical_distribution.as_ref().map(|diagnostics| diagnostics.nodes.len()),
            "execution_duration_seconds": self.execution_duration_seconds,
            "response": response,
        }))
    }
}

/// Observed snapshot result.
#[derive(Debug, Serialize)]
pub struct ObservedOptionsSnapshotResult {
    #[serde(serialize_with = "serialize_utc_datetime")]
    pub valuation_time_utc: DateTime<Utc>,
    pub spot: f64,
    pub nodes: Vec<ObservedOptionsPricingNode>,
}

/// Priced observed option node.
#[derive(Clone, Debug, Serialize)]
pub struct ObservedOptionsPricingNode {
    #[serde(serialize_with = "serialize_utc_datetime")]
    pub valuation_time_utc: DateTime<Utc>,
    #[serde(serialize_with = "serialize_utc_datetime")]
    pub quote_timestamp_utc: DateTime<Utc>,
    #[serde(serialize_with = "serialize_utc_datetime")]
    pub expiry_timestamp_utc: DateTime<Utc>,
    pub strike: f64,
    pub option_type: VanillaOptionPayoffType,
    pub risk_free_rate: f64,
    pub dividend_yield: f64,
    pub maturity_interval: f64,
    pub observed_bid_price: Option<f64>,
    pub observed_ask_price: Option<f64>,
    pub observed_mid_price: Option<f64>,
    pub model_price: Option<f64>,
    pub model_implied_volatility: Option<f64>,
    pub implied_volatility_inversion_status:
        Option<janiform::pricing::VanillaOptionImpliedVolatilityInversionStatus>,
    pub pricing_status: VanillaOptionPricingStatus,
    pub pricing_failure_stage: Option<VanillaOptionPricingFailureStage>,
    pub pricing_failure_reason: Option<String>,
    pub forward_price: Option<f64>,
    pub discount_factor: Option<f64>,
    pub boundary_state_norm: Option<f64>,
}

#[derive(Clone)]
pub(super) struct PreparedSnapshotGroup {
    pub(super) payoff_type: VanillaOptionPayoffType,
    pub(super) expiry_timestamp_utc_nanos: i64,
    pub(super) nodes: Vec<PreparedSnapshotNode>,
}

#[derive(Clone)]
pub(super) struct PreparedSnapshotNode {
    pub(super) quote_timestamp_utc_nanos: i64,
    pub(super) expiry_timestamp_utc_nanos: i64,
    pub(super) strike: f64,
    pub(super) option_type: VanillaOptionPayoffType,
    pub(super) observed_bid_price: Option<f64>,
    pub(super) observed_ask_price: Option<f64>,
    pub(super) observed_mid_price: Option<f64>,
}

#[derive(Clone)]
pub(super) struct PreparedObservedSnapshot {
    pub(super) valuation_time_utc_nanos: i64,
    pub(super) spot: f64,
    pub(super) estimated_state: CurrentStateEstimationOutput,
    pub(super) snapshot_groups: Vec<PreparedSnapshotGroup>,
}

pub(super) struct ObservedTickStateColumns {
    pub(super) timestamp_utc_nanos: Vec<i64>,
    pub(super) trade_prices: Vec<f64>,
    pub(super) trade_sides: Vec<HawkesEventSide>,
}
