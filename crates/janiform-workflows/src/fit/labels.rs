use janiform_calibration_artifacts::CalibrationStage;
use janiform_calibration_runtime::{
    CalibrationMarketDataWorkflowScope, CalibrationStageExecutionStatus, MarketDataInputFileFormat,
    MarketDataTickPriceInputPolicy,
};
use janiform_terminal_ui::format_token_as_sentence_label;

/// Stable fit workflow identifier used by event logs.
pub(super) const fn fit_workflow_id() -> &'static str {
    "fit"
}

/// Human-readable fit workflow label used by event logs.
pub(super) const fn fit_workflow_label() -> &'static str {
    "Fit"
}

/// Human-readable progress scope label for fit workflows.
pub(super) const fn fit_progress_scope_label() -> &'static str {
    "Calibrating"
}

/// Human-readable progress scope label for fit preparation.
pub(super) const fn fit_preparation_progress_scope_label() -> &'static str {
    "Preparing fit"
}

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

/// Render a workflow scope for terminal summaries.
pub const fn workflow_scope_label(scope: CalibrationMarketDataWorkflowScope) -> &'static str {
    match scope {
        CalibrationMarketDataWorkflowScope::SeasonalityOnly => "Seasonality only",
        CalibrationMarketDataWorkflowScope::FullWithoutOptionSurface => {
            "Full calibration without option surface"
        }
        CalibrationMarketDataWorkflowScope::FullWithOptionSurface => {
            "Full calibration with option snapshot"
        }
    }
}

/// Render a persisted workflow-scope token for terminal summaries.
pub fn workflow_scope_label_from_token(scope: &str) -> String {
    match scope {
        "seasonality_only" => "Seasonality only".to_string(),
        "full_without_option_surface" => "Full calibration without option surface".to_string(),
        "full_with_option_surface" => "Full calibration with option snapshot".to_string(),
        "underlying_only" => "Underlying calibration".to_string(),
        "joint_option_calibrated" => "Joint option calibration".to_string(),
        other => format_token_as_sentence_label(other),
    }
}

/// Render a staged calibration stage for progress and event payloads.
pub const fn stage_label(stage: CalibrationStage) -> &'static str {
    match stage {
        CalibrationStage::Seasonality => "Fit seasonality",
        CalibrationStage::Hawkes => "Fit event activity",
        CalibrationStage::ReturnComponents => "Fit return distribution",
        CalibrationStage::LongVariance => "Fit long variance",
        CalibrationStage::NewsShocks => "Fit news shocks",
        CalibrationStage::MeanReversion => "Fit mean reversion",
        CalibrationStage::JointRefinement => "Refine dynamics",
        CalibrationStage::OptionRefinement => "Fit option surface",
        CalibrationStage::Finalization => "Finalize model",
    }
}

/// Render a staged calibration execution status for terminal and event payloads.
pub const fn stage_execution_status_label(status: CalibrationStageExecutionStatus) -> &'static str {
    match status {
        CalibrationStageExecutionStatus::Success => "completed",
        CalibrationStageExecutionStatus::SuccessWithWarnings => "completed with warnings",
        CalibrationStageExecutionStatus::Failure => "failed",
    }
}

/// Render a persisted stage token for terminal summaries.
pub fn stage_label_from_token(stage: &str) -> String {
    let label = match stage {
        "seasonality" => "Fit seasonality",
        "hawkes" => "Fit event activity",
        "return_components" => "Fit return distribution",
        "long_variance" => "Fit long variance",
        "news_shocks" => "Fit news shocks",
        "mean_reversion" => "Fit mean reversion",
        "joint_refinement" => "Refine dynamics",
        "option_refinement" => "Fit option surface",
        "finalization" => "Finalize model",
        other => return format_token_as_sentence_label(other),
    };

    label.to_string()
}
