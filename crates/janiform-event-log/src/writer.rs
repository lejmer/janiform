use std::fs;
use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use crate::error::EventLogError;
use crate::event::{RunEvent, RunEventPayload, RunLifecycleEvent, WorkflowRef};
use crate::state::RunStateSnapshot;

/// Explicit snapshot persistence policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotWritePolicy {
    /// Persist the latest-state snapshot after every accepted event.
    EveryAcceptedEvent,
    /// Persist only after durable lifecycle, artifact, warning, and error boundaries.
    DurableBoundariesOnly,
}

/// Explicit event persistence policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventPersistencePolicy {
    pub snapshot_policy: SnapshotWritePolicy,
}

impl Default for EventPersistencePolicy {
    fn default() -> Self {
        Self {
            snapshot_policy: SnapshotWritePolicy::EveryAcceptedEvent,
        }
    }
}

/// JSONL event-log writer that also maintains latest-state snapshots.
pub struct RunEventLogWriter {
    event_log_path: PathBuf,
    snapshot_path: Option<PathBuf>,
    writer: BufWriter<File>,
    state: RunStateSnapshot,
    policy: EventPersistencePolicy,
}

impl RunEventLogWriter {
    /// Create a JSONL event writer for a run.
    pub fn create(
        event_log_path: impl AsRef<Path>,
        snapshot_path: Option<impl AsRef<Path>>,
        run_id: impl Into<String>,
        workflow: WorkflowRef,
    ) -> Result<Self, EventLogError> {
        Self::create_with_policy(
            event_log_path,
            snapshot_path,
            run_id,
            workflow,
            EventPersistencePolicy::default(),
        )
    }

    /// Create a JSONL event writer with an explicit persistence policy.
    pub fn create_with_policy(
        event_log_path: impl AsRef<Path>,
        snapshot_path: Option<impl AsRef<Path>>,
        run_id: impl Into<String>,
        workflow: WorkflowRef,
        policy: EventPersistencePolicy,
    ) -> Result<Self, EventLogError> {
        let event_log_path = PathBuf::from(event_log_path.as_ref());
        if let Some(parent) = event_log_path.parent() {
            fs::create_dir_all(parent).map_err(|source| EventLogError::CreateDirectory {
                path: PathBuf::from(parent),
                source,
            })?;
        }

        let file =
            File::create(&event_log_path).map_err(|source| EventLogError::CreateEventLog {
                path: event_log_path.clone(),
                source,
            })?;

        Ok(Self {
            event_log_path,
            snapshot_path: snapshot_path.map(|path| PathBuf::from(path.as_ref())),
            writer: BufWriter::new(file),
            state: RunStateSnapshot::new(run_id, workflow),
            policy,
        })
    }

    /// Append one event, update state, and flush on durable boundaries.
    pub fn append_event(&mut self, event: &RunEvent) -> Result<(), EventLogError> {
        write_jsonl_event(&mut self.writer, &self.event_log_path, event)?;

        self.state.apply_event(event);

        if self.should_persist_snapshot(event)
            && let Some(snapshot_path) = &self.snapshot_path
        {
            persist_run_state_snapshot(snapshot_path, &self.state)?;
        }

        if flush_boundary(event) {
            self.flush()?;
        }

        Ok(())
    }

    /// Flush buffered event bytes to the event log.
    pub fn flush(&mut self) -> Result<(), EventLogError> {
        self.writer
            .flush()
            .map_err(|source| EventLogError::FlushEventLog {
                path: self.event_log_path.clone(),
                source,
            })
    }

    /// Current reconstructed state.
    pub fn state(&self) -> &RunStateSnapshot {
        &self.state
    }

    fn should_persist_snapshot(&self, event: &RunEvent) -> bool {
        match self.policy.snapshot_policy {
            SnapshotWritePolicy::EveryAcceptedEvent => true,
            SnapshotWritePolicy::DurableBoundariesOnly => durable_snapshot_boundary(event),
        }
    }
}

