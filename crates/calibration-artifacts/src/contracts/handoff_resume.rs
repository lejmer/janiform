use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use super::{CalibrationExecutionMode, CalibrationStage};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationStageHandoffTransport {
    InMemoryOutput,
    PersistedArtifactReference,
}

/// Typed stage-to-stage handoff payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub enum CalibrationStageHandoffRoute {
    InMemoryOutput,
    PersistedArtifactReference { artifact_reference: String },
}

impl CalibrationStageHandoffRoute {
    pub const fn transport(&self) -> CalibrationStageHandoffTransport {
        match self {
            Self::InMemoryOutput => CalibrationStageHandoffTransport::InMemoryOutput,
            Self::PersistedArtifactReference { .. } => {
                CalibrationStageHandoffTransport::PersistedArtifactReference
            }
        }
    }

    pub fn artifact_reference(&self) -> Option<&str> {
        match self {
            Self::InMemoryOutput => None,
            Self::PersistedArtifactReference { artifact_reference } => Some(artifact_reference),
        }
    }
}

/// One directed stage handoff contract entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalibrationStageHandoff {
    pub from_stage: CalibrationStage,
    pub to_stage: CalibrationStage,
    #[serde(flatten)]
    pub route: CalibrationStageHandoffRoute,
}

impl CalibrationStageHandoff {
    pub fn try_new(
        from_stage: CalibrationStage,
        to_stage: CalibrationStage,
        route: CalibrationStageHandoffRoute,
    ) -> Result<Self, CalibrationStageHandoffError> {
        if from_stage == to_stage {
            return Err(CalibrationStageHandoffError::SelfHandoff { stage: from_stage });
        }

        if let CalibrationStageHandoffRoute::PersistedArtifactReference { artifact_reference } =
            &route
            && artifact_reference.trim().is_empty()
        {
            return Err(
                CalibrationStageHandoffError::EmptyArtifactReferenceForPersistedHandoff {
                    from_stage,
                    to_stage,
                },
            );
        }

        Ok(Self {
            from_stage,
            to_stage,
            route,
        })
    }

    pub fn in_memory_output(from_stage: CalibrationStage, to_stage: CalibrationStage) -> Self {
        Self {
            from_stage,
            to_stage,
            route: CalibrationStageHandoffRoute::InMemoryOutput,
        }
    }

    pub fn persisted_artifact_reference(
        from_stage: CalibrationStage,
        to_stage: CalibrationStage,
        artifact_reference: impl Into<String>,
    ) -> Result<Self, CalibrationStageHandoffError> {
        Self::try_new(
            from_stage,
            to_stage,
            CalibrationStageHandoffRoute::PersistedArtifactReference {
                artifact_reference: artifact_reference.into(),
            },
        )
    }

    pub const fn transport(&self) -> CalibrationStageHandoffTransport {
        self.route.transport()
    }

    pub fn artifact_reference(&self) -> Option<&str> {
        self.route.artifact_reference()
    }

