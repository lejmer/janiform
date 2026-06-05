use serde::Serialize;

use crate::stages::progress::StageProgressUpdate;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LongVarianceStageProgressPhase {
    InputPreflight,
    IntervalSourcePreparation,
    ParameterEstimation,
    HorizonDiagnostics,
    ReportAssembly,
}

pub type LongVarianceStageProgressUpdate = StageProgressUpdate<LongVarianceStageProgressPhase>;
