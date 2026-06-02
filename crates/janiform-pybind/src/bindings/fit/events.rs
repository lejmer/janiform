use std::sync::mpsc;

use janiform_event_log::RunEvent;
use janiform_workflows::{error::WorkflowError, write_machine_event_to_stderr};
use pyo3::prelude::PyResult;

use crate::CalibrationExecutionError;
use crate::bindings::events::{CallbackErrorState, callback_failed, raise_callback_error};

pub(super) fn fit_machine_event_sink(
    emit_machine_events: bool,
    event_sender: Option<mpsc::Sender<RunEvent>>,
    callback_error_state: CallbackErrorState,
) -> Option<impl FnMut(&RunEvent) -> Result<(), WorkflowError>> {
    if !emit_machine_events && event_sender.is_none() {
        return None;
    }

    Some(move |event: &RunEvent| {
        if emit_machine_events {
            write_machine_event_to_stderr(event)?;
        }

        if let Some(event_sender) = &event_sender {
            if callback_failed(&callback_error_state) {
                return Ok(());
            }

            let _ = event_sender.send(event.clone());
        }

        Ok(())
    })
}

pub(super) fn raise_fit_progress_callback_error(
    callback_error_state: &CallbackErrorState,
) -> PyResult<()> {
    raise_callback_error(
        callback_error_state,
        "fit progress callback failed",
        CalibrationExecutionError::new_err,
    )
}
