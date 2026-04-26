use std::path::Path;
use std::path::PathBuf;

use crate::event::EVENT_LOG_FILE_NAME;
use crate::event::RUN_STATE_SNAPSHOT_FILE_NAME;

/// Default event paths for a run directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunEventPaths {
    pub event_log_path: PathBuf,
    pub latest_state_snapshot_path: PathBuf,
}

/// Resolve the default event log and latest-state snapshot paths for a run directory.
pub fn default_run_event_paths(run_directory: impl AsRef<Path>) -> RunEventPaths {
    let run_directory = run_directory.as_ref();

    RunEventPaths {
        event_log_path: run_directory.join(EVENT_LOG_FILE_NAME),
        latest_state_snapshot_path: run_directory.join(RUN_STATE_SNAPSHOT_FILE_NAME),
    }
}
