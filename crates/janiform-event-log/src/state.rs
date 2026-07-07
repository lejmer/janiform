use serde::Deserialize;
use serde::Serialize;

use crate::event::{
    ProgressMeasure, RUN_STATE_SNAPSHOT_SCHEMA_VERSION, RunEvent, RunEventPayload,
    RunLifecycleEvent, RunPhaseRef, RunStageRef, WorkflowRef,
};

/// Current run status reconstructed from snapshots or event replay.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Completed,
    Cancelled,
    Failed,
}

/// Current status for one stage in a run.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStageStatus {
    Pending,
    Running,
    Completed,
    Cancelled,
    Failed,
}

/// Latest known state for one run stage.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RunStageState {
    pub stage: RunStageRef,
    pub status: RunStageStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at_utc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at_utc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_phase: Option<RunPhaseRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub latest_progress: Vec<ProgressMeasure>,
}

/// Compact warning summary carried by the latest-state snapshot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WarningSummary {
    pub occurred_at_utc: String,
    pub warning_code: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<RunStageRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<RunPhaseRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_surface_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_paths: Vec<String>,
}

/// Compact artifact summary carried by the latest-state snapshot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArtifactSummary {
    pub occurred_at_utc: String,
    pub artifact_kind: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<RunStageRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<RunPhaseRef>,
}

/// Checkpoint availability reconstructed from checkpoint artifact events.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct CheckpointAvailability {
    pub available_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_path: Option<String>,
}

/// Failure summary reconstructed from run-failed lifecycle events.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RunFailureSummary {
    pub occurred_at_utc: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_stage: Option<RunStageRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_phase: Option<RunPhaseRef>,
}

/// Latest run state used by `inspect run` and future monitoring views.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RunStateSnapshot {
    pub schema_version: String,
    pub run_id: String,
    pub workflow: WorkflowRef,
    pub status: RunStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_stage: Option<RunStageRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_phase: Option<RunPhaseRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub latest_progress: Vec<ProgressMeasure>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stage_states: Vec<RunStageState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<WarningSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ArtifactSummary>,
    pub checkpoints: CheckpointAvailability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<RunFailureSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_event_timestamp_utc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_event_timestamp_utc: Option<String>,
    pub event_count: usize,
}

impl RunStateSnapshot {
    /// Build an empty latest-state snapshot for a run.
    pub fn new(run_id: impl Into<String>, workflow: WorkflowRef) -> Self {
        Self {
            schema_version: RUN_STATE_SNAPSHOT_SCHEMA_VERSION.to_string(),
            run_id: run_id.into(),
            workflow,
            status: RunStatus::Running,
            active_stage: None,
            latest_phase: None,
            latest_progress: Vec::new(),
            stage_states: Vec::new(),
            warnings: Vec::new(),
            artifacts: Vec::new(),
            checkpoints: CheckpointAvailability::default(),
            failure: None,
            first_event_timestamp_utc: None,
            latest_event_timestamp_utc: None,
            event_count: 0,
        }
    }

    /// Apply one event to the snapshot.
    pub fn apply_event(&mut self, event: &RunEvent) {
        if self.first_event_timestamp_utc.is_none() {
            self.first_event_timestamp_utc = Some(event.occurred_at_utc.clone());
        }

        self.latest_event_timestamp_utc = Some(event.occurred_at_utc.clone());

        self.event_count += 1;

        match &event.payload {
            RunEventPayload::Lifecycle(lifecycle) => self.apply_lifecycle(event, lifecycle),
            RunEventPayload::Artifact(artifact) => {
                self.artifacts.push(ArtifactSummary {
                    occurred_at_utc: event.occurred_at_utc.clone(),
                    artifact_kind: artifact.artifact_kind.clone(),
                    path: artifact.path.clone(),
                    stage: artifact.stage.clone(),
                    phase: artifact.phase.clone(),
                });

                if artifact.artifact_kind == "checkpoint" {
                    self.checkpoints.available_count += 1;
                    self.checkpoints.latest_path = Some(artifact.path.clone());
                }
            }
            RunEventPayload::Warning(warning) => self.warnings.push(WarningSummary {
                occurred_at_utc: event.occurred_at_utc.clone(),
                warning_code: warning.warning_code.clone(),
                reason: warning.reason.clone(),
                stage: warning.stage.clone(),
                phase: warning.phase.clone(),
                input_surface_id: warning
                    .input_surface
                    .as_ref()
                    .map(|surface| surface.surface_id.clone()),
                field_path: warning.field.as_ref().map(|field| field.field_path.clone()),
                artifact_paths: warning.artifact_paths.clone(),
            }),
            RunEventPayload::Error(error) => self.warnings.push(WarningSummary {
                occurred_at_utc: event.occurred_at_utc.clone(),
                warning_code: error.error_code.clone(),
                reason: error.reason.clone(),
                stage: error.stage.clone(),
                phase: error.phase.clone(),
                input_surface_id: error
                    .input_surface
                    .as_ref()
                    .map(|surface| surface.surface_id.clone()),
                field_path: error.field.as_ref().map(|field| field.field_path.clone()),
                artifact_paths: error.artifact_paths.clone(),
            }),
            RunEventPayload::Progress(progress) => {
                self.active_stage = progress.stage.clone().or_else(|| self.active_stage.clone());
                self.latest_phase = progress
                    .subphase
                    .clone()
                    .or_else(|| progress.phase.clone())
                    .or_else(|| self.latest_phase.clone());
                self.latest_progress = progress.measures.clone();

                if let Some(stage) = self.active_stage.clone() {
                    let latest_phase = self.latest_phase.clone();
                    let latest_progress = self.latest_progress.clone();
                    let stage_state = self.stage_state_mut(&stage);
                    stage_state.latest_phase = latest_phase;
                    stage_state.latest_progress = latest_progress;
                }
            }
            RunEventPayload::MetricUpdate(update) => {
                self.active_stage = update.stage.clone().or_else(|| self.active_stage.clone());
                self.latest_phase = update.phase.clone().or_else(|| self.latest_phase.clone());
                self.latest_progress = update.measures.clone();

                if let Some(stage) = self.active_stage.clone() {
                    let latest_phase = self.latest_phase.clone();
                    let latest_progress = self.latest_progress.clone();
                    let stage_state = self.stage_state_mut(&stage);
                    stage_state.latest_phase = latest_phase;
                    stage_state.latest_progress = latest_progress;
                }
            }
        }
    }

