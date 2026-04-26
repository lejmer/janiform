//! Structured run event logs, latest-state snapshots, and replay support.

#![forbid(unsafe_code)]

mod channel;
mod error;
mod event;
mod paths;
mod replay;
mod state;
mod writer;

pub use channel::EventDeliveryOutcome;
pub use channel::RunEventSender;
pub use channel::bounded_event_channel;
pub use error::EventLogError;
pub use event::BoundedCount;
pub use event::EVENT_CHANNEL_DELIVERY_POLICY_ID;
pub use event::EVENT_LOG_FILE_NAME;
pub use event::EventDeliveryPriority;
pub use event::InputFieldRef;
pub use event::InputSurfaceRef;
pub use event::PricingProgressGeometry;
pub use event::ProgressMeasure;
pub use event::RUN_EVENT_SCHEMA_VERSION;
pub use event::RUN_STATE_SNAPSHOT_FILE_NAME;
pub use event::RUN_STATE_SNAPSHOT_SCHEMA_VERSION;
pub use event::RunArtifactEvent;
pub use event::RunErrorEvent;
pub use event::RunEvent;
pub use event::RunEventPayload;
pub use event::RunLifecycleEvent;
pub use event::RunMetricUpdateEvent;
pub use event::RunPhaseRef;
pub use event::RunProgressEvent;
pub use event::RunStageRef;
pub use event::RunWarningEvent;
pub use event::WorkflowRef;
pub use paths::RunEventPaths;
pub use paths::default_run_event_paths;
pub use replay::replay_event_log;
pub use state::ArtifactSummary;
pub use state::CheckpointAvailability;
pub use state::RunFailureSummary;
pub use state::RunStageState;
pub use state::RunStageStatus;
pub use state::RunStateSnapshot;
pub use state::RunStatus;
pub use state::WarningSummary;
pub use writer::EventPersistencePolicy;
pub use writer::RunEventLogWriter;
pub use writer::SnapshotWritePolicy;
pub use writer::append_event_to_log;
pub use writer::load_run_state_snapshot;
pub use writer::persist_run_state_snapshot;
