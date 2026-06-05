use serde::Serialize;

use crate::stages::progress::StageProgressUpdate;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeanReversionStageProgressPhase {
    InputPreflight,
    GridEvaluation,
    PredictiveEvidence,
    CandidateSelection,
    ReportAssembly,
}

pub type MeanReversionStageProgressUpdate = StageProgressUpdate<MeanReversionStageProgressPhase>;