    fn apply_lifecycle(&mut self, event: &RunEvent, lifecycle: &RunLifecycleEvent) {
        match lifecycle {
            RunLifecycleEvent::RunStarted { stage_sequence, .. } => {
                self.status = RunStatus::Running;
                self.stage_states = stage_sequence
                    .iter()
                    .cloned()
                    .map(|stage| RunStageState {
                        stage,
                        status: RunStageStatus::Pending,
                        started_at_utc: None,
                        completed_at_utc: None,
                        latest_phase: None,
                        latest_progress: Vec::new(),
                    })
                    .collect();
            }
            RunLifecycleEvent::StageStarted { stage, .. } => {
                self.status = RunStatus::Running;
                self.active_stage = Some(stage.clone());
                self.latest_phase = None;
                self.latest_progress.clear();

                let stage_state = self.stage_state_mut(stage);

                stage_state.status = RunStageStatus::Running;
                stage_state.started_at_utc = Some(event.occurred_at_utc.clone());
            }
            RunLifecycleEvent::StageCompleted { stage, .. } => {
                let stage_state = self.stage_state_mut(stage);

                stage_state.status = RunStageStatus::Completed;
                stage_state.completed_at_utc = Some(event.occurred_at_utc.clone());

                if self
                    .active_stage
                    .as_ref()
                    .is_some_and(|active_stage| active_stage.stage_id == stage.stage_id)
                {
                    self.active_stage = None;
                }
            }
            RunLifecycleEvent::RunCompleted { .. } => {
                self.status = RunStatus::Completed;
                self.active_stage = None;
            }
            RunLifecycleEvent::RunFailed {
                failed_stage,
                failed_phase,
                reason,
                ..
            } => {
                self.status = RunStatus::Failed;
                self.active_stage = failed_stage.clone();
                self.latest_phase = failed_phase.clone().or_else(|| self.latest_phase.clone());

                self.failure = Some(RunFailureSummary {
                    occurred_at_utc: event.occurred_at_utc.clone(),
                    reason: reason.clone(),
                    failed_stage: failed_stage.clone(),
                    failed_phase: failed_phase.clone(),
                });

                if let Some(stage) = failed_stage {
                    let stage_state = self.stage_state_mut(stage);

                    stage_state.status = RunStageStatus::Failed;
                    stage_state.latest_phase = failed_phase.clone();
                }
            }
            RunLifecycleEvent::RunCancelled {
                cancelled_before_stage,
                ..
            } => {
                self.status = RunStatus::Cancelled;
                self.active_stage = cancelled_before_stage.clone();
                self.latest_progress.clear();

                if let Some(stage) = cancelled_before_stage {
                    let stage_state = self.stage_state_mut(stage);
                    stage_state.status = RunStageStatus::Cancelled;
                }
            }
        }
    }

    fn stage_state_mut(&mut self, stage: &RunStageRef) -> &mut RunStageState {
        let existing_position = self
            .stage_states
            .iter()
            .position(|state| state.stage.stage_id == stage.stage_id);

        let position = match existing_position {
            Some(position) => position,
            None => {
                self.stage_states.push(RunStageState {
                    stage: stage.clone(),
                    status: RunStageStatus::Pending,
                    started_at_utc: None,
                    completed_at_utc: None,
                    latest_phase: None,
                    latest_progress: Vec::new(),
                });

                self.stage_states.len() - 1
            }
        };

        &mut self.stage_states[position]
    }
}
