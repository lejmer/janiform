use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

/// Stable schema version for JSONL run events.
pub const RUN_EVENT_SCHEMA_VERSION: &str = "janiform.run_event.v1";

/// Stable schema version for latest-state snapshots.
pub const RUN_STATE_SNAPSHOT_SCHEMA_VERSION: &str = "janiform.run_state_snapshot.v1";

/// Default JSONL event-log file name inside a run directory.
pub const EVENT_LOG_FILE_NAME: &str = "events.jsonl";

/// Default latest-state snapshot file name inside a run directory.
pub const RUN_STATE_SNAPSHOT_FILE_NAME: &str = "latest-state.json";

/// Named delivery policy for bounded event channels.
pub const EVENT_CHANNEL_DELIVERY_POLICY_ID: &str =
    "janiform.event_channel.delivery.required-or-droppable-v1";

/// Stable workflow identifier plus an optional display label.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkflowRef {
    pub workflow_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl WorkflowRef {
    /// Build a workflow reference with no display label.
    pub fn new(workflow_id: impl Into<String>) -> Self {
        Self {
            workflow_id: workflow_id.into(),
            label: None,
        }
    }

    /// Build a workflow reference with a stable identifier and display label.
    pub fn labeled(workflow_id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            workflow_id: workflow_id.into(),
            label: Some(label.into()),
        }
    }
}

/// Stable stage identifier plus an optional display label.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RunStageRef {
    pub stage_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl RunStageRef {
    /// Build a stage reference with no display label.
    pub fn new(stage_id: impl Into<String>) -> Self {
        Self {
            stage_id: stage_id.into(),
            label: None,
        }
    }

    /// Build a stage reference with a stable identifier and display label.
    pub fn labeled(stage_id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            stage_id: stage_id.into(),
            label: Some(label.into()),
        }
    }
}

/// Stable phase or subphase identifier plus an optional display label.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RunPhaseRef {
    pub phase_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl RunPhaseRef {
    /// Build a phase reference with no display label.
    pub fn new(phase_id: impl Into<String>) -> Self {
        Self {
            phase_id: phase_id.into(),
            label: None,
        }
    }

    /// Build a phase reference with a stable identifier and display label.
    pub fn labeled(phase_id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            phase_id: phase_id.into(),
            label: Some(label.into()),
        }
    }
}

/// Input surface related to a warning or error.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InputSurfaceRef {
    pub surface_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl InputSurfaceRef {
    /// Build an input-surface reference.
    pub fn new(surface_id: impl Into<String>) -> Self {
        Self {
            surface_id: surface_id.into(),
            label: None,
        }
    }
}

/// Input field related to a warning or error.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InputFieldRef {
    pub field_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl InputFieldRef {
    /// Build an input-field reference.
    pub fn new(field_path: impl Into<String>) -> Self {
        Self {
            field_path: field_path.into(),
            label: None,
        }
    }
}

/// Event delivery priority for bounded channels.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventDeliveryPriority {
    /// Lifecycle, artifact, warning, and error events that must be delivered or surfaced as errors.
    Required,
    /// Volatile progress or metric events that may be dropped under backpressure.
    DroppableVolatile,
}

/// One event in a run event stream.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RunEvent {
    pub schema_version: String,
    pub occurred_at_utc: String,
    pub run_id: String,
    pub workflow: WorkflowRef,
    pub priority: EventDeliveryPriority,
    #[serde(flatten)]
    pub payload: RunEventPayload,
}

impl RunEvent {
    /// Build a run event with the current UTC timestamp.
    pub fn new(
        run_id: impl Into<String>,
        workflow: WorkflowRef,
        priority: EventDeliveryPriority,
        payload: RunEventPayload,
    ) -> Self {
        Self::at(Utc::now().to_rfc3339(), run_id, workflow, priority, payload)
    }

    /// Build a run event with an explicit UTC timestamp string.
    pub fn at(
        occurred_at_utc: impl Into<String>,
        run_id: impl Into<String>,
        workflow: WorkflowRef,
        priority: EventDeliveryPriority,
        payload: RunEventPayload,
    ) -> Self {
        Self {
            schema_version: RUN_EVENT_SCHEMA_VERSION.to_string(),
            occurred_at_utc: occurred_at_utc.into(),
            run_id: run_id.into(),
            workflow,
            priority,
            payload,
        }
    }

    pub(crate) fn kind_name(&self) -> &'static str {
        self.payload.kind_name()
    }
}