    pub(super) fn validate(
        &self,
        execution_mode: CalibrationExecutionMode,
        selected_stages: &BTreeSet<CalibrationStage>,
    ) -> Result<(), CalibrationStageHandoffError> {
        if self.from_stage == self.to_stage {
            return Err(CalibrationStageHandoffError::SelfHandoff {
                stage: self.from_stage,
            });
        }

        if !selected_stages.contains(&self.from_stage) {
            return Err(CalibrationStageHandoffError::FromStageIsNotSelected {
                stage: self.from_stage,
            });
        }

        if !selected_stages.contains(&self.to_stage) {
            return Err(CalibrationStageHandoffError::ToStageIsNotSelected {
                stage: self.to_stage,
            });
        }

        match &self.route {
            CalibrationStageHandoffRoute::InMemoryOutput => {}
            CalibrationStageHandoffRoute::PersistedArtifactReference { artifact_reference } => {
                if execution_mode != CalibrationExecutionMode::Disk {
                    return Err(
                        CalibrationStageHandoffError::PersistedArtifactHandoffRequiresDiskMode {
                            from_stage: self.from_stage,
                            to_stage: self.to_stage,
                        },
                    );
                }

                if artifact_reference.trim().is_empty() {
                    return Err(
                        CalibrationStageHandoffError::EmptyArtifactReferenceForPersistedHandoff {
                            from_stage: self.from_stage,
                            to_stage: self.to_stage,
                        },
                    );
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationStageHandoffError {
    SelfHandoff {
        stage: CalibrationStage,
    },
    FromStageIsNotSelected {
        stage: CalibrationStage,
    },
    ToStageIsNotSelected {
        stage: CalibrationStage,
    },
    PersistedArtifactHandoffRequiresDiskMode {
        from_stage: CalibrationStage,
        to_stage: CalibrationStage,
    },
    EmptyArtifactReferenceForPersistedHandoff {
        from_stage: CalibrationStage,
        to_stage: CalibrationStage,
    },
}

impl Display for CalibrationStageHandoffError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SelfHandoff { stage } => write!(
                formatter,
                "stage_handoffs contains invalid self handoff for stage `{stage:?}`"
            ),
            Self::FromStageIsNotSelected { stage } => write!(
                formatter,
                "stage_handoffs.from_stage `{stage:?}` must be selected in selected_stages"
            ),
            Self::ToStageIsNotSelected { stage } => write!(
                formatter,
                "stage_handoffs.to_stage `{stage:?}` must be selected in selected_stages"
            ),
            Self::PersistedArtifactHandoffRequiresDiskMode {
                from_stage,
                to_stage,
            } => write!(
                formatter,
                "stage_handoffs entry `{from_stage:?}` -> `{to_stage:?}` uses persisted_artifact_reference and requires execution_mode=disk"
            ),
            Self::EmptyArtifactReferenceForPersistedHandoff {
                from_stage,
                to_stage,
            } => write!(
                formatter,
                "stage_handoffs entry `{from_stage:?}` -> `{to_stage:?}` has empty artifact_reference"
            ),
        }
    }
}

impl Error for CalibrationStageHandoffError {}

/// Resume source contract for calibration orchestration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum CalibrationResumeSource {
    SameProcess,
    PersistedCheckpointReference { checkpoint_reference: String },
}

/// Resume configuration for staged calibration orchestration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalibrationResumeSettings {
    #[serde(flatten)]
    pub source: CalibrationResumeSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_from_stage: Option<CalibrationStage>,
}

impl CalibrationResumeSettings {
    pub fn same_process(resume_from_stage: Option<CalibrationStage>) -> Self {
        Self {
            source: CalibrationResumeSource::SameProcess,
            resume_from_stage,
        }
    }

    pub fn persisted_checkpoint_reference(
        checkpoint_reference: impl Into<String>,
        resume_from_stage: Option<CalibrationStage>,
    ) -> Self {
        Self {
            source: CalibrationResumeSource::PersistedCheckpointReference {
                checkpoint_reference: checkpoint_reference.into(),
            },
            resume_from_stage,
        }
    }

    pub(super) fn validate(
        &self,
        execution_mode: CalibrationExecutionMode,
        selected_stages: &BTreeSet<CalibrationStage>,
    ) -> Result<(), CalibrationResumeSettingsError> {
        if let Some(resume_from_stage) = self.resume_from_stage
            && !selected_stages.contains(&resume_from_stage)
        {
            return Err(CalibrationResumeSettingsError::ResumeFromUnselectedStage {
                stage: resume_from_stage,
            });
        }

        match &self.source {
            CalibrationResumeSource::SameProcess => {
                if execution_mode != CalibrationExecutionMode::Memory {
                    return Err(
                        CalibrationResumeSettingsError::SameProcessResumeRequiresMemoryMode,
                    );
                }
            }
            CalibrationResumeSource::PersistedCheckpointReference {
                checkpoint_reference,
            } => {
                if execution_mode != CalibrationExecutionMode::Disk {
                    return Err(
                        CalibrationResumeSettingsError::PersistedCheckpointResumeRequiresDiskMode,
                    );
                }

                if checkpoint_reference.trim().is_empty() {
                    return Err(
                        CalibrationResumeSettingsError::EmptyCheckpointReferenceForPersistedResume,
                    );
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationResumeSettingsError {
    ResumeFromUnselectedStage { stage: CalibrationStage },
    SameProcessResumeRequiresMemoryMode,
    PersistedCheckpointResumeRequiresDiskMode,
    EmptyCheckpointReferenceForPersistedResume,
}

impl Display for CalibrationResumeSettingsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ResumeFromUnselectedStage { stage } => write!(
                formatter,
                "resume_settings.resume_from_stage `{stage:?}` must be selected in selected_stages"
            ),
            Self::SameProcessResumeRequiresMemoryMode => write!(
                formatter,
                "resume_settings.source=same_process is only supported when execution_mode=memory"
            ),
            Self::PersistedCheckpointResumeRequiresDiskMode => write!(
                formatter,
                "resume_settings.source=persisted_checkpoint_reference is only supported when execution_mode=disk"
            ),
            Self::EmptyCheckpointReferenceForPersistedResume => write!(
                formatter,
                "resume_settings.checkpoint_reference must be non-empty when provided"
            ),
        }
    }
}

impl Error for CalibrationResumeSettingsError {}
