use std::collections::BTreeMap;

use janiform_calibration_artifacts::{CalibrationStage, CalibrationStageCheckpointMetadata};

use crate::stages::hawkes::HawkesStageFittedPayload;
use crate::stages::joint_refinement::{
    JointRefinementStageFittedPayload, JointRefinementStageRuntimeHandoff,
    JointRefinementStageSeasonalityFactorsInput,
};
use crate::stages::long_variance::LongVarianceStageFittedPayload;
use crate::stages::mean_reversion::{
    MeanReversionStageFittedPayload, MeanReversionStageSeasonalityInput,
};
use crate::stages::news_shocks::NewsShocksStageFittedPayload;
use crate::stages::return_components::ReturnComponentsStageFittedPayload;
use crate::stages::seasonality::SeasonalityStageNormalizedPayload;

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct CalibrationInMemoryStagePayloadState {
    pub(crate) seasonality_stage_payload: Option<SeasonalityStageNormalizedPayload>,
    pub(crate) mean_reversion_seasonality_input: Option<MeanReversionStageSeasonalityInput>,
    pub(crate) joint_refinement_seasonality_factors_input:
        Option<JointRefinementStageSeasonalityFactorsInput>,
    pub(crate) hawkes_stage_payload: Option<HawkesStageFittedPayload>,
    pub(crate) return_components_stage_payload: Option<ReturnComponentsStageFittedPayload>,
    pub(crate) long_variance_stage_payload: Option<LongVarianceStageFittedPayload>,
    pub(crate) news_shocks_stage_payload: Option<NewsShocksStageFittedPayload>,
    pub(crate) mean_reversion_stage_payload: Option<MeanReversionStageFittedPayload>,
    pub(crate) joint_refinement_stage_payload: Option<JointRefinementStageFittedPayload>,
    pub(crate) option_refinement_stage_payload: Option<JointRefinementStageFittedPayload>,
    pub(crate) finalization_stage_payload: Option<JointRefinementStageFittedPayload>,
    pub(crate) joint_refinement_runtime_handoff: Option<JointRefinementStageRuntimeHandoff>,
    pub(crate) option_refinement_runtime_handoff: Option<JointRefinementStageRuntimeHandoff>,
}

#[derive(Debug, Clone, Default)]
struct CalibrationInMemoryCheckpointRunState {
    checkpoint_by_stage: BTreeMap<CalibrationStage, CalibrationStageCheckpointMetadata>,
    last_successful_stage: Option<CalibrationStage>,
    payload_state: CalibrationInMemoryStagePayloadState,
}

#[derive(Debug, Clone, Default)]
pub struct CalibrationMemoryCheckpointStore {
    run_state_by_id: BTreeMap<String, CalibrationInMemoryCheckpointRunState>,
}

#[derive(Debug, Clone)]
pub(crate) struct LoadedInMemoryCheckpointState {
    pub(crate) checkpoint_metadata: CalibrationStageCheckpointMetadata,
    pub(crate) payload_state: CalibrationInMemoryStagePayloadState,
}

impl CalibrationMemoryCheckpointStore {
    pub const fn new() -> Self {
        Self {
            run_state_by_id: BTreeMap::new(),
        }
    }

    pub fn checkpoint_count_for_run(&self, run_id: &str) -> usize {
        self.run_state_by_id
            .get(run_id)
            .map(|state| state.checkpoint_by_stage.len())
            .unwrap_or(0)
    }

    pub fn checkpoint_for_stage(
        &self,
        run_id: &str,
        stage: CalibrationStage,
    ) -> Option<&CalibrationStageCheckpointMetadata> {
        self.run_state_by_id
            .get(run_id)
            .and_then(|state| state.checkpoint_by_stage.get(&stage))
    }

    pub fn last_checkpoint_for_run(
        &self,
        run_id: &str,
    ) -> Option<&CalibrationStageCheckpointMetadata> {
        let run_state = self.run_state_by_id.get(run_id)?;
        let last_stage = run_state.last_successful_stage?;

        run_state.checkpoint_by_stage.get(&last_stage)
    }

    pub(crate) fn load_resume_state(&self, run_id: &str) -> Option<LoadedInMemoryCheckpointState> {
        let run_state = self.run_state_by_id.get(run_id)?;
        let last_stage = run_state.last_successful_stage?;
        let checkpoint_metadata = run_state.checkpoint_by_stage.get(&last_stage)?.clone();

        Some(LoadedInMemoryCheckpointState {
            checkpoint_metadata,
            payload_state: run_state.payload_state.clone(),
        })
    }

    pub(crate) fn record_stage_checkpoint(
        &mut self,
        checkpoint_metadata: CalibrationStageCheckpointMetadata,
        payload_state: &CalibrationInMemoryStagePayloadState,
    ) {
        let run_state = self
            .run_state_by_id
            .entry(checkpoint_metadata.run_id.clone())
            .or_default();

        run_state.checkpoint_by_stage.insert(
            checkpoint_metadata.completed_stage,
            checkpoint_metadata.clone(),
        );
        run_state.last_successful_stage = Some(checkpoint_metadata.completed_stage);
        run_state.payload_state = payload_state.clone();
    }
}
