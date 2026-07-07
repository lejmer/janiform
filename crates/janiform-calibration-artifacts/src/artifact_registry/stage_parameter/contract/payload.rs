use serde::{Deserialize, Serialize};

use super::super::super::super::contracts::CalibrationStage;
use super::hawkes::HawkesStageParameterPayload;
use super::joint_refinement::JointRefinementStageParameterPayload;
use super::long_variance::LongVarianceStageParameterPayload;
use super::mean_reversion::MeanReversionStageParameterPayload;
use super::news_shocks::NewsShocksStageParameterPayload;
use super::return_backfit::ReturnBackfitStageParameterPayload;
use super::return_components::ReturnComponentsStageParameterPayload;
use super::seasonality::SeasonalityStageParameterPayload;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "payload_kind", content = "payload", rename_all = "snake_case")]
pub enum CalibrationStageParameterPayload {
    Seasonality(Box<SeasonalityStageParameterPayload>),
    Hawkes(Box<HawkesStageParameterPayload>),
    ReturnComponents(Box<ReturnComponentsStageParameterPayload>),
    LongVariance(Box<LongVarianceStageParameterPayload>),
    NewsShocks(Box<NewsShocksStageParameterPayload>),
    ReturnBackfit(Box<ReturnBackfitStageParameterPayload>),
    MeanReversion(Box<MeanReversionStageParameterPayload>),
    JointRefinement(Box<JointRefinementStageParameterPayload>),
    OptionRefinement(Box<JointRefinementStageParameterPayload>),
    Finalization(Box<JointRefinementStageParameterPayload>),
}

impl CalibrationStageParameterPayload {
    pub(super) const fn stage(&self) -> CalibrationStage {
        match self {
            Self::Seasonality(_) => CalibrationStage::Seasonality,
            Self::Hawkes(_) => CalibrationStage::Hawkes,
            Self::ReturnComponents(_) => CalibrationStage::ReturnComponents,
            Self::LongVariance(_) => CalibrationStage::LongVariance,
            Self::NewsShocks(_) => CalibrationStage::NewsShocks,
            Self::ReturnBackfit(_) => CalibrationStage::ReturnBackfit,
            Self::MeanReversion(_) => CalibrationStage::MeanReversion,
            Self::JointRefinement(_) => CalibrationStage::JointRefinement,
            Self::OptionRefinement(_) => CalibrationStage::OptionRefinement,
            Self::Finalization(_) => CalibrationStage::Finalization,
        }
    }
}
