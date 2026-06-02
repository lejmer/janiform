use std::fmt::Display;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use janiform::pricing::PricingProgressEvent;
use janiform_foundations::cancellation::CancellationToken;
use pyo3::prelude::{Bound, Py, PyResult, Python};
use pyo3::types::PyAny;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::PricingExecutionError;
use crate::bindings::events::{
    callback_error_state, callback_failed, emit_serializable_callback_event, raise_callback_error,
};
use crate::native_interrupt::{NativeInterruptMode, run_interruptible_native_with_poll};
use crate::python_value::{json_value_to_py_object, py_any_to_json_value};

const PRICING_PROGRESS_CALLBACK_MIN_INTERVAL: Duration = Duration::from_millis(100);

pub(super) fn run_pricing_workflow_with_events<TExecutionInput, TRunResult, TRunError, TRunFn>(
    py: Python<'_>,
    execution_input: &Bound<'_, PyAny>,
    event_callback: &Bound<'_, PyAny>,
    execution_input_description: &str,
    run_result_description: &str,
    run_with_progress_sender: TRunFn,
) -> PyResult<Py<PyAny>>
where
    TExecutionInput: DeserializeOwned + Send + 'static,
    TRunResult: Serialize + Send + 'static,
    TRunError: Display + Send + 'static,
    TRunFn: FnOnce(
            TExecutionInput,
            mpsc::Sender<PricingProgressEvent>,
            CancellationToken,
        ) -> Result<TRunResult, TRunError>
        + Send
        + 'static,
{
    let execution_input: TExecutionInput =
        serde_json::from_value(py_any_to_json_value(execution_input)?).map_err(|source| {
            PricingExecutionError::new_err(format!(
                "failed to deserialize {execution_input_description}: {source}"
            ))
        })?;

    let event_callback = event_callback.clone().unbind();
    let callback_error_state = callback_error_state();

    let (progress_sender, progress_receiver) = mpsc::channel::<PricingProgressEvent>();

    let mut last_emitted_event = None::<PricingProgressEvent>;
    let mut last_emitted_at = None::<Instant>;

    let run_result = run_interruptible_native_with_poll(
        py,
        NativeInterruptMode::ImportedApi,
        move |cancellation_token| {
            Ok(run_with_progress_sender(
                execution_input,
                progress_sender,
                cancellation_token,
            ))
        },
        |_: Python<'_>| {
            for event in progress_receiver.try_iter() {
                if callback_failed(&callback_error_state) {
                    continue;
                }

                if !should_emit_pricing_progress_event(
                    &event,
                    last_emitted_event.as_ref(),
                    last_emitted_at,
                ) {
                    continue;
                }

                last_emitted_event = Some(event.clone());
                last_emitted_at = Some(Instant::now());

                emit_serializable_callback_event(
                    &event_callback,
                    &callback_error_state,
                    &event,
                    "pricing progress event",
                );
            }

            Ok(())
        },
    )?;

    let run_result =
        run_result.map_err(|source| PricingExecutionError::new_err(source.to_string()))?;

    raise_callback_error(
        &callback_error_state,
        "pricing progress callback failed",
        PricingExecutionError::new_err,
    )?;

    let serialized_run_result = serde_json::to_value(&run_result).map_err(|source| {
        PricingExecutionError::new_err(format!(
            "failed to serialize {run_result_description}: {source}"
        ))
    })?;

    json_value_to_py_object(py, &serialized_run_result)
}

fn should_emit_pricing_progress_event(
    event: &PricingProgressEvent,
    last_emitted_event: Option<&PricingProgressEvent>,
    last_emitted_at: Option<Instant>,
) -> bool {
    let Some(previous_event) = last_emitted_event else {
        return true;
    };

    if previous_event.phase != event.phase {
        return true;
    }

    if event.completed_units >= event.total_units {
        return true;
    }

    if previous_event.completed_units != event.completed_units {
        return last_emitted_at
            .is_none_or(|instant| instant.elapsed() >= PRICING_PROGRESS_CALLBACK_MIN_INTERVAL);
    }

    false
}
