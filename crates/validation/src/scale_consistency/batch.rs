//! Batch orchestration and mode-aware resume for scale-consistency validation runs.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

use janiform_calibration_artifacts::CalibrationExecutionMode;
use serde::{Deserialize, Serialize};

use super::super::contracts::RegressionCaseStatus;
use super::contracts::{
    SCALE_CONSISTENCY_PAIRED_RUN_CONTRACT_SCHEMA_VERSION,
    SCALE_CONSISTENCY_PAIRED_RUN_SPEC_SCHEMA_VERSION, ScaleConsistencyPairedExperimentScenarioSpec,
    ScaleConsistencyPairedRunContract, ScaleConsistencyRandomControlMetadata,
};

pub const SCALE_CONSISTENCY_BATCH_RUN_REQUEST_SCHEMA_VERSION: &str =
    "validation.scale_consistency.batch_run_request.v1";
pub const SCALE_CONSISTENCY_BATCH_RUN_REPORT_SCHEMA_VERSION: &str =
    "validation.scale_consistency.batch_run_report.v1";
pub const SCALE_CONSISTENCY_BATCH_CHECKPOINT_SCHEMA_VERSION: &str =
    "validation.scale_consistency.batch_checkpoint.v1";
pub const SCALE_CONSISTENCY_BATCH_ORDERING_POLICY: &str =
    "preserve_run_spec_scenario_order_after_selection_resume_and_result_collation.v1";
pub const SCALE_CONSISTENCY_BATCH_RESUME_POLICY: &str = "memory_mode_uses_same_process_resume_only_and_disk_mode_uses_persisted_checkpoint_reference_only.v1";
pub const SCALE_CONSISTENCY_BATCH_CHECKPOINT_REFERENCE_POLICY: &str = "persisted_checkpoint_reference_is_a_json_checkpoint_file_path_and_memory_resume_uses_run_id_lookup.v1";
pub const SCALE_CONSISTENCY_BATCH_ARTIFACT_PERSISTENCE_POLICY: &str = "disk_mode_optionally_persists_checkpoint_and_report_json_files_when_artifact_output_is_configured.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleConsistencyBatchScenarioSelection {
    All,
    FailedFromResume,
    ScenarioIds { scenario_ids: Vec<String> },
}

