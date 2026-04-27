use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::path::Path;
use std::path::PathBuf;

use crate::error::EventLogError;
use crate::event::RunEvent;
use crate::state::RunStateSnapshot;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TrailingPartialLinePolicy {
    Reject,
    IgnoreMalformedUnterminated,
}

/// Replay a JSONL event log into the latest known run state.
pub fn replay_event_log(path: impl AsRef<Path>) -> Result<RunStateSnapshot, EventLogError> {
    replay_event_log_with_policy(path, TrailingPartialLinePolicy::Reject)
}

/// Replay a JSONL event log while tolerating a final in-progress event row.
pub fn replay_event_log_allowing_trailing_partial(
    path: impl AsRef<Path>,
) -> Result<RunStateSnapshot, EventLogError> {
    replay_event_log_with_policy(path, TrailingPartialLinePolicy::IgnoreMalformedUnterminated)
}

fn replay_event_log_with_policy(
    path: impl AsRef<Path>,
    trailing_partial_line_policy: TrailingPartialLinePolicy,
) -> Result<RunStateSnapshot, EventLogError> {
    let path = path.as_ref();
    let file = File::open(path).map_err(|source| EventLogError::OpenEventLog {
        path: PathBuf::from(path),
        source,
    })?;

    let mut reader = BufReader::new(file);

    let mut state = None;
    let mut line = String::new();
    let mut line_number = 0;

    loop {
        line.clear();
        let byte_count =
            reader
                .read_line(&mut line)
                .map_err(|source| EventLogError::ReadEventLog {
                    path: PathBuf::from(path),
                    source,
                })?;

        if byte_count == 0 {
            break;
        }

        line_number += 1;
        let line_is_terminated = line.ends_with('\n');

        let event: RunEvent = match serde_json::from_str(&line) {
            Ok(event) => event,
            Err(source)
                if trailing_partial_line_policy
                    == TrailingPartialLinePolicy::IgnoreMalformedUnterminated
                    && !line_is_terminated =>
            {
                break;
            }
            Err(source) => {
                return Err(EventLogError::ParseEventLogLine {
                    path: PathBuf::from(path),
                    line_number,
                    source,
                });
            }
        };

        let state = state.get_or_insert_with(|| {
            RunStateSnapshot::new(event.run_id.clone(), event.workflow.clone())
        });

        state.apply_event(&event);
    }

    state.ok_or_else(|| EventLogError::EmptyEventLog {
        path: PathBuf::from(path),
    })
}
