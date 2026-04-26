use std::io;
use std::path::PathBuf;

use thiserror::Error;

/// Error raised by run event logging, snapshot persistence, replay, or delivery.
#[derive(Debug, Error)]
pub enum EventLogError {
    #[error("event channel capacity must be greater than zero")]
    EmptyChannelCapacity,

    #[error("required event `{event_kind}` could not be delivered because the channel is full")]
    RequiredEventBackpressure { event_kind: &'static str },

    #[error("event `{event_kind}` could not be delivered because the receiver is closed")]
    EventReceiverClosed { event_kind: &'static str },

    #[error("failed to create event-log directory `{}`", path.display())]
    CreateDirectory { path: PathBuf, source: io::Error },

    #[error("failed to create event log `{}`", path.display())]
    CreateEventLog { path: PathBuf, source: io::Error },

    #[error("failed to open event log `{}`", path.display())]
    OpenEventLog { path: PathBuf, source: io::Error },

    #[error("failed to write event log `{}`", path.display())]
    WriteEventLog {
        path: PathBuf,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("failed to flush event log `{}`", path.display())]
    FlushEventLog { path: PathBuf, source: io::Error },

    #[error("event log `{}` is empty", path.display())]
    EmptyEventLog { path: PathBuf },

    #[error("failed to read event log `{}`", path.display())]
    ReadEventLog { path: PathBuf, source: io::Error },

    #[error("failed to parse event log `{}` at line {line_number}", path.display())]
    ParseEventLogLine {
        path: PathBuf,
        line_number: usize,
        source: serde_json::Error,
    },

    #[error("failed to create snapshot directory `{}`", path.display())]
    CreateSnapshotDirectory { path: PathBuf, source: io::Error },

    #[error("failed to create temporary snapshot `{}`", path.display())]
    CreateTemporarySnapshot { path: PathBuf, source: io::Error },

    #[error("failed to write snapshot `{}`", path.display())]
    WriteSnapshot {
        path: PathBuf,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("failed to publish snapshot `{}`", path.display())]
    PublishSnapshot { path: PathBuf, source: io::Error },

    #[error("failed to open snapshot `{}`", path.display())]
    OpenSnapshot { path: PathBuf, source: io::Error },

    #[error("failed to parse snapshot `{}`", path.display())]
    ParseSnapshot {
        path: PathBuf,
        source: serde_json::Error,
    },
}
