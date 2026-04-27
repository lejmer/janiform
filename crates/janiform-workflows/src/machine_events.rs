use std::io::Write;

use janiform_event_log::RunEvent;

use crate::error::WorkflowError;

/// Write one machine-readable workflow event as JSONL to stderr.
pub fn write_machine_event_to_stderr(event: &RunEvent) -> Result<(), WorkflowError> {
    let mut stderr = std::io::stderr().lock();

    serde_json::to_writer(&mut stderr, event)
        .map_err(|source| WorkflowError::OutputSerialize { source })?;

    writeln!(stderr).map_err(|source| WorkflowError::OutputWrite { source })
}