impl Default for ScaleConsistencyBatchScenarioSelection {
    fn default() -> Self {
        Self::All
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum ScaleConsistencyBatchResumeSource {
    SameProcess,
    PersistedCheckpointReference { checkpoint_reference: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyBatchResumeSettings {
    #[serde(flatten)]
    pub source: ScaleConsistencyBatchResumeSource,
}

impl ScaleConsistencyBatchResumeSettings {
    pub const fn same_process() -> Self {
        Self {
            source: ScaleConsistencyBatchResumeSource::SameProcess,
        }
    }

    pub fn persisted_checkpoint_reference(checkpoint_reference: impl Into<String>) -> Self {
        Self {
            source: ScaleConsistencyBatchResumeSource::PersistedCheckpointReference {
                checkpoint_reference: checkpoint_reference.into(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyBatchRunRequest {
    pub schema_version: String,
    pub run_contract: ScaleConsistencyPairedRunContract,
    #[serde(default)]
    pub scenario_selection: ScaleConsistencyBatchScenarioSelection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_settings: Option<ScaleConsistencyBatchResumeSettings>,
}

impl ScaleConsistencyBatchRunRequest {
    pub fn try_new(
        run_contract: ScaleConsistencyPairedRunContract,
    ) -> Result<Self, ScaleConsistencyBatchRunRequestError> {
        let request = Self {
            schema_version: SCALE_CONSISTENCY_BATCH_RUN_REQUEST_SCHEMA_VERSION.to_string(),
            run_contract,
            scenario_selection: ScaleConsistencyBatchScenarioSelection::All,
            resume_settings: None,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn with_scenario_selection(
        mut self,
        scenario_selection: ScaleConsistencyBatchScenarioSelection,
    ) -> Self {
        self.scenario_selection = scenario_selection;
        self
    }

    pub fn with_resume_settings(
        mut self,
        resume_settings: ScaleConsistencyBatchResumeSettings,
    ) -> Self {
        self.resume_settings = Some(resume_settings);
        self
    }

    pub fn validate(&self) -> Result<(), ScaleConsistencyBatchRunRequestError> {
        if self.schema_version != SCALE_CONSISTENCY_BATCH_RUN_REQUEST_SCHEMA_VERSION {
            return Err(ScaleConsistencyBatchRunRequestError::InvalidSchemaVersion {
                expected: SCALE_CONSISTENCY_BATCH_RUN_REQUEST_SCHEMA_VERSION.to_string(),
                actual: self.schema_version.clone(),
            });
        }

        let execution_mode = self.run_contract.run_spec.execution_mode;
        let available_scenario_ids = self
            .run_contract
            .run_spec
            .scenarios
            .iter()
            .map(|scenario| scenario.scenario_id.as_str())
            .collect::<Vec<_>>();
        let available_scenario_id_set: BTreeSet<&str> =
            available_scenario_ids.iter().copied().collect();

        match &self.scenario_selection {
            ScaleConsistencyBatchScenarioSelection::All => {}
            ScaleConsistencyBatchScenarioSelection::FailedFromResume => {
                if self.resume_settings.is_none() {
                    return Err(
                        ScaleConsistencyBatchRunRequestError::FailedSelectionRequiresResumeSettings,
                    );
                }
            }
            ScaleConsistencyBatchScenarioSelection::ScenarioIds {
                scenario_ids: selected_scenario_ids,
            } => {
                if selected_scenario_ids.is_empty() {
                    return Err(ScaleConsistencyBatchRunRequestError::ScenarioSelectionIsEmpty);
                }

                let mut observed_ids: BTreeSet<&str> = BTreeSet::new();
                for scenario_id in selected_scenario_ids {
                    let trimmed_id = scenario_id.trim();
                    if trimmed_id.is_empty() {
                        return Err(
                            ScaleConsistencyBatchRunRequestError::ScenarioSelectionContainsEmptyId,
                        );
                    }
                    if !observed_ids.insert(trimmed_id) {
                        return Err(
                            ScaleConsistencyBatchRunRequestError::DuplicateSelectedScenarioId {
                                scenario_id: trimmed_id.to_string(),
                            },
                        );
                    }
                    if !available_scenario_id_set.contains(trimmed_id) {
                        return Err(
                            ScaleConsistencyBatchRunRequestError::UnknownSelectedScenarioId {
                                scenario_id: trimmed_id.to_string(),
                            },
                        );
                    }
                }
            }
        }

        if let Some(resume_settings) = &self.resume_settings {
            match &resume_settings.source {
                ScaleConsistencyBatchResumeSource::SameProcess => {
                    if execution_mode != CalibrationExecutionMode::Memory {
                        return Err(
                            ScaleConsistencyBatchRunRequestError::SameProcessResumeRequiresMemoryMode {
                                execution_mode,
                            },
                        );
                    }
                }
                ScaleConsistencyBatchResumeSource::PersistedCheckpointReference {
                    checkpoint_reference,
                } => {
                    if execution_mode != CalibrationExecutionMode::Disk {
                        return Err(
                            ScaleConsistencyBatchRunRequestError::PersistedResumeRequiresDiskMode {
                                execution_mode,
                            },
                        );
                    }
                    if checkpoint_reference.trim().is_empty() {
                        return Err(ScaleConsistencyBatchRunRequestError::EmptyCheckpointReference);
                    }
                }
            }
        }

        Ok(())
    }

    pub const fn schema_version() -> &'static str {
        SCALE_CONSISTENCY_BATCH_RUN_REQUEST_SCHEMA_VERSION
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScaleConsistencyBatchRunRequestError {
    InvalidSchemaVersion {
        expected: String,
        actual: String,
    },
    FailedSelectionRequiresResumeSettings,
    ScenarioSelectionIsEmpty,
    ScenarioSelectionContainsEmptyId,
    DuplicateSelectedScenarioId {
        scenario_id: String,
    },
    UnknownSelectedScenarioId {
        scenario_id: String,
    },
    SameProcessResumeRequiresMemoryMode {
        execution_mode: CalibrationExecutionMode,
    },
    PersistedResumeRequiresDiskMode {
        execution_mode: CalibrationExecutionMode,
    },
    EmptyCheckpointReference,
}

impl Display for ScaleConsistencyBatchRunRequestError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSchemaVersion { expected, actual } => write!(
                formatter,
                "schema_version must be `{expected}`, found `{actual}`"
            ),
            Self::FailedSelectionRequiresResumeSettings => write!(
                formatter,
                "scenario_selection=`failed_from_resume` requires resume_settings"
            ),
            Self::ScenarioSelectionIsEmpty => write!(
                formatter,
                "scenario_selection.scenario_ids must contain at least one scenario_id"
            ),
            Self::ScenarioSelectionContainsEmptyId => write!(
                formatter,
                "scenario_selection.scenario_ids must not contain empty scenario_id values"
            ),
            Self::DuplicateSelectedScenarioId { scenario_id } => write!(
                formatter,
                "scenario_selection.scenario_ids contains duplicate scenario_id `{scenario_id}`"
            ),
            Self::UnknownSelectedScenarioId { scenario_id } => write!(
                formatter,
                "scenario_selection references unknown scenario_id `{scenario_id}`"
            ),
            Self::SameProcessResumeRequiresMemoryMode { execution_mode } => write!(
                formatter,
                "resume_settings.source=`same_process` requires execution_mode=`memory`, found `{execution_mode:?}`"
            ),
            Self::PersistedResumeRequiresDiskMode { execution_mode } => write!(
                formatter,
                "resume_settings.source=`persisted_checkpoint_reference` requires execution_mode=`disk`, found `{execution_mode:?}`"
            ),
            Self::EmptyCheckpointReference => write!(
                formatter,
                "resume_settings.checkpoint_reference must be a non-empty string"
            ),
        }
    }
}

impl Error for ScaleConsistencyBatchRunRequestError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleConsistencyBatchScenarioStatus {
    Pass,
    Fail,
    ExecutionError,
}

impl ScaleConsistencyBatchScenarioStatus {
    const fn from_regression_status(status: RegressionCaseStatus) -> Self {
        match status {
            RegressionCaseStatus::Pass => Self::Pass,
            RegressionCaseStatus::Fail => Self::Fail,
        }
    }

    const fn is_success(self) -> bool {
        matches!(self, Self::Pass)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleConsistencyBatchScenarioRecordSource {
    ExecutedInCurrentRun,
    LoadedFromResume,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleConsistencyBatchStatus {
    Pass,
    Fail,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyBatchScenarioSummaryMetrics {
    pub total_metric_count: usize,
    pub computed_metric_count: usize,
    pub missing_metric_count: usize,
    pub failed_metric_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_absolute_delta: Option<f64>,
}

impl ScaleConsistencyBatchScenarioSummaryMetrics {
    pub fn validate(&self) -> Result<(), ScaleConsistencyBatchScenarioSummaryMetricsError> {
        if self.computed_metric_count > self.total_metric_count {
            return Err(
                ScaleConsistencyBatchScenarioSummaryMetricsError::ComputedMetricCountExceedsTotal {
                    total_metric_count: self.total_metric_count,
                    computed_metric_count: self.computed_metric_count,
                },
            );
        }
        if self.missing_metric_count > self.total_metric_count {
            return Err(
                ScaleConsistencyBatchScenarioSummaryMetricsError::MissingMetricCountExceedsTotal {
                    total_metric_count: self.total_metric_count,
                    missing_metric_count: self.missing_metric_count,
                },
            );
        }
        if self.failed_metric_count > self.total_metric_count {
            return Err(
                ScaleConsistencyBatchScenarioSummaryMetricsError::FailedMetricCountExceedsTotal {
                    total_metric_count: self.total_metric_count,
                    failed_metric_count: self.failed_metric_count,
                },
            );
        }
        if self.computed_metric_count + self.missing_metric_count > self.total_metric_count {
            return Err(
                ScaleConsistencyBatchScenarioSummaryMetricsError::ComputedAndMissingCountsExceedTotal {
                    total_metric_count: self.total_metric_count,
                    computed_metric_count: self.computed_metric_count,
                    missing_metric_count: self.missing_metric_count,
                },
            );
        }
        if let Some(max_absolute_delta) = self.max_absolute_delta
            && (!max_absolute_delta.is_finite() || max_absolute_delta < 0.0)
        {
            return Err(
                ScaleConsistencyBatchScenarioSummaryMetricsError::InvalidMaxAbsoluteDelta {
                    value: max_absolute_delta,
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScaleConsistencyBatchScenarioSummaryMetricsError {
    ComputedMetricCountExceedsTotal {
        total_metric_count: usize,
        computed_metric_count: usize,
    },
    MissingMetricCountExceedsTotal {
        total_metric_count: usize,
        missing_metric_count: usize,
    },
    FailedMetricCountExceedsTotal {
        total_metric_count: usize,
        failed_metric_count: usize,
    },
    ComputedAndMissingCountsExceedTotal {
        total_metric_count: usize,
        computed_metric_count: usize,
        missing_metric_count: usize,
    },
    InvalidMaxAbsoluteDelta {
        value: f64,
    },
}

impl Display for ScaleConsistencyBatchScenarioSummaryMetricsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ComputedMetricCountExceedsTotal {
                total_metric_count,
                computed_metric_count,
            } => write!(
                formatter,
                "computed_metric_count ({computed_metric_count}) must not exceed total_metric_count ({total_metric_count})"
            ),
            Self::MissingMetricCountExceedsTotal {
                total_metric_count,
                missing_metric_count,
            } => write!(
                formatter,
                "missing_metric_count ({missing_metric_count}) must not exceed total_metric_count ({total_metric_count})"
            ),
            Self::FailedMetricCountExceedsTotal {
                total_metric_count,
                failed_metric_count,
            } => write!(
                formatter,
                "failed_metric_count ({failed_metric_count}) must not exceed total_metric_count ({total_metric_count})"
            ),
            Self::ComputedAndMissingCountsExceedTotal {
                total_metric_count,
                computed_metric_count,
                missing_metric_count,
            } => write!(
                formatter,
                "computed_metric_count ({computed_metric_count}) plus missing_metric_count ({missing_metric_count}) must not exceed total_metric_count ({total_metric_count})"
            ),
            Self::InvalidMaxAbsoluteDelta { value } => write!(
                formatter,
                "max_absolute_delta must be finite and non-negative when provided, found {value}"
            ),
        }
    }
}

impl Error for ScaleConsistencyBatchScenarioSummaryMetricsError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyScenarioExecutionOutcome {
    pub status: RegressionCaseStatus,
    pub summary_metrics: ScaleConsistencyBatchScenarioSummaryMetrics,
}

impl ScaleConsistencyScenarioExecutionOutcome {
    pub fn validate(&self) -> Result<(), ScaleConsistencyBatchScenarioSummaryMetricsError> {
        self.summary_metrics.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyBatchScenarioExecutionMetadata {
    pub scenario_position: usize,
    pub execution_mode: CalibrationExecutionMode,
    pub record_source: ScaleConsistencyBatchScenarioRecordSource,
    pub random_control_metadata: ScaleConsistencyRandomControlMetadata,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyBatchScenarioReport {
    pub scenario_id: String,
    pub parameter_set_id: String,
    pub coarse_interval_delta: f64,
    pub fine_interval_delta: f64,
    pub horizon_interval_count: usize,
    pub status: ScaleConsistencyBatchScenarioStatus,
    pub summary_metrics: ScaleConsistencyBatchScenarioSummaryMetrics,
    pub execution_metadata: ScaleConsistencyBatchScenarioExecutionMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyBatchRunSummary {
    pub total_scenario_count: usize,
    pub executed_scenario_count: usize,
    pub resumed_scenario_count: usize,
    pub passed_scenario_count: usize,
    pub failed_scenario_count: usize,
    pub execution_error_scenario_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleConsistencyBatchResumeStatus {
    FreshRun,
    SameProcessResumed,
    PersistedCheckpointResumed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyBatchArtifactExportReceipt {
    pub checkpoint_reference: String,
    pub checkpoint_absolute_path: String,
    pub report_absolute_path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyBatchRunReport {
    pub schema_version: String,
    pub ordering_policy: String,
    pub resume_policy: String,
    pub artifact_persistence_policy: String,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub status: ScaleConsistencyBatchStatus,
    pub resume_status: ScaleConsistencyBatchResumeStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resumed_from_checkpoint_reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_export: Option<ScaleConsistencyBatchArtifactExportReceipt>,
    pub summary: ScaleConsistencyBatchRunSummary,
    pub scenario_reports: Vec<ScaleConsistencyBatchScenarioReport>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScaleConsistencyBatchCheckpoint {
    schema_version: String,
    checkpoint_reference_policy: String,
    run_contract_schema_version: String,
    run_spec_schema_version: String,
    run_id: String,
    execution_mode: CalibrationExecutionMode,
    scenario_id_order: Vec<String>,
    scenario_reports: Vec<ScaleConsistencyBatchScenarioReport>,
}

impl ScaleConsistencyBatchCheckpoint {
    fn from_run_contract_and_reports(
        run_contract: &ScaleConsistencyPairedRunContract,
        scenario_reports: Vec<ScaleConsistencyBatchScenarioReport>,
    ) -> Self {
        Self {
            schema_version: SCALE_CONSISTENCY_BATCH_CHECKPOINT_SCHEMA_VERSION.to_string(),
            checkpoint_reference_policy: SCALE_CONSISTENCY_BATCH_CHECKPOINT_REFERENCE_POLICY
                .to_string(),
            run_contract_schema_version: SCALE_CONSISTENCY_PAIRED_RUN_CONTRACT_SCHEMA_VERSION
                .to_string(),
            run_spec_schema_version: SCALE_CONSISTENCY_PAIRED_RUN_SPEC_SCHEMA_VERSION.to_string(),
            run_id: run_contract.run_spec.run_id.clone(),
            execution_mode: run_contract.run_spec.execution_mode,
            scenario_id_order: run_contract
                .run_spec
                .scenarios
                .iter()
                .map(|scenario| scenario.scenario_id.clone())
                .collect(),
            scenario_reports,
        }
    }

    fn validate_compatibility(
        &self,
        run_contract: &ScaleConsistencyPairedRunContract,
    ) -> Result<(), ScaleConsistencyBatchCheckpointCompatibilityError> {
        if self.schema_version != SCALE_CONSISTENCY_BATCH_CHECKPOINT_SCHEMA_VERSION {
            return Err(
                ScaleConsistencyBatchCheckpointCompatibilityError::UnsupportedSchemaVersion {
                    expected: SCALE_CONSISTENCY_BATCH_CHECKPOINT_SCHEMA_VERSION.to_string(),
                    found: self.schema_version.clone(),
                },
            );
        }
        if self.checkpoint_reference_policy != SCALE_CONSISTENCY_BATCH_CHECKPOINT_REFERENCE_POLICY {
            return Err(
                ScaleConsistencyBatchCheckpointCompatibilityError::UnsupportedReferencePolicy {
                    expected: SCALE_CONSISTENCY_BATCH_CHECKPOINT_REFERENCE_POLICY.to_string(),
                    found: self.checkpoint_reference_policy.clone(),
                },
            );
        }
        if self.run_contract_schema_version != run_contract.schema_version {
            return Err(
                ScaleConsistencyBatchCheckpointCompatibilityError::RunContractSchemaVersionMismatch {
                    expected: run_contract.schema_version.clone(),
                    found: self.run_contract_schema_version.clone(),
                },
            );
        }
        if self.run_spec_schema_version != run_contract.run_spec_schema_version {
            return Err(
                ScaleConsistencyBatchCheckpointCompatibilityError::RunSpecSchemaVersionMismatch {
                    expected: run_contract.run_spec_schema_version.clone(),
                    found: self.run_spec_schema_version.clone(),
                },
            );
        }
        if self.run_id != run_contract.run_spec.run_id {
            return Err(
                ScaleConsistencyBatchCheckpointCompatibilityError::RunIdMismatch {
                    expected: run_contract.run_spec.run_id.clone(),
                    found: self.run_id.clone(),
                },
            );
        }
        if self.execution_mode != run_contract.run_spec.execution_mode {
            return Err(
                ScaleConsistencyBatchCheckpointCompatibilityError::ExecutionModeMismatch {
                    expected: run_contract.run_spec.execution_mode,
                    found: self.execution_mode,
                },
            );
        }

        let expected_order = run_contract
            .run_spec
            .scenarios
            .iter()
            .map(|scenario| scenario.scenario_id.clone())
            .collect::<Vec<_>>();
        if self.scenario_id_order != expected_order {
            return Err(
                ScaleConsistencyBatchCheckpointCompatibilityError::ScenarioOrderMismatch {
                    expected: expected_order,
                    found: self.scenario_id_order.clone(),
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScaleConsistencyBatchCheckpointCompatibilityError {
    UnsupportedSchemaVersion {
        expected: String,
        found: String,
    },
    UnsupportedReferencePolicy {
        expected: String,
        found: String,
    },
    RunContractSchemaVersionMismatch {
        expected: String,
        found: String,
    },
    RunSpecSchemaVersionMismatch {
        expected: String,
        found: String,
    },
    RunIdMismatch {
        expected: String,
        found: String,
    },
    ExecutionModeMismatch {
        expected: CalibrationExecutionMode,
        found: CalibrationExecutionMode,
    },
    ScenarioOrderMismatch {
        expected: Vec<String>,
        found: Vec<String>,
    },
}

impl Display for ScaleConsistencyBatchCheckpointCompatibilityError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion { expected, found } => write!(
                formatter,
                "checkpoint schema_version is incompatible, expected `{expected}`, found `{found}`"
            ),
            Self::UnsupportedReferencePolicy { expected, found } => write!(
                formatter,
                "checkpoint checkpoint_reference_policy is incompatible, expected `{expected}`, found `{found}`"
            ),
            Self::RunContractSchemaVersionMismatch { expected, found } => write!(
                formatter,
                "checkpoint run_contract_schema_version is incompatible, expected `{expected}`, found `{found}`"
            ),
            Self::RunSpecSchemaVersionMismatch { expected, found } => write!(
                formatter,
                "checkpoint run_spec_schema_version is incompatible, expected `{expected}`, found `{found}`"
            ),
            Self::RunIdMismatch { expected, found } => write!(
                formatter,
                "checkpoint run_id is incompatible, expected `{expected}`, found `{found}`"
            ),
            Self::ExecutionModeMismatch { expected, found } => write!(
                formatter,
                "checkpoint execution_mode is incompatible, expected `{expected:?}`, found `{found:?}`"
            ),
            Self::ScenarioOrderMismatch { expected, found } => write!(
                formatter,
                "checkpoint scenario_id_order is incompatible, expected {:?}, found {:?}",
                expected, found
            ),
        }
    }
}

impl Error for ScaleConsistencyBatchCheckpointCompatibilityError {}

#[derive(Debug, Clone, Default)]
pub struct ScaleConsistencyBatchMemoryCheckpointStore {
    checkpoint_by_run_id: BTreeMap<String, ScaleConsistencyBatchCheckpoint>,
}

impl ScaleConsistencyBatchMemoryCheckpointStore {
    pub const fn new() -> Self {
        Self {
            checkpoint_by_run_id: BTreeMap::new(),
        }
    }

    fn load_checkpoint(&self, run_id: &str) -> Option<ScaleConsistencyBatchCheckpoint> {
        self.checkpoint_by_run_id.get(run_id).cloned()
    }

    fn record_checkpoint(&mut self, checkpoint: ScaleConsistencyBatchCheckpoint) {
        self.checkpoint_by_run_id
            .insert(checkpoint.run_id.clone(), checkpoint);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyScenarioExecutionRequest {
    pub scenario_position: usize,
    pub random_control_metadata: ScaleConsistencyRandomControlMetadata,
}

pub trait ScaleConsistencyScenarioExecutor {
    type Error: Display;

    fn execute_scenario(
        &self,
        scenario: &ScaleConsistencyPairedExperimentScenarioSpec,
        request: &ScaleConsistencyScenarioExecutionRequest,
    ) -> Result<ScaleConsistencyScenarioExecutionOutcome, Self::Error>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScaleConsistencyBatchOrchestratorError {
    InvalidRunRequest {
        source: ScaleConsistencyBatchRunRequestError,
    },
    MissingMemoryCheckpointStore,
    MissingResumeCheckpointForRun {
        run_id: String,
    },
    CheckpointLoadFailed {
        checkpoint_reference: String,
        error_message: String,
    },
    CheckpointCompatibilityError {
        checkpoint_reference: String,
        source: ScaleConsistencyBatchCheckpointCompatibilityError,
    },
    ScenarioExecutionProducedInvalidMetrics {
        scenario_id: String,
        source: ScaleConsistencyBatchScenarioSummaryMetricsError,
    },
    ArtifactPersistenceRequiresDiskMode {
        execution_mode: CalibrationExecutionMode,
    },
    ArtifactPersistenceFailed {
        export_directory: String,
        error_message: String,
    },
}

impl Display for ScaleConsistencyBatchOrchestratorError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRunRequest { source } => {
                write!(formatter, "batch run request is invalid: {source}")
            }
            Self::MissingMemoryCheckpointStore => {
                write!(
                    formatter,
                    "same-process batch resume requires a memory checkpoint store"
                )
            }
            Self::MissingResumeCheckpointForRun { run_id } => write!(
                formatter,
                "no existing batch checkpoint is available for run_id `{run_id}`"
            ),
            Self::CheckpointLoadFailed {
                checkpoint_reference,
                error_message,
            } => write!(
                formatter,
                "failed to load checkpoint `{checkpoint_reference}`: {error_message}"
            ),
            Self::CheckpointCompatibilityError {
                checkpoint_reference,
                source,
            } => write!(
                formatter,
                "checkpoint `{checkpoint_reference}` is incompatible with the requested run: {source}"
            ),
            Self::ScenarioExecutionProducedInvalidMetrics {
                scenario_id,
                source,
            } => write!(
                formatter,
                "scenario `{scenario_id}` produced invalid summary metrics: {source}"
            ),
            Self::ArtifactPersistenceRequiresDiskMode { execution_mode } => write!(
                formatter,
                "artifact persistence requires execution_mode=`disk`, found `{execution_mode:?}`"
            ),
            Self::ArtifactPersistenceFailed {
                export_directory,
                error_message,
            } => write!(
                formatter,
                "failed to persist batch artifacts in `{export_directory}`: {error_message}"
            ),
        }
    }
}

impl Error for ScaleConsistencyBatchOrchestratorError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRunRequest { source } => Some(source),
            Self::CheckpointCompatibilityError { source, .. } => Some(source),
            Self::ScenarioExecutionProducedInvalidMetrics { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub fn run_scale_consistency_batch_orchestrator<E>(
    request: &ScaleConsistencyBatchRunRequest,
    executor: &E,
    mut memory_checkpoint_store: Option<&mut ScaleConsistencyBatchMemoryCheckpointStore>,
) -> Result<ScaleConsistencyBatchRunReport, ScaleConsistencyBatchOrchestratorError>
where
    E: ScaleConsistencyScenarioExecutor,
{
    request
        .validate()
        .map_err(|source| ScaleConsistencyBatchOrchestratorError::InvalidRunRequest { source })?;

    let run_contract = &request.run_contract;
    let run_spec = &run_contract.run_spec;
    let execution_mode = run_spec.execution_mode;

    let (resume_status, resumed_from_checkpoint_reference, resume_checkpoint) = match &request
        .resume_settings
    {
        None => (ScaleConsistencyBatchResumeStatus::FreshRun, None, None),
        Some(resume_settings) => match &resume_settings.source {
            ScaleConsistencyBatchResumeSource::SameProcess => {
                let Some(checkpoint_store) = memory_checkpoint_store.as_deref_mut() else {
                    return Err(
                        ScaleConsistencyBatchOrchestratorError::MissingMemoryCheckpointStore,
                    );
                };

                let checkpoint = checkpoint_store
                    .load_checkpoint(run_spec.run_id.as_str())
                    .ok_or_else(|| {
                        ScaleConsistencyBatchOrchestratorError::MissingResumeCheckpointForRun {
                            run_id: run_spec.run_id.clone(),
                        }
                    })?;

                checkpoint
                    .validate_compatibility(run_contract)
                    .map_err(|source| {
                        ScaleConsistencyBatchOrchestratorError::CheckpointCompatibilityError {
                            checkpoint_reference: format!("memory://{}", run_spec.run_id),
                            source,
                        }
                    })?;

                (
                    ScaleConsistencyBatchResumeStatus::SameProcessResumed,
                    Some(format!("memory://{}", run_spec.run_id)),
                    Some(checkpoint),
                )
            }
            ScaleConsistencyBatchResumeSource::PersistedCheckpointReference {
                checkpoint_reference,
            } => {
                let checkpoint = load_batch_checkpoint_from_disk(Path::new(checkpoint_reference))
                    .map_err(|error_message| {
                    ScaleConsistencyBatchOrchestratorError::CheckpointLoadFailed {
                        checkpoint_reference: checkpoint_reference.clone(),
                        error_message,
                    }
                })?;

                checkpoint
                    .validate_compatibility(run_contract)
                    .map_err(|source| {
                        ScaleConsistencyBatchOrchestratorError::CheckpointCompatibilityError {
                            checkpoint_reference: checkpoint_reference.clone(),
                            source,
                        }
                    })?;

                (
                    ScaleConsistencyBatchResumeStatus::PersistedCheckpointResumed,
                    Some(checkpoint_reference.clone()),
                    Some(checkpoint),
                )
            }
        },
    };

    let mut report_by_scenario_id = resume_checkpoint
        .as_ref()
        .map(|checkpoint| {
            checkpoint
                .scenario_reports
                .iter()
                .cloned()
                .map(|report| (report.scenario_id.clone(), report))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let scenario_ids_to_execute = resolve_scenario_ids_to_execute(request, &report_by_scenario_id);

    for (scenario_position, scenario) in run_spec.scenarios.iter().enumerate() {
        if !scenario_ids_to_execute.contains(scenario.scenario_id.as_str()) {
            continue;
        }

        let random_control_metadata =
            scenario.resolve_random_control_metadata(scenario_position as u64);
        let execution_request = ScaleConsistencyScenarioExecutionRequest {
            scenario_position,
            random_control_metadata: random_control_metadata.clone(),
        };

        let scenario_report = match executor.execute_scenario(scenario, &execution_request) {
            Ok(outcome) => {
                outcome.validate().map_err(|source| {
                    ScaleConsistencyBatchOrchestratorError::ScenarioExecutionProducedInvalidMetrics {
                        scenario_id: scenario.scenario_id.clone(),
                        source,
                    }
                })?;

                ScaleConsistencyBatchScenarioReport {
                    scenario_id: scenario.scenario_id.clone(),
                    parameter_set_id: scenario.parameter_set.parameter_set_id.clone(),
                    coarse_interval_delta: scenario.coarse_interval_delta,
                    fine_interval_delta: scenario.fine_interval_delta,
                    horizon_interval_count: scenario.horizon_interval_count,
                    status: ScaleConsistencyBatchScenarioStatus::from_regression_status(
                        outcome.status,
                    ),
                    summary_metrics: outcome.summary_metrics,
                    execution_metadata: ScaleConsistencyBatchScenarioExecutionMetadata {
                        scenario_position,
                        execution_mode,
                        record_source:
                            ScaleConsistencyBatchScenarioRecordSource::ExecutedInCurrentRun,
                        random_control_metadata,
                    },
                    error_message: None,
                }
            }
            Err(error) => ScaleConsistencyBatchScenarioReport {
                scenario_id: scenario.scenario_id.clone(),
                parameter_set_id: scenario.parameter_set.parameter_set_id.clone(),
                coarse_interval_delta: scenario.coarse_interval_delta,
                fine_interval_delta: scenario.fine_interval_delta,
                horizon_interval_count: scenario.horizon_interval_count,
                status: ScaleConsistencyBatchScenarioStatus::ExecutionError,
                summary_metrics: ScaleConsistencyBatchScenarioSummaryMetrics {
                    total_metric_count: 0,
                    computed_metric_count: 0,
                    missing_metric_count: 0,
                    failed_metric_count: 0,
                    max_absolute_delta: None,
                },
                execution_metadata: ScaleConsistencyBatchScenarioExecutionMetadata {
                    scenario_position,
                    execution_mode,
                    record_source: ScaleConsistencyBatchScenarioRecordSource::ExecutedInCurrentRun,
                    random_control_metadata,
                },
                error_message: Some(error.to_string()),
            },
        };

        report_by_scenario_id.insert(scenario.scenario_id.clone(), scenario_report);

        if execution_mode == CalibrationExecutionMode::Disk && run_spec.artifact_output.is_some() {
            persist_checkpoint_from_reports(run_contract, &report_by_scenario_id)?;
        }
    }

    let selected_ids_for_report =
        resolve_scenario_ids_to_include_in_report(request, &report_by_scenario_id);
    let mut scenario_reports = Vec::with_capacity(selected_ids_for_report.len());

    for (scenario_position, scenario) in run_spec.scenarios.iter().enumerate() {
        if !selected_ids_for_report.contains(scenario.scenario_id.as_str()) {
            continue;
        }

        let Some(mut scenario_report) = report_by_scenario_id.remove(scenario.scenario_id.as_str())
        else {
            continue;
        };

        if !scenario_ids_to_execute.contains(scenario.scenario_id.as_str()) {
            scenario_report.execution_metadata.record_source =
                ScaleConsistencyBatchScenarioRecordSource::LoadedFromResume;
            scenario_report.execution_metadata.scenario_position = scenario_position;
        }

        scenario_reports.push(scenario_report);
    }

    let summary = build_batch_summary(&scenario_reports);
    let status =
        if summary.failed_scenario_count == 0 && summary.execution_error_scenario_count == 0 {
            ScaleConsistencyBatchStatus::Pass
        } else {
            ScaleConsistencyBatchStatus::Fail
        };

    let mut report = ScaleConsistencyBatchRunReport {
        schema_version: SCALE_CONSISTENCY_BATCH_RUN_REPORT_SCHEMA_VERSION.to_string(),
        ordering_policy: SCALE_CONSISTENCY_BATCH_ORDERING_POLICY.to_string(),
        resume_policy: SCALE_CONSISTENCY_BATCH_RESUME_POLICY.to_string(),
        artifact_persistence_policy: SCALE_CONSISTENCY_BATCH_ARTIFACT_PERSISTENCE_POLICY
            .to_string(),
        run_id: run_spec.run_id.clone(),
        execution_mode,
        status,
        resume_status,
        resumed_from_checkpoint_reference,
        artifact_export: None,
        summary,
        scenario_reports,
    };

    let checkpoint = ScaleConsistencyBatchCheckpoint::from_run_contract_and_reports(
        run_contract,
        report.scenario_reports.clone(),
    );
    if let Some(checkpoint_store) = memory_checkpoint_store.as_deref_mut() {
        checkpoint_store.record_checkpoint(checkpoint.clone());
    }

    if execution_mode == CalibrationExecutionMode::Disk && run_spec.artifact_output.is_some() {
        report.artifact_export = Some(persist_report_and_checkpoint(
            run_contract,
            &report,
            &checkpoint,
        )?);
    }

    Ok(report)
}

fn resolve_scenario_ids_to_execute(
    request: &ScaleConsistencyBatchRunRequest,
    existing_reports: &BTreeMap<String, ScaleConsistencyBatchScenarioReport>,
) -> BTreeSet<String> {
    match &request.scenario_selection {
        ScaleConsistencyBatchScenarioSelection::All => request
            .run_contract
            .run_spec
            .scenarios
            .iter()
            .map(|scenario| scenario.scenario_id.clone())
            .collect(),
        ScaleConsistencyBatchScenarioSelection::FailedFromResume => existing_reports
            .iter()
            .filter(|(_, report)| !report.status.is_success())
            .map(|(scenario_id, _)| scenario_id.clone())
            .collect(),
        ScaleConsistencyBatchScenarioSelection::ScenarioIds { scenario_ids } => {
            scenario_ids.iter().cloned().collect()
        }
    }
}

fn resolve_scenario_ids_to_include_in_report(
    request: &ScaleConsistencyBatchRunRequest,
    existing_reports: &BTreeMap<String, ScaleConsistencyBatchScenarioReport>,
) -> BTreeSet<String> {
    if request.resume_settings.is_some() {
        return existing_reports.keys().cloned().collect();
    }

    resolve_scenario_ids_to_execute(request, existing_reports)
}

fn build_batch_summary(
    scenario_reports: &[ScaleConsistencyBatchScenarioReport],
) -> ScaleConsistencyBatchRunSummary {
    let total_scenario_count = scenario_reports.len();
    let executed_scenario_count = scenario_reports
        .iter()
        .filter(|report| {
            report.execution_metadata.record_source
                == ScaleConsistencyBatchScenarioRecordSource::ExecutedInCurrentRun
        })
        .count();
    let resumed_scenario_count = total_scenario_count - executed_scenario_count;
    let passed_scenario_count = scenario_reports
        .iter()
        .filter(|report| report.status == ScaleConsistencyBatchScenarioStatus::Pass)
        .count();
    let failed_scenario_count = scenario_reports
        .iter()
        .filter(|report| report.status == ScaleConsistencyBatchScenarioStatus::Fail)
        .count();
    let execution_error_scenario_count = scenario_reports
        .iter()
        .filter(|report| report.status == ScaleConsistencyBatchScenarioStatus::ExecutionError)
        .count();

    ScaleConsistencyBatchRunSummary {
        total_scenario_count,
        executed_scenario_count,
        resumed_scenario_count,
        passed_scenario_count,
        failed_scenario_count,
        execution_error_scenario_count,
    }
}

fn persist_checkpoint_from_reports(
    run_contract: &ScaleConsistencyPairedRunContract,
    report_by_scenario_id: &BTreeMap<String, ScaleConsistencyBatchScenarioReport>,
) -> Result<(), ScaleConsistencyBatchOrchestratorError> {
    let checkpoint = ScaleConsistencyBatchCheckpoint::from_run_contract_and_reports(
        run_contract,
        run_contract
            .run_spec
            .scenarios
            .iter()
            .filter_map(|scenario| {
                report_by_scenario_id
                    .get(scenario.scenario_id.as_str())
                    .cloned()
            })
            .collect(),
    );
    let _ = persist_checkpoint(run_contract, &checkpoint)?;

    Ok(())
}

fn persist_report_and_checkpoint(
    run_contract: &ScaleConsistencyPairedRunContract,
    report: &ScaleConsistencyBatchRunReport,
    checkpoint: &ScaleConsistencyBatchCheckpoint,
) -> Result<ScaleConsistencyBatchArtifactExportReceipt, ScaleConsistencyBatchOrchestratorError> {
    let checkpoint_path = persist_checkpoint(run_contract, checkpoint)?;
    let report_path = batch_report_path(run_contract)?;

    persist_json_artifact(run_contract, report_path.as_path(), report)?;

    Ok(ScaleConsistencyBatchArtifactExportReceipt {
        checkpoint_reference: checkpoint_path.to_string_lossy().into_owned(),
        checkpoint_absolute_path: checkpoint_path.to_string_lossy().into_owned(),
        report_absolute_path: report_path.to_string_lossy().into_owned(),
    })
}

fn persist_checkpoint(
    run_contract: &ScaleConsistencyPairedRunContract,
    checkpoint: &ScaleConsistencyBatchCheckpoint,
) -> Result<PathBuf, ScaleConsistencyBatchOrchestratorError> {
    if run_contract.run_spec.execution_mode != CalibrationExecutionMode::Disk {
        return Err(
            ScaleConsistencyBatchOrchestratorError::ArtifactPersistenceRequiresDiskMode {
                execution_mode: run_contract.run_spec.execution_mode,
            },
        );
    }

    let checkpoint_path = batch_checkpoint_path(run_contract)?;
    persist_json_artifact(run_contract, checkpoint_path.as_path(), checkpoint)?;

    Ok(checkpoint_path)
}

fn persist_json_artifact<T>(
    run_contract: &ScaleConsistencyPairedRunContract,
    path: &Path,
    value: &T,
) -> Result<(), ScaleConsistencyBatchOrchestratorError>
where
    T: Serialize,
{
    write_json_file(path, value).map_err(|error_message| {
        ScaleConsistencyBatchOrchestratorError::ArtifactPersistenceFailed {
            export_directory: run_contract
                .run_spec
                .artifact_output
                .as_ref()
                .expect("disk-mode persistence requires configured artifact output")
                .artifact_root_uri
                .clone(),
            error_message,
        }
    })?;

    Ok(())
}

fn batch_checkpoint_path(
    run_contract: &ScaleConsistencyPairedRunContract,
) -> Result<PathBuf, ScaleConsistencyBatchOrchestratorError> {
    let artifact_output = run_contract.run_spec.artifact_output.as_ref().ok_or(
        ScaleConsistencyBatchOrchestratorError::ArtifactPersistenceFailed {
            export_directory: String::new(),
            error_message: "artifact_output is not configured".to_string(),
        },
    )?;

    Ok(Path::new(&artifact_output.artifact_root_uri).join(format!(
        "{}.batch_checkpoint.json",
        artifact_output.file_stem
    )))
}

fn batch_report_path(
    run_contract: &ScaleConsistencyPairedRunContract,
) -> Result<PathBuf, ScaleConsistencyBatchOrchestratorError> {
    let artifact_output = run_contract.run_spec.artifact_output.as_ref().ok_or(
        ScaleConsistencyBatchOrchestratorError::ArtifactPersistenceFailed {
            export_directory: String::new(),
            error_message: "artifact_output is not configured".to_string(),
        },
    )?;

    Ok(Path::new(&artifact_output.artifact_root_uri)
        .join(format!("{}.batch_report.json", artifact_output.file_stem)))
}

fn write_json_file<T>(path: &Path, value: &T) -> Result<(), String>
where
    T: Serialize,
{
    let Some(parent_directory) = path.parent() else {
        return Err(format!(
            "path `{}` does not have a parent directory",
            path.display()
        ));
    };

    fs::create_dir_all(parent_directory).map_err(|error| {
        format!(
            "failed to create directory `{}`: {error}",
            parent_directory.display()
        )
    })?;

    let serialized = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("failed to serialize JSON payload: {error}"))?;

    fs::write(path, serialized)
        .map_err(|error| format!("failed to write `{}`: {error}", path.display()))?;

    Ok(())
}

fn load_batch_checkpoint_from_disk(path: &Path) -> Result<ScaleConsistencyBatchCheckpoint, String> {
    let serialized =
        fs::read(path).map_err(|error| format!("failed to read `{}`: {error}", path.display()))?;
    serde_json::from_slice(&serialized).map_err(|error| {
        format!(
            "failed to parse checkpoint JSON `{}`: {error}",
            path.display()
        )
    })
}
