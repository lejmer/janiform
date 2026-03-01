use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::randomness::splitmix64_output_mixer;
pub const CALIBRATION_RUN_SPEC_SCHEMA_VERSION: &str = "calibration.orchestration.run_spec.v1";
pub const CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION: &str = "calibration.orchestration.stage_graph.v1";
pub const CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION: &str =
    "calibration.orchestration.run_contract.v1";
pub const CALIBRATION_SOLVER_SETTINGS_CAPTURE_POLICY: &str =
    "capture_stage_solver_settings_per_run_contract.v1";
pub const CALIBRATION_STAGE_SEED_DERIVATION_POLICY: &str =
    "global_seed_splitmix64_per_stage_with_override_precedence.v1";

/// Caller-surface default policy for execution-mode resolution.
pub const CALIBRATION_EXECUTION_MODE_DEFAULT_POLICY: &str =
    "cli_defaults_to_disk_python_importable_api_defaults_to_memory.v1";
pub const CALIBRATION_SEASONALITY_STAGE_EXECUTION_REPORT_SCHEMA_VERSION: &str =
    "calibration.orchestration.stage_execution.seasonality.report.v1";
pub const CALIBRATION_SEASONALITY_STAGE_OUTPUT_SCHEMA_VERSION: &str =
    "calibration.orchestration.stage_output.seasonality.v1";
pub const CALIBRATION_SEASONALITY_STAGE_HANDOFF_POLICY: &str =
    "explicit_run_spec_handoffs_mode_aware_transport.v1";
pub const CALIBRATION_SEASONALITY_STAGE_DEFAULT_ESTIMATOR_VERSION: &str =
    "seasonality_estimator.v1";
pub const CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION: &str =
    "calibration.orchestration.stage_execution.hawkes.report.v1";
pub const CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION: &str =
    "calibration.orchestration.stage_output.hawkes.v1";
pub const CALIBRATION_HAWKES_STAGE_HANDOFF_POLICY: &str =
    "explicit_run_spec_handoffs_mode_aware_transport.v1";
pub const CALIBRATION_RETURN_COMPONENTS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION: &str =
    "calibration.orchestration.stage_execution.return_components.report.v1";
pub const CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION: &str =
    "calibration.orchestration.stage_output.return_components.v1";
pub const CALIBRATION_RETURN_COMPONENTS_STAGE_HANDOFF_POLICY: &str =
    "explicit_run_spec_handoffs_mode_aware_transport.v1";
pub const CALIBRATION_RETURN_COMPONENTS_STAGE_OBJECTIVE_WEIGHTING_POLICY: &str =
    "weighted_sum_of_configured_objective_terms.v1";
pub const CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION: &str =
    "calibration.orchestration.stage_checkpoint.v1";
pub const CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY: &str =
    "checkpoint_uri_with_run_id_and_stage_token.v1";
pub const CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY: &str = "atomic_tempfile_then_rename.v1";
pub const CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION: &str =
    "calibration.artifact_registry.manifest.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationStage {
    Seasonality,
    Hawkes,
    ReturnComponents,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationExecutionMode {
    Memory,
    Disk,
}

/// Originating caller surface used for execution-mode defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationCallerSurface {
    Cli,
    PythonImportableApi,
}

/// Resolve the default execution mode for one caller surface.
pub const fn default_calibration_execution_mode_for_surface(
    caller_surface: CalibrationCallerSurface,
) -> CalibrationExecutionMode {
    match caller_surface {
        CalibrationCallerSurface::Cli => CalibrationExecutionMode::Disk,
        CalibrationCallerSurface::PythonImportableApi => CalibrationExecutionMode::Memory,
    }
}

