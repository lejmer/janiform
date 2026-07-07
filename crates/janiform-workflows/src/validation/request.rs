//! Validation workflow request contracts.

use std::path::PathBuf;

use janiform::calibration::market_data::{
    MarketDataDuplicateRecordPolicy, MarketDataInputFileFormat, MarketDataTickPriceInputPolicy,
};

use super::output::StylizedFactsValidationCacheLevel;

/// Request for the stylized-facts validation workflow.
#[derive(Debug, Clone)]
pub struct StylizedFactsValidationWorkflowRequest {
    /// Persisted model artifact path for raw-data validation.
    pub model_path: Option<PathBuf>,
    /// Empirical tick data path for raw-data validation.
    pub empirical_data_path: Option<PathBuf>,
    /// Explicit empirical input format.
    pub empirical_data_format: Option<MarketDataInputFileFormat>,
    /// User-facing return interval labels.
    pub intervals: Vec<String>,
    /// Number of seeded simulation paths.
    pub simulation_path_count: usize,
    /// Base random seed.
    pub seed: u64,
    /// CPU count available for parallel validation work.
    pub cpu_count: Option<usize>,
    /// Resolved output directory for validation artifacts.
    pub output_dir: PathBuf,
    /// Optional explicit threshold policy artifact.
    pub threshold_policy: Option<PathBuf>,
    /// Optional statistical significance level.
    pub significance_level: Option<f64>,
    /// Requested cache level.
    pub cache: Option<StylizedFactsValidationCacheLevel>,
    /// Optional precomputed comparison request artifact.
    pub comparison_request: Option<PathBuf>,
    /// Optional metric catalog artifact.
    pub metric_catalog: Option<PathBuf>,
    /// Empirical timestamp column override.
    pub empirical_timestamp_column: Option<String>,
    /// Empirical price column override.
    pub empirical_price_column: Option<String>,
    /// Empirical trade sign column override.
    pub empirical_trade_sign_column: Option<String>,
    /// Empirical bid column override.
    pub empirical_bid_column: Option<String>,
    /// Empirical ask column override.
    pub empirical_ask_column: Option<String>,
    /// Empirical tick price normalization policy.
    pub empirical_price_input_policy: Option<MarketDataTickPriceInputPolicy>,
    /// Timezone for naive empirical timestamps.
    pub empirical_timestamp_timezone: Option<String>,
    /// Duplicate empirical record policy.
    pub empirical_duplicate_record_policy: Option<MarketDataDuplicateRecordPolicy>,
    /// Keep empirical source order instead of sorting by timestamp.
    pub keep_empirical_source_order: bool,
}
