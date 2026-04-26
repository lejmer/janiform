use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::path::Path;
use std::path::PathBuf;

use crate::error::EventLogError;
use crate::event::RunEvent;
use crate::state::RunStateSnapshot;

/// Replay a JSONL event log into the latest known run state.
pub fn replay_event_log(path: impl AsRef<Path>) -> Result<RunStateSnapshot, EventLogError> {
    let path = path.as_ref();
    let file = File::open(path).map_err(|source| EventLogError::OpenEventLog {
        path: PathBuf::from(path),
        source,
    })?;

    let reader = BufReader::new(file);

    let mut state = None;

    for (zero_based_line_number, line_result) in reader.lines().enumerate() {
        let line_number = zero_based_line_number + 1;

        let line = line_result.map_err(|source| EventLogError::ReadEventLog {
            path: PathBuf::from(path),
            source,
        })?;

        let event: RunEvent =
            serde_json::from_str(&line).map_err(|source| EventLogError::ParseEventLogLine {
                path: PathBuf::from(path),
                line_number,
                source,
            })?;

        let state = state.get_or_insert_with(|| {
            RunStateSnapshot::new(event.run_id.clone(), event.workflow.clone())
        });

        state.apply_event(&event);
    }

    state.ok_or_else(|| EventLogError::EmptyEventLog {
        path: PathBuf::from(path),
    })
}