/// Resolve execution mode using caller-surface defaults and optional override.
pub const fn resolve_calibration_execution_mode_for_surface(
    caller_surface: CalibrationCallerSurface,
    execution_mode_override: Option<CalibrationExecutionMode>,
) -> CalibrationExecutionMode {
    match execution_mode_override {
        Some(execution_mode) => execution_mode,
        None => default_calibration_execution_mode_for_surface(caller_surface),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalibrationRunDataInputs {
    pub exchange_code: String,
    pub trade_events_data_reference: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub option_surface_data_reference: Option<String>,
}

impl CalibrationRunDataInputs {
    pub fn try_new(
        exchange_code: impl Into<String>,
        trade_events_data_reference: impl Into<String>,
        option_surface_data_reference: Option<String>,
    ) -> Result<Self, CalibrationRunDataInputsError> {
        let data_inputs = Self {
            exchange_code: exchange_code.into(),
            trade_events_data_reference: trade_events_data_reference.into(),
            option_surface_data_reference,
        };
        data_inputs.validate()?;
        Ok(data_inputs)
    }

    pub fn validate(&self) -> Result<(), CalibrationRunDataInputsError> {
        if self.exchange_code.trim().is_empty() {
            return Err(CalibrationRunDataInputsError::MissingExchangeCode);
        }

        if self.trade_events_data_reference.trim().is_empty() {
            return Err(CalibrationRunDataInputsError::MissingTradeEventsDataReference);
        }

        if let Some(option_surface_data_reference) = &self.option_surface_data_reference
            && option_surface_data_reference.trim().is_empty()
        {
            return Err(CalibrationRunDataInputsError::EmptyOptionSurfaceDataReference);
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationRunDataInputsError {
    MissingExchangeCode,
    MissingTradeEventsDataReference,
    EmptyOptionSurfaceDataReference,
}

impl Display for CalibrationRunDataInputsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingExchangeCode => {
                write!(
                    formatter,
                    "data_inputs.exchange_code must be a non-empty string"
                )
            }
            Self::MissingTradeEventsDataReference => write!(
                formatter,
                "data_inputs.trade_events_data_reference must be a non-empty string"
            ),
            Self::EmptyOptionSurfaceDataReference => write!(
                formatter,
                "data_inputs.option_surface_data_reference must be non-empty when provided"
            ),
        }
    }
}

impl Error for CalibrationRunDataInputsError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalibrationStageSeedOverride {
    pub stage: CalibrationStage,
    pub seed: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationStageSeedSource {
    DerivedFromGlobalSeed,
    StageOverride,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalibrationResolvedStageSeed {
    pub run_global_seed: u64,
    pub stage_seed: u64,
    pub stage_seed_source: CalibrationStageSeedSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalibrationRunSeedSettings {
    pub global_seed: u64,
    #[serde(default)]
    pub stage_seed_overrides: Vec<CalibrationStageSeedOverride>,
}

impl CalibrationRunSeedSettings {
    pub fn try_new(
        global_seed: u64,
        stage_seed_overrides: Vec<CalibrationStageSeedOverride>,
    ) -> Result<Self, CalibrationRunSeedSettingsError> {
        let seed_settings = Self {
            global_seed,
            stage_seed_overrides,
        };
        seed_settings.validate()?;
        Ok(seed_settings)
    }

    pub fn validate(&self) -> Result<(), CalibrationRunSeedSettingsError> {
        let mut stage_overrides = BTreeSet::new();
        for stage_seed_override in &self.stage_seed_overrides {
            if !stage_overrides.insert(stage_seed_override.stage) {
                return Err(
                    CalibrationRunSeedSettingsError::DuplicateStageSeedOverride {
                        stage: stage_seed_override.stage,
                    },
                );
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationRunSeedSettingsError {
    DuplicateStageSeedOverride { stage: CalibrationStage },
}

impl Display for CalibrationRunSeedSettingsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateStageSeedOverride { stage } => write!(
                formatter,
                "seed_settings.stage_seed_overrides contains duplicate stage `{stage:?}`"
            ),
        }
    }
}

impl Error for CalibrationRunSeedSettingsError {}

/// Stage-to-stage handoff transport for orchestration contracts.
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

    fn validate(
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

    fn validate(
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationOptimizerSettings {
    pub solver_name: String,
    pub max_iterations: usize,
    pub absolute_tolerance: f64,
    pub relative_tolerance: f64,
    pub constraint_penalty_weight: f64,
}

impl CalibrationOptimizerSettings {
    pub fn try_new(
        solver_name: impl Into<String>,
        max_iterations: usize,
        absolute_tolerance: f64,
        relative_tolerance: f64,
        constraint_penalty_weight: f64,
    ) -> Result<Self, CalibrationOptimizerSettingsError> {
        let optimizer_settings = Self {
            solver_name: solver_name.into(),
            max_iterations,
            absolute_tolerance,
            relative_tolerance,
            constraint_penalty_weight,
        };
        optimizer_settings.validate()?;
        Ok(optimizer_settings)
    }

    pub fn validate(&self) -> Result<(), CalibrationOptimizerSettingsError> {
        if self.solver_name.trim().is_empty() {
            return Err(CalibrationOptimizerSettingsError::MissingSolverName);
        }
        if self.max_iterations == 0 {
            return Err(CalibrationOptimizerSettingsError::InvalidMaxIterations {
                value: self.max_iterations,
            });
        }
        if !self.absolute_tolerance.is_finite() || self.absolute_tolerance <= 0.0 {
            return Err(
                CalibrationOptimizerSettingsError::InvalidAbsoluteTolerance {
                    value: self.absolute_tolerance,
                },
            );
        }
        if !self.relative_tolerance.is_finite() || self.relative_tolerance <= 0.0 {
            return Err(
                CalibrationOptimizerSettingsError::InvalidRelativeTolerance {
                    value: self.relative_tolerance,
                },
            );
        }
        if !self.constraint_penalty_weight.is_finite() || self.constraint_penalty_weight < 0.0 {
            return Err(
                CalibrationOptimizerSettingsError::InvalidConstraintPenaltyWeight {
                    value: self.constraint_penalty_weight,
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationOptimizerSettingsError {
    MissingSolverName,
    InvalidMaxIterations { value: usize },
    InvalidAbsoluteTolerance { value: f64 },
    InvalidRelativeTolerance { value: f64 },
    InvalidConstraintPenaltyWeight { value: f64 },
}

impl Display for CalibrationOptimizerSettingsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSolverName => {
                write!(formatter, "solver_name must be a non-empty string")
            }
            Self::InvalidMaxIterations { value } => write!(
                formatter,
                "max_iterations must be strictly positive, found {value}"
            ),
            Self::InvalidAbsoluteTolerance { value } => write!(
                formatter,
                "absolute_tolerance must be finite and strictly positive, found {value}"
            ),
            Self::InvalidRelativeTolerance { value } => write!(
                formatter,
                "relative_tolerance must be finite and strictly positive, found {value}"
            ),
            Self::InvalidConstraintPenaltyWeight { value } => write!(
                formatter,
                "constraint_penalty_weight must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for CalibrationOptimizerSettingsError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationStageSolverSettings {
    pub stage: CalibrationStage,
    pub optimizer_settings: CalibrationOptimizerSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationRunSpec {
    pub run_id: String,
    pub data_inputs: CalibrationRunDataInputs,
    pub execution_mode: CalibrationExecutionMode,
    pub selected_stages: Vec<CalibrationStage>,
    pub seed_settings: CalibrationRunSeedSettings,
    pub optimizer_settings_by_stage: Vec<CalibrationStageSolverSettings>,
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

    pub fn build(self) -> Result<CalibrationRunSpec, CalibrationRunSpecError> {
        let run_spec = CalibrationRunSpec {
            run_id: self.run_id,
            data_inputs: self.data_inputs,
            execution_mode: self.execution_mode,
            selected_stages: self.selected_stages,
            seed_settings: self.seed_settings,
            optimizer_settings_by_stage: self.optimizer_settings_by_stage,
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
            stage_handoffs: Vec::new(),
            resume_settings: None,
        }
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
            Self::InvalidStageHandoff { source } => match source {
                CalibrationStageHandoffError::PersistedArtifactHandoffRequiresDiskMode {
                    ..
                } => CalibrationRunSpecErrorClassification::ModeIncompatibleOptions,
                _ => CalibrationRunSpecErrorClassification::InvalidInput,
            },
            Self::InvalidResumeSettings { source } => match source {
                CalibrationResumeSettingsError::SameProcessResumeRequiresMemoryMode
                | CalibrationResumeSettingsError::PersistedCheckpointResumeRequiresDiskMode => {
                    CalibrationRunSpecErrorClassification::ModeIncompatibleOptions
                }
                _ => CalibrationRunSpecErrorClassification::InvalidInput,
            },
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
            Self::InvalidStageSolverSettings { source, .. } => Some(source),
            Self::InvalidStageHandoff { source } => Some(source),
            Self::InvalidResumeSettings { source } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CalibrationStageDependency {
    pub stage: CalibrationStage,
    pub depends_on: CalibrationStage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CalibrationStageGraph {
    #[serde(default)]
    pub dependencies: Vec<CalibrationStageDependency>,
}

impl CalibrationStageGraph {
    pub const fn new(dependencies: Vec<CalibrationStageDependency>) -> Self {
        Self { dependencies }
    }

    pub fn resolve_execution_order(
        &self,
        selected_stages: &[CalibrationStage],
    ) -> Result<Vec<CalibrationStage>, CalibrationStageGraphError> {
        if selected_stages.is_empty() {
            return Err(CalibrationStageGraphError::MissingSelectedStages);
        }

        let mut stage_positions = BTreeMap::new();
        for (position, stage) in selected_stages.iter().copied().enumerate() {
            if stage_positions.insert(stage, position).is_some() {
                return Err(CalibrationStageGraphError::DuplicateSelectedStage { stage });
            }
        }

        let stage_count = selected_stages.len();
        let mut indegree_by_position = vec![0usize; stage_count];
        let mut adjacency_by_position = vec![Vec::<usize>::new(); stage_count];
        let mut seen_dependencies = BTreeSet::new();

        for dependency in &self.dependencies {
            let Some(&stage_position) = stage_positions.get(&dependency.stage) else {
                return Err(CalibrationStageGraphError::UnsupportedDependency {
                    stage: dependency.stage,
                    depends_on: dependency.depends_on,
                    reason: "stage is not selected in run_spec.selected_stages",
                });
            };
            let Some(&depends_on_position) = stage_positions.get(&dependency.depends_on) else {
                return Err(CalibrationStageGraphError::UnsupportedDependency {
                    stage: dependency.stage,
                    depends_on: dependency.depends_on,
                    reason: "depends_on stage is not selected in run_spec.selected_stages",
                });
            };

            if dependency.stage == dependency.depends_on {
                return Err(CalibrationStageGraphError::SelfDependency {
                    stage: dependency.stage,
                });
            }

            if !seen_dependencies.insert(*dependency) {
                return Err(CalibrationStageGraphError::DuplicateDependency {
                    stage: dependency.stage,
                    depends_on: dependency.depends_on,
                });
            }

            indegree_by_position[stage_position] =
                indegree_by_position[stage_position].saturating_add(1);

            adjacency_by_position[depends_on_position].push(stage_position);
        }

        for dependencies in &mut adjacency_by_position {
            dependencies.sort_unstable();
        }

        let mut ready_queue = VecDeque::new();
        for (position, indegree) in indegree_by_position.iter().copied().enumerate() {
            if indegree == 0 {
                ready_queue.push_back(position);
            }
        }

        let mut execution_positions = Vec::with_capacity(stage_count);
        while let Some(current_position) = ready_queue.pop_front() {
            execution_positions.push(current_position);
            for dependent_position in adjacency_by_position[current_position].iter().copied() {
                if indegree_by_position[dependent_position] == 0 {
                    continue;
                }

                indegree_by_position[dependent_position] -= 1;

                if indegree_by_position[dependent_position] == 0 {
                    ready_queue.push_back(dependent_position);
                }
            }
        }

        if execution_positions.len() != stage_count {
            let cyclic_stages = indegree_by_position
                .iter()
                .copied()
                .enumerate()
                .filter_map(|(position, indegree)| {
                    if indegree > 0 {
                        Some(selected_stages[position])
                    } else {
                        None
                    }
                })
                .collect();

            return Err(CalibrationStageGraphError::CyclicDependency {
                stages: cyclic_stages,
            });
        }

        let execution_order = execution_positions
            .iter()
            .copied()
            .map(|position| selected_stages[position])
            .collect();
        Ok(execution_order)
    }

    pub const fn schema_version() -> &'static str {
        CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationStageGraphError {
    MissingSelectedStages,
    DuplicateSelectedStage {
        stage: CalibrationStage,
    },
    UnsupportedDependency {
        stage: CalibrationStage,
        depends_on: CalibrationStage,
        reason: &'static str,
    },
    SelfDependency {
        stage: CalibrationStage,
    },
    DuplicateDependency {
        stage: CalibrationStage,
        depends_on: CalibrationStage,
    },
    CyclicDependency {
        stages: Vec<CalibrationStage>,
    },
}

impl Display for CalibrationStageGraphError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSelectedStages => write!(
                formatter,
                "stage graph validation requires at least one selected stage"
            ),
            Self::DuplicateSelectedStage { stage } => write!(
                formatter,
                "run_spec.selected_stages contains duplicate stage `{stage:?}`"
            ),
            Self::UnsupportedDependency {
                stage,
                depends_on,
                reason,
            } => write!(
                formatter,
                "unsupported stage dependency `{stage:?}` -> `{depends_on:?}`: {reason}"
            ),
            Self::SelfDependency { stage } => write!(
                formatter,
                "stage graph contains self dependency for stage `{stage:?}`"
            ),
            Self::DuplicateDependency { stage, depends_on } => write!(
                formatter,
                "stage graph contains duplicate dependency `{stage:?}` -> `{depends_on:?}`"
            ),
            Self::CyclicDependency { stages } => write!(
                formatter,
                "stage graph contains cyclic dependency involving stages: {stages:?}"
            ),
        }
    }
}

impl Error for CalibrationStageGraphError {}

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

/// Resolve the seed used for one stage after applying stage-level overrides.
pub fn resolve_calibration_stage_seed(
    run_spec: &CalibrationRunSpec,
    stage: CalibrationStage,
) -> u64 {
    resolve_calibration_stage_seed_metadata(run_spec, stage).stage_seed
}

pub fn resolve_calibration_stage_seed_metadata(
    run_spec: &CalibrationRunSpec,
    stage: CalibrationStage,
) -> CalibrationResolvedStageSeed {
    if let Some(stage_seed_override) = run_spec
        .seed_settings
        .stage_seed_overrides
        .iter()
        .find(|stage_seed_override| stage_seed_override.stage == stage)
    {
        return CalibrationResolvedStageSeed {
            run_global_seed: run_spec.seed_settings.global_seed,
            stage_seed: stage_seed_override.seed,
            stage_seed_source: CalibrationStageSeedSource::StageOverride,
        };
    }

    CalibrationResolvedStageSeed {
        run_global_seed: run_spec.seed_settings.global_seed,
        stage_seed: derive_calibration_stage_seed_from_global_seed(
            run_spec.seed_settings.global_seed,
            stage,
        ),
        stage_seed_source: CalibrationStageSeedSource::DerivedFromGlobalSeed,
    }
}

pub fn derive_calibration_stage_seed_from_global_seed(
    global_seed: u64,
    stage: CalibrationStage,
) -> u64 {
    splitmix64_output_mixer(global_seed ^ calibration_stage_seed_derivation_nonce(stage))
}

const fn calibration_stage_seed_derivation_nonce(stage: CalibrationStage) -> u64 {
    match stage {
        CalibrationStage::Seasonality => 0x5E45_0A3D_B51F_4E91,
        CalibrationStage::Hawkes => 0xA67B_3D2C_17F8_94E0,
        CalibrationStage::ReturnComponents => 0x3CF2_D8A1_6B94_C705,
    }
}

pub(crate) fn stage_solver_settings_for_stage(
    run_spec: &CalibrationRunSpec,
    stage: CalibrationStage,
) -> Option<CalibrationOptimizerSettings> {
    run_spec
        .optimizer_settings_by_stage
        .iter()
        .find(|stage_solver_settings| stage_solver_settings.stage == stage)
        .map(|stage_solver_settings| stage_solver_settings.optimizer_settings.clone())
}
