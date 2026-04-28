use std::sync::{Arc, Mutex};

use pyo3::prelude::{Py, PyErr, PyResult, Python};
use pyo3::types::PyAny;
use serde::Serialize;
use serde_json::Value;

use crate::python_value::json_value_to_py_object;

pub(crate) type CallbackErrorState = Arc<Mutex<Option<String>>>;

pub(crate) fn callback_error_state() -> CallbackErrorState {
    Arc::new(Mutex::new(None))
}

pub(crate) fn callback_failed(callback_error_state: &CallbackErrorState) -> bool {
    callback_error_state
        .lock()
        .map(|guard| guard.is_some())
        .unwrap_or(false)
}

pub(crate) fn emit_serializable_callback_event<TEvent>(
    event_callback: &Py<PyAny>,
    callback_error_state: &CallbackErrorState,
    event: &TEvent,
    event_description: &str,
) where
    TEvent: Serialize,
{
    if callback_failed(callback_error_state) {
        return;
    }

    emit_callback_event_payload(event_callback, callback_error_state, |py| {
        python_event_to_py_object(py, event, event_description)
    });
}

pub(crate) fn emit_callback_event_payload<TPayload>(
    event_callback: &Py<PyAny>,
    callback_error_state: &CallbackErrorState,
    payload_factory: TPayload,
) where
    TPayload: FnOnce(Python<'_>) -> PyResult<Py<PyAny>>,
{
    if callback_failed(callback_error_state) {
        return;
    }

    Python::attach(|py| {
        let callback_result = payload_factory(py)
            .and_then(|payload| event_callback.call1(py, (payload,)).map(|_| ()));

        if let Err(source) = callback_result {
            record_callback_error(callback_error_state, source.to_string());
        }
    });
}

pub(crate) fn raise_callback_error<TError>(
    callback_error_state: &CallbackErrorState,
    message_prefix: &str,
    error_factory: TError,
) -> PyResult<()>
where
    TError: FnOnce(String) -> PyErr,
{
    let Ok(guard) = callback_error_state.lock() else {
        return Ok(());
    };

    let Some(error_message) = guard.as_ref() else {
        return Ok(());
    };

    Err(error_factory(format!("{message_prefix}: {error_message}")))
}

fn record_callback_error(callback_error_state: &CallbackErrorState, error_message: String) {
    let Ok(mut guard) = callback_error_state.lock() else {
        return;
    };

    if guard.is_some() {
        return;
    }

    *guard = Some(error_message);
}

fn python_event_to_py_object<TEvent>(
    py: Python<'_>,
    event: &TEvent,
    event_description: &str,
) -> PyResult<Py<PyAny>>
where
    TEvent: Serialize,
{
    let serialized_event = match serde_json::to_value(event).map_err(|source| {
        PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!(
            "failed to serialize {event_description}: {source}"
        ))
    })? {
        Value::Object(object) => Value::Object(object),
        _ => {
            return Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!(
                "failed to serialize {event_description}: expected a JSON object"
            )));
        }
    };

    json_value_to_py_object(py, &serialized_event)
}