/// Domain payload carried by a run event.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "event_class", content = "payload", rename_all = "snake_case")]
pub enum RunEventPayload {
    Lifecycle(RunLifecycleEvent),
    Artifact(RunArtifactEvent),
    Warning(RunWarningEvent),
    Error(RunErrorEvent),
    Progress(RunProgressEvent),
    MetricUpdate(RunMetricUpdateEvent),
}

impl RunEventPayload {
    pub(crate) const fn kind_name(&self) -> &'static str {
        match self {
            Self::Lifecycle(event) => event.kind_name(),
            Self::Artifact(_) => "artifact",
            Self::Warning(_) => "warning",
            Self::Error(_) => "error",
            Self::Progress(_) => "progress",
            Self::MetricUpdate(_) => "metric_update",
        }
    }
}

/// Durable run lifecycle event.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "event_kind", rename_all = "snake_case")]
pub enum RunLifecycleEvent {
    RunStarted {
        scope_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        scope_label: Option<String>,
        inputs: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_directory: Option<String>,
        stage_sequence: Vec<RunStageRef>,
    },
    StageStarted {
        stage: RunStageRef,
        stage_index: usize,
        stage_count: usize,
    },
    StageCompleted {
        stage: RunStageRef,
        stage_index: usize,
        stage_count: usize,
        runtime_seconds: f64,
    },
    RunCompleted {
        total_runtime_seconds: f64,
    },
    RunFailed {
        total_runtime_seconds: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        failed_stage: Option<RunStageRef>,
        #[serde(skip_serializing_if = "Option::is_none")]
        failed_phase: Option<RunPhaseRef>,
        reason: String,
    },
    RunCancelled {
        total_runtime_seconds: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        cancelled_before_stage: Option<RunStageRef>,
    },
}

impl RunLifecycleEvent {
    pub(crate) const fn kind_name(&self) -> &'static str {
        match self {
            Self::RunStarted { .. } => "run_started",
            Self::StageStarted { .. } => "stage_started",
            Self::StageCompleted { .. } => "stage_completed",
            Self::RunCompleted { .. } => "run_completed",
            Self::RunFailed { .. } => "run_failed",
            Self::RunCancelled { .. } => "run_cancelled",
        }
    }
}

/// Durable artifact event.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RunArtifactEvent {
    pub artifact_kind: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<RunStageRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<RunPhaseRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Durable warning event.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RunWarningEvent {
    pub warning_code: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<RunStageRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<RunPhaseRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_surface: Option<InputSurfaceRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<InputFieldRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_paths: Vec<String>,
}

/// Durable error event.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RunErrorEvent {
    pub error_code: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<RunStageRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<RunPhaseRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_surface: Option<InputSurfaceRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<InputFieldRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_paths: Vec<String>,
}

/// Volatile progress event.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RunProgressEvent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<RunStageRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<RunPhaseRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subphase: Option<RunPhaseRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub measures: Vec<ProgressMeasure>,
}

/// Volatile metric update event.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RunMetricUpdateEvent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<RunStageRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<RunPhaseRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub measures: Vec<ProgressMeasure>,
}

/// Reusable bounded count contract.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BoundedCount {
    pub completed_units: usize,
    pub total_units: usize,
}

/// Pricing grid coordinates for progress reporting.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PricingProgressGeometry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<BoundedCount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maturity: Option<BoundedCount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strike: Option<BoundedCount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval: Option<BoundedCount>,
}

/// Progress or metric payload with stable identifiers and optional labels.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "measure_kind", rename_all = "snake_case")]
pub enum ProgressMeasure {
    BoundedCount {
        metric_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        completed_units: usize,
        total_units: usize,
    },
    EvaluationCount {
        metric_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        completed_evaluations: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        evaluation_budget: Option<usize>,
    },
    Convergence {
        metric_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        value: f64,
    },
    CandidateId {
        metric_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        candidate_id: String,
    },
    ObjectiveValue {
        metric_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        value: f64,
    },
    PricingGeometry {
        metric_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        geometry: PricingProgressGeometry,
    },
    SimulationIntervalCount {
        metric_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        completed_intervals: usize,
        total_intervals: usize,
    },
    OutputChunkCount {
        metric_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        completed_chunks: usize,
        total_chunks: usize,
    },
    AnalyticsCheckCount {
        metric_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        completed_checks: usize,
        total_checks: usize,
    },
    Scalar {
        metric_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        value: f64,
    },
    Text {
        metric_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        value: String,
    },
}
