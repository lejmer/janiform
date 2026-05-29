use janiform::calibration::artifacts::{CalibrationExecutionMode, CalibrationStage};
use janiform::calibration::market_data::{
    MarketDataDuplicateRecordPolicy, MarketDataInputFileFormat, MarketDataOptionQuoteInputPolicy,
    MarketDataTickPriceInputPolicy, PriceCoordinatePolicy,
};

use crate::commands::options;
use crate::commands::options::CalibrationStageOption;
use crate::commands::options::InputFileFormat;

pub(crate) fn calibration_execution_mode(mode: options::ExecutionMode) -> CalibrationExecutionMode {
    match mode {
        options::ExecutionMode::Memory => CalibrationExecutionMode::Memory,
        options::ExecutionMode::Disk => CalibrationExecutionMode::Disk,
    }
}

pub(crate) fn calibration_stage(stage: CalibrationStageOption) -> CalibrationStage {
    match stage {
        CalibrationStageOption::Seasonality => CalibrationStage::Seasonality,
        CalibrationStageOption::Hawkes => CalibrationStage::Hawkes,
        CalibrationStageOption::ReturnComponents => CalibrationStage::ReturnComponents,
        CalibrationStageOption::LongVariance => CalibrationStage::LongVariance,
        CalibrationStageOption::NewsShocks => CalibrationStage::NewsShocks,
        CalibrationStageOption::MeanReversion => CalibrationStage::MeanReversion,
        CalibrationStageOption::JointRefinement => CalibrationStage::JointRefinement,
        CalibrationStageOption::OptionRefinement => CalibrationStage::OptionRefinement,
        CalibrationStageOption::Finalization => CalibrationStage::Finalization,
    }
}

pub(crate) fn market_data_file_format(format: InputFileFormat) -> MarketDataInputFileFormat {
    match format {
        InputFileFormat::Csv => MarketDataInputFileFormat::Csv,
        InputFileFormat::Parquet => MarketDataInputFileFormat::Parquet,
        InputFileFormat::Json => MarketDataInputFileFormat::Json,
    }
}

pub(crate) fn tick_price_input_policy(
    policy: options::TickPriceInputPolicy,
) -> MarketDataTickPriceInputPolicy {
    match policy {
        options::TickPriceInputPolicy::ExplicitPriceColumn => {
            MarketDataTickPriceInputPolicy::ExplicitPriceColumn
        }
        options::TickPriceInputPolicy::BidAskMidpoint => {
            MarketDataTickPriceInputPolicy::BidAskMidpoint
        }
    }
}

pub(crate) fn option_quote_input_policy(
    policy: options::OptionQuoteInputPolicy,
) -> MarketDataOptionQuoteInputPolicy {
    match policy {
        options::OptionQuoteInputPolicy::MidPriceColumn => {
            MarketDataOptionQuoteInputPolicy::MidPriceColumn
        }
        options::OptionQuoteInputPolicy::BidAskMidpoint => {
            MarketDataOptionQuoteInputPolicy::BidAskMidpoint
        }
        options::OptionQuoteInputPolicy::ImpliedVolatilityColumn => {
            MarketDataOptionQuoteInputPolicy::ImpliedVolatilityColumn
        }
    }
}

pub(crate) fn price_coordinate_policy(
    policy: options::PriceCoordinatePolicyOption,
) -> PriceCoordinatePolicy {
    match policy {
        options::PriceCoordinatePolicyOption::Auto => PriceCoordinatePolicy::Auto,
        options::PriceCoordinatePolicyOption::Single => PriceCoordinatePolicy::Single,
        options::PriceCoordinatePolicyOption::ContinuousFitRawOptionAnchor => {
            PriceCoordinatePolicy::ContinuousFitRawOptionAnchor
        }
    }
}

pub(crate) fn duplicate_record_policy(
    policy: options::DuplicateRecordPolicy,
) -> MarketDataDuplicateRecordPolicy {
    match policy {
        options::DuplicateRecordPolicy::KeepFirst => MarketDataDuplicateRecordPolicy::KeepFirst,
        options::DuplicateRecordPolicy::KeepLast => MarketDataDuplicateRecordPolicy::KeepLast,
    }
}
