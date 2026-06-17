use super::super::super::super::contracts::{
    CalibrationOptimizerSettings, CalibrationStageSeedSource,
};

#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationStageParameterBuildMetadata {
    pub stage_output_schema_version: String,
    pub run_global_seed: u64,
    pub stage_seed: u64,
    pub stage_seed_source: CalibrationStageSeedSource,
    pub stage_seed_derivation_policy: String,
    pub solver_settings: CalibrationOptimizerSettings,
}
