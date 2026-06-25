use std::io;
use std::path::PathBuf;

use janiform_foundations::paths::format_path;
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

    #[error("failed to create event-log directory `{}`", format_path(path))]
    CreateDirectory { path: PathBuf, source: io::Error },

    #[error("failed to create event log `{}`", format_path(path))]
    CreateEventLog { path: PathBuf, source: io::Error },

    #[error("failed to open event log `{}`", format_path(path))]
    OpenEventLog { path: PathBuf, source: io::Error },

    #[error("failed to write event log `{}`", format_path(path))]
    WriteEventLog {
        path: PathBuf,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("failed to flush event log `{}`", format_path(path))]
    FlushEventLog { path: PathBuf, source: io::Error },

    #[error("event log `{}` is empty", format_path(path))]
    EmptyEventLog { path: PathBuf },

    #[error("failed to read event log `{}`", format_path(path))]
    ReadEventLog { path: PathBuf, source: io::Error },

    #[error(
        "failed to parse event log `{}` at line {line_number}",
        format_path(path)
    )]
    ParseEventLogLine {
        path: PathBuf,
        line_number: usize,
        source: serde_json::Error,
    },

    #[error("failed to create snapshot directory `{}`", format_path(path))]
    CreateSnapshotDirectory { path: PathBuf, source: io::Error },

    #[error("failed to create temporary snapshot `{}`", format_path(path))]
    CreateTemporarySnapshot { path: PathBuf, source: io::Error },

    #[error("failed to write snapshot `{}`", format_path(path))]
    WriteSnapshot {
        path: PathBuf,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("failed to publish snapshot `{}`", format_path(path))]
    PublishSnapshot { path: PathBuf, source: io::Error },

    #[error("failed to open snapshot `{}`", format_path(path))]
    OpenSnapshot { path: PathBuf, source: io::Error },

    #[error("failed to parse snapshot `{}`", format_path(path))]
    ParseSnapshot {
        path: PathBuf,
        source: serde_json::Error,
    },
}
