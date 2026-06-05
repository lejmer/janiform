use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use super::{
    CALIBRATION_RUN_SPEC_SCHEMA_VERSION, CalibrationExecutionMode, CalibrationObjectivePolicy,
    CalibrationObjectivePolicyError, CalibrationOptimizerSettingsError, CalibrationResumeSettings,
    CalibrationResumeSettingsError, CalibrationRunDataInputs, CalibrationRunDataInputsError,
    CalibrationRunSeedSettings, CalibrationRunSeedSettingsError, CalibrationStage,
    CalibrationStageHandoff, CalibrationStageHandoffError, CalibrationStageHandoffTransport,
    CalibrationStageSolverSettings,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationRunSpec {
    pub run_id: String,
    pub data_inputs: CalibrationRunDataInputs,
    pub execution_mode: CalibrationExecutionMode,
    pub selected_stages: Vec<CalibrationStage>,
    pub seed_settings: CalibrationRunSeedSettings,
    pub optimizer_settings_by_stage: Vec<CalibrationStageSolverSettings>,
    #[serde(default)]
    pub objective_policy: CalibrationObjectivePolicy,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stage_handoffs: Vec<CalibrationStageHandoff>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_settings: Option<CalibrationResumeSettings>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationRunSpecBuilder {
    run_id: String,
    data_inputs: CalibrationRunDataInputs,
    execution_mode: CalibrationExecutionMode,
    selected_stages: Vec<CalibrationStage>,
    seed_settings: CalibrationRunSeedSettings,
    optimizer_settings_by_stage: Vec<CalibrationStageSolverSettings>,
    objective_policy: CalibrationObjectivePolicy,
    stage_handoffs: Vec<CalibrationStageHandoff>,
    resume_settings: Option<CalibrationResumeSettings>,
}

impl CalibrationRunSpecBuilder {
    pub fn with_stage_handoffs(mut self, stage_handoffs: Vec<CalibrationStageHandoff>) -> Self {
        self.stage_handoffs = stage_handoffs;
        self
    }

    pub fn with_resume_settings(mut self, resume_settings: CalibrationResumeSettings) -> Self {
        self.resume_settings = Some(resume_settings);
        self
    }

    pub fn with_objective_policy(mut self, objective_policy: CalibrationObjectivePolicy) -> Self {
        self.objective_policy = objective_policy;
        self
    }

    pub fn build(self) -> Result<CalibrationRunSpec, CalibrationRunSpecError> {
        let run_spec = CalibrationRunSpec {
            run_id: self.run_id,
            data_inputs: self.data_inputs,
            execution_mode: self.execution_mode,
            selected_stages: self.selected_stages,
            seed_settings: self.seed_settings,
            optimizer_settings_by_stage: self.optimizer_settings_by_stage,
            objective_policy: self.objective_policy,
            stage_handoffs: self.stage_handoffs,
            resume_settings: self.resume_settings,
        };

        run_spec.validate()?;

        Ok(run_spec)
    }
}

impl CalibrationRunSpec {
    pub fn try_new(
        run_id: impl Into<String>,
        data_inputs: CalibrationRunDataInputs,
        execution_mode: CalibrationExecutionMode,
        selected_stages: Vec<CalibrationStage>,
        seed_settings: CalibrationRunSeedSettings,
        optimizer_settings_by_stage: Vec<CalibrationStageSolverSettings>,
    ) -> Result<Self, CalibrationRunSpecError> {
        Self::builder(
            run_id,
            data_inputs,
            execution_mode,
            selected_stages,
            seed_settings,
            optimizer_settings_by_stage,
        )
        .build()
    }

    pub fn builder(
        run_id: impl Into<String>,
        data_inputs: CalibrationRunDataInputs,
        execution_mode: CalibrationExecutionMode,
        selected_stages: Vec<CalibrationStage>,
        seed_settings: CalibrationRunSeedSettings,
        optimizer_settings_by_stage: Vec<CalibrationStageSolverSettings>,
    ) -> CalibrationRunSpecBuilder {
        CalibrationRunSpecBuilder {
            run_id: run_id.into(),
            data_inputs,
            execution_mode,
            selected_stages,
            seed_settings,
            optimizer_settings_by_stage,
            objective_policy: CalibrationObjectivePolicy::default(),
            stage_handoffs: Vec::new(),
            resume_settings: None,
        }
    }

    /// Find the configured handoff from one selected stage to another.
    pub fn stage_handoff(
        &self,
        from_stage: CalibrationStage,
        to_stage: CalibrationStage,
    ) -> Option<&CalibrationStageHandoff> {
        self.stage_handoffs.iter().find(|stage_handoff| {
            stage_handoff.from_stage == from_stage && stage_handoff.to_stage == to_stage
        })
    }

    /// Resolve the handoff transport for a stage edge.
    ///
    /// If the run spec does not contain an explicit handoff for the edge, the
    /// default follows the execution mode: memory runs pass in-memory payloads,
    /// while disk runs pass persisted artifact references.
    pub fn stage_handoff_transport_or_default(
        &self,
        from_stage: CalibrationStage,
        to_stage: CalibrationStage,
    ) -> CalibrationStageHandoffTransport {
        self.stage_handoff(from_stage, to_stage)
            .map(CalibrationStageHandoff::transport)
            .unwrap_or(match self.execution_mode {
                CalibrationExecutionMode::Memory => {
                    CalibrationStageHandoffTransport::InMemoryOutput
                }
                CalibrationExecutionMode::Disk => {
                    CalibrationStageHandoffTransport::PersistedArtifactReference
                }
            })
    }

    pub fn validate(&self) -> Result<(), CalibrationRunSpecError> {
        if self.run_id.trim().is_empty() {
            return Err(CalibrationRunSpecError::MissingRunId);
        }

        self.data_inputs
            .validate()
            .map_err(|source| CalibrationRunSpecError::InvalidDataInputs { source })?;

        if self.selected_stages.is_empty() {
            return Err(CalibrationRunSpecError::MissingSelectedStages);
        }

        let mut selected_stages = BTreeSet::new();
        for stage in &self.selected_stages {
            if !selected_stages.insert(*stage) {
                return Err(CalibrationRunSpecError::DuplicateSelectedStage { stage: *stage });
            }
        }

        self.seed_settings
            .validate()
            .map_err(|source| CalibrationRunSpecError::InvalidSeedSettings { source })?;

        self.objective_policy
            .validate()
            .map_err(|source| CalibrationRunSpecError::InvalidObjectivePolicy { source })?;

        for stage_seed_override in &self.seed_settings.stage_seed_overrides {
            if !selected_stages.contains(&stage_seed_override.stage) {
                return Err(CalibrationRunSpecError::SeedOverrideForUnselectedStage {
                    stage: stage_seed_override.stage,
                });
            }
        }

        let mut configured_stage_settings = BTreeMap::new();

        for stage_solver_settings in &self.optimizer_settings_by_stage {
            stage_solver_settings
                .optimizer_settings
                .validate()
                .map_err(
                    |source| CalibrationRunSpecError::InvalidStageSolverSettings {
                        stage: stage_solver_settings.stage,
                        source,
                    },
                )?;

            if configured_stage_settings
                .insert(stage_solver_settings.stage, ())
                .is_some()
            {
                return Err(CalibrationRunSpecError::DuplicateStageSolverSettings {
                    stage: stage_solver_settings.stage,
                });
            }
        }

        for selected_stage in &self.selected_stages {
            if !configured_stage_settings.contains_key(selected_stage) {
                return Err(CalibrationRunSpecError::MissingStageSolverSettings {
                    stage: *selected_stage,
                });
            }
        }

        for configured_stage in configured_stage_settings.keys() {
            if !selected_stages.contains(configured_stage) {
                return Err(CalibrationRunSpecError::UnexpectedStageSolverSettings {
                    stage: *configured_stage,
                });
            }
        }

        let mut stage_handoff_pairs = BTreeSet::new();

        for stage_handoff in &self.stage_handoffs {
            stage_handoff
                .validate(self.execution_mode, &selected_stages)
                .map_err(|source| CalibrationRunSpecError::InvalidStageHandoff { source })?;

            if !stage_handoff_pairs.insert((stage_handoff.from_stage, stage_handoff.to_stage)) {
                return Err(CalibrationRunSpecError::DuplicateStageHandoff {
                    from_stage: stage_handoff.from_stage,
                    to_stage: stage_handoff.to_stage,
                });
            }
        }

        if let Some(resume_settings) = &self.resume_settings {
            resume_settings
                .validate(self.execution_mode, &selected_stages)
                .map_err(|source| CalibrationRunSpecError::InvalidResumeSettings { source })?;
        }

        Ok(())
    }

    pub const fn schema_version() -> &'static str {
        CALIBRATION_RUN_SPEC_SCHEMA_VERSION
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationRunSpecError {
    MissingRunId,
    InvalidDataInputs {
        source: CalibrationRunDataInputsError,
    },
    MissingSelectedStages,
    DuplicateSelectedStage {
        stage: CalibrationStage,
    },
    InvalidSeedSettings {
        source: CalibrationRunSeedSettingsError,
    },
    InvalidObjectivePolicy {
        source: CalibrationObjectivePolicyError,
    },
    SeedOverrideForUnselectedStage {
        stage: CalibrationStage,
    },
    InvalidStageSolverSettings {
        stage: CalibrationStage,
        source: CalibrationOptimizerSettingsError,
    },
    DuplicateStageSolverSettings {
        stage: CalibrationStage,
    },
    MissingStageSolverSettings {
        stage: CalibrationStage,
    },
    UnexpectedStageSolverSettings {
        stage: CalibrationStage,
    },
    InvalidStageHandoff {
        source: CalibrationStageHandoffError,
    },
    DuplicateStageHandoff {
        from_stage: CalibrationStage,
        to_stage: CalibrationStage,
    },
    InvalidResumeSettings {
        source: CalibrationResumeSettingsError,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalibrationRunSpecErrorClassification {
    InvalidInput,
    ModeIncompatibleOptions,
}

impl CalibrationRunSpecError {
    /// Return a deterministic error class for caller-side handling.
    pub const fn classification(&self) -> CalibrationRunSpecErrorClassification {
        match self {
            Self::InvalidStageHandoff {
                source:
                    CalibrationStageHandoffError::PersistedArtifactHandoffRequiresDiskMode { .. },
            } => CalibrationRunSpecErrorClassification::ModeIncompatibleOptions,
            Self::InvalidStageHandoff { .. } => CalibrationRunSpecErrorClassification::InvalidInput,
            Self::InvalidResumeSettings {
                source:
                    CalibrationResumeSettingsError::SameProcessResumeRequiresMemoryMode
                    | CalibrationResumeSettingsError::PersistedCheckpointResumeRequiresDiskMode,
            } => CalibrationRunSpecErrorClassification::ModeIncompatibleOptions,
            Self::InvalidResumeSettings { .. } => {
                CalibrationRunSpecErrorClassification::InvalidInput
            }
            _ => CalibrationRunSpecErrorClassification::InvalidInput,
        }
    }
}

impl Display for CalibrationRunSpecError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingRunId => write!(formatter, "run_id must be a non-empty string"),
            Self::InvalidDataInputs { source } => {
                write!(formatter, "data_inputs payload is invalid: {source}")
            }
            Self::MissingSelectedStages => write!(
                formatter,
                "selected_stages must include at least one calibration stage"
            ),
            Self::DuplicateSelectedStage { stage } => write!(
                formatter,
                "selected_stages contains duplicate stage `{stage:?}`"
            ),
            Self::InvalidSeedSettings { source } => {
                write!(formatter, "seed_settings payload is invalid: {source}")
            }
            Self::InvalidObjectivePolicy { source } => {
                write!(formatter, "objective_policy payload is invalid: {source}")
            }
            Self::SeedOverrideForUnselectedStage { stage } => write!(
                formatter,
                "seed_settings.stage_seed_overrides contains stage `{stage:?}` that is not selected"
            ),
            Self::InvalidStageSolverSettings { stage, source } => write!(
                formatter,
                "optimizer_settings_by_stage entry for stage `{stage:?}` is invalid: {source}"
            ),
            Self::DuplicateStageSolverSettings { stage } => write!(
                formatter,
                "optimizer_settings_by_stage contains duplicate stage `{stage:?}`"
            ),
            Self::MissingStageSolverSettings { stage } => write!(
                formatter,
                "optimizer_settings_by_stage must include solver settings for selected stage `{stage:?}`"
            ),
            Self::UnexpectedStageSolverSettings { stage } => write!(
                formatter,
                "optimizer_settings_by_stage contains stage `{stage:?}` that is not selected"
            ),
            Self::InvalidStageHandoff { source } => {
                write!(formatter, "stage_handoffs payload is invalid: {source}")
            }
            Self::DuplicateStageHandoff {
                from_stage,
                to_stage,
            } => write!(
                formatter,
                "stage_handoffs contains duplicate entry `{from_stage:?}` -> `{to_stage:?}`"
            ),
            Self::InvalidResumeSettings { source } => {
                write!(formatter, "resume_settings payload is invalid: {source}")
            }
        }
    }
}

impl Error for CalibrationRunSpecError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidDataInputs { source } => Some(source),
            Self::InvalidSeedSettings { source } => Some(source),
            Self::InvalidObjectivePolicy { source } => Some(source),
            Self::InvalidStageSolverSettings { source, .. } => Some(source),
            Self::InvalidStageHandoff { source } => Some(source),
            Self::InvalidResumeSettings { source } => Some(source),
            _ => None,
        }
    }
}
