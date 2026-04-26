use janiform::calibration::{
    CalibrationMarketDataWorkflowScope, CalibrationStage, MarketDataInputFileFormat,
    MarketDataTickPriceInputPolicy,
};

use crate::commands::options::WorkflowScope;

pub(crate) fn optional_label(value: Option<&str>) -> String {
    value.unwrap_or("none").to_string()
}

pub(crate) fn market_data_file_format_label(format: MarketDataInputFileFormat) -> &'static str {
    match format {
        MarketDataInputFileFormat::Csv => "csv",
        MarketDataInputFileFormat::Parquet => "parquet",
        MarketDataInputFileFormat::Json => "json",
    }
}

pub(crate) fn tick_price_policy_label(policy: MarketDataTickPriceInputPolicy) -> &'static str {
    match policy {
        MarketDataTickPriceInputPolicy::ExplicitPriceColumn => "explicit price column",
        MarketDataTickPriceInputPolicy::BidAskMidpoint => "bid ask midpoint",
    }
}

pub(crate) fn workflow_scope_argument_label(scope: WorkflowScope) -> String {
    match scope {
        WorkflowScope::Full => "full",
        WorkflowScope::SeasonalityOnly => "seasonality only",
        WorkflowScope::FullWithoutOptionSurface => "full without option surface",
        WorkflowScope::FullWithOptionSurface => "full with option surface",
    }
    .to_string()
}

pub(crate) fn workflow_scope_label(scope: CalibrationMarketDataWorkflowScope) -> &'static str {
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

pub(crate) fn stage_label(stage: CalibrationStage) -> &'static str {
    match stage {
        CalibrationStage::Seasonality => "Prepare market data and fit seasonality",
        CalibrationStage::Hawkes => "Fit event activity",
        CalibrationStage::ReturnComponents => "Fit return distribution",
        CalibrationStage::MeanReversion => "Fit mean reversion",
        CalibrationStage::JointRefinement => "Refine dynamics",
    }
}
