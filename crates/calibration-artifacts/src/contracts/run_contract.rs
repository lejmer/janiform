use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use super::{
    CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION, CALIBRATION_RUN_SPEC_SCHEMA_VERSION,
    CALIBRATION_SOLVER_SETTINGS_CAPTURE_POLICY, CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION,
    CalibrationRunSpec, CalibrationRunSpecError, CalibrationStage, CalibrationStageGraph,
    CalibrationStageGraphError, CalibrationStageSolverSettings,
};

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationRunContractError {
    InvalidRunSpec { source: CalibrationRunSpecError },
    InvalidStageGraph { source: CalibrationStageGraphError },
}

impl Display for CalibrationRunContractError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRunSpec { source } => {
                write!(formatter, "run_spec is invalid: {source}")
            }
            Self::InvalidStageGraph { source } => {
                write!(formatter, "stage_graph is invalid: {source}")
            }
        }
    }
}

impl Error for CalibrationRunContractError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRunSpec { source } => Some(source),
            Self::InvalidStageGraph { source } => Some(source),
        }
    }
}

pub fn resolve_calibration_stage_execution_order(
    run_spec: &CalibrationRunSpec,
    stage_graph: &CalibrationStageGraph,
) -> Result<Vec<CalibrationStage>, CalibrationRunContractError> {
    run_spec
        .validate()
        .map_err(|source| CalibrationRunContractError::InvalidRunSpec { source })?;

    stage_graph
        .resolve_execution_order(&run_spec.selected_stages)
        .map_err(|source| CalibrationRunContractError::InvalidStageGraph { source })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationRunContract {
    pub schema_version: String,
    pub run_spec_schema_version: String,
    pub stage_graph_schema_version: String,
    pub solver_settings_capture_policy: String,
    pub run_spec: CalibrationRunSpec,
    pub stage_graph: CalibrationStageGraph,
    pub execution_stage_order: Vec<CalibrationStage>,
    pub stage_solver_settings: Vec<CalibrationStageSolverSettings>,
}

impl CalibrationRunContract {
    pub fn try_new(
        run_spec: CalibrationRunSpec,
        stage_graph: CalibrationStageGraph,
    ) -> Result<Self, CalibrationRunContractError> {
        let execution_stage_order =
            resolve_calibration_stage_execution_order(&run_spec, &stage_graph)?;

        Ok(Self {
            schema_version: CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION.to_string(),
            run_spec_schema_version: CALIBRATION_RUN_SPEC_SCHEMA_VERSION.to_string(),
            stage_graph_schema_version: CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION.to_string(),
            solver_settings_capture_policy: CALIBRATION_SOLVER_SETTINGS_CAPTURE_POLICY.to_string(),
            stage_solver_settings: run_spec.optimizer_settings_by_stage.clone(),
            run_spec,
            stage_graph,
            execution_stage_order,
        })
    }
}