/// Append one event to an existing writer as one JSONL line.
pub fn append_event_to_log<W: Write>(
    writer: &mut W,
    event_log_path: &Path,
    event: &RunEvent,
) -> Result<(), EventLogError> {
    write_jsonl_event(writer, event_log_path, event)
}

/// Persist a latest-state snapshot as JSON through an atomic temporary file publish.
pub fn persist_run_state_snapshot(
    path: impl AsRef<Path>,
    state: &RunStateSnapshot,
) -> Result<(), EventLogError> {
    let path = path.as_ref();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| EventLogError::CreateSnapshotDirectory {
            path: PathBuf::from(parent),
            source,
        })?;
    }

    let temporary_path = temporary_snapshot_path(path);
    {
        let file = File::create(&temporary_path).map_err(|source| {
            EventLogError::CreateTemporarySnapshot {
                path: temporary_path.clone(),
                source,
            }
        })?;

        let mut writer = BufWriter::new(file);

        serde_json::to_writer_pretty(&mut writer, state).map_err(|source| {
            EventLogError::WriteSnapshot {
                path: temporary_path.clone(),
                source: Box::new(source),
            }
        })?;

        writeln!(writer).map_err(|source| EventLogError::WriteSnapshot {
            path: temporary_path.clone(),
            source: Box::new(source),
        })?;

        writer
            .flush()
            .map_err(|source| EventLogError::WriteSnapshot {
                path: temporary_path.clone(),
                source: Box::new(source),
            })?;
    }

    fs::rename(&temporary_path, path).map_err(|source| EventLogError::PublishSnapshot {
        path: PathBuf::from(path),
        source,
    })
}

/// Load a latest-state snapshot from disk.
pub fn load_run_state_snapshot(path: impl AsRef<Path>) -> Result<RunStateSnapshot, EventLogError> {
    let path = path.as_ref();
    let file = File::open(path).map_err(|source| EventLogError::OpenSnapshot {
        path: PathBuf::from(path),
        source,
    })?;

    serde_json::from_reader(file).map_err(|source| EventLogError::ParseSnapshot {
        path: PathBuf::from(path),
        source,
    })
}

fn write_jsonl_event<W: Write>(
    writer: &mut W,
    event_log_path: &Path,
    event: &RunEvent,
) -> Result<(), EventLogError> {
    serde_json::to_writer(&mut *writer, event).map_err(|source| EventLogError::WriteEventLog {
        path: PathBuf::from(event_log_path),
        source: Box::new(source),
    })?;

    writeln!(writer).map_err(|source| EventLogError::WriteEventLog {
        path: PathBuf::from(event_log_path),
        source: Box::new(source),
    })
}

fn durable_snapshot_boundary(event: &RunEvent) -> bool {
    matches!(
        &event.payload,
        RunEventPayload::Lifecycle(_)
            | RunEventPayload::Artifact(_)
            | RunEventPayload::Warning(_)
            | RunEventPayload::Error(_)
    )
}

fn flush_boundary(event: &RunEvent) -> bool {
    match &event.payload {
        RunEventPayload::Lifecycle(lifecycle) => matches!(
            lifecycle,
            RunLifecycleEvent::StageStarted { .. }
                | RunLifecycleEvent::StageCompleted { .. }
                | RunLifecycleEvent::RunCompleted { .. }
                | RunLifecycleEvent::RunFailed { .. }
        ),
        RunEventPayload::Artifact(artifact) => artifact.artifact_kind == "checkpoint",
        RunEventPayload::Warning(_) | RunEventPayload::Error(_) => true,
        RunEventPayload::Progress(_) | RunEventPayload::MetricUpdate(_) => false,
    }
}

fn temporary_snapshot_path(path: &Path) -> PathBuf {
    let Some(file_name) = path.file_name().and_then(|file_name| file_name.to_str()) else {
        return path.with_extension("tmp");
    };

    path.with_file_name(format!("{file_name}.tmp"))
}
