use janiform_calibration_artifacts::CalibrationStage;
use janiform_calibration_runtime::{
    CalibrationMarketDataWorkflowScope, MarketDataInputFileFormat, MarketDataTickPriceInputPolicy,
};

/// Render a market-data file format for workflow summaries.
pub const fn market_data_file_format_label(format: MarketDataInputFileFormat) -> &'static str {
    match format {
        MarketDataInputFileFormat::Csv => "csv",
        MarketDataInputFileFormat::Parquet => "parquet",
        MarketDataInputFileFormat::Json => "json",
    }
}

/// Render a tick-price input policy for workflow summaries.
pub const fn tick_price_policy_label(policy: MarketDataTickPriceInputPolicy) -> &'static str {
    match policy {
        MarketDataTickPriceInputPolicy::ExplicitPriceColumn => "explicit price column",
        MarketDataTickPriceInputPolicy::BidAskMidpoint => "bid ask midpoint",
    }
}

/// Render a workflow scope for human summaries.
pub const fn workflow_scope_label(scope: CalibrationMarketDataWorkflowScope) -> &'static str {
    match scope {
        CalibrationMarketDataWorkflowScope::SeasonalityOnly => "seasonality only",
        CalibrationMarketDataWorkflowScope::FullWithoutOptionSurface => {
            "full calibration without option surface"
        }
        CalibrationMarketDataWorkflowScope::FullWithOptionSurface => {
            "full calibration with option snapshot"
        }
    }
}

/// Render a staged calibration stage for progress and event payloads.
pub const fn stage_label(stage: CalibrationStage) -> &'static str {
    match stage {
        CalibrationStage::Seasonality => "Fit seasonality",
        CalibrationStage::Hawkes => "Fit event activity",
        CalibrationStage::ReturnComponents => "Fit return distribution",
        CalibrationStage::MeanReversion => "Fit mean reversion",
        CalibrationStage::JointRefinement => "Refine dynamics",
    }
}
