use pyo3::PyErr;
use pyo3::PyResult;
use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};

pub(crate) fn panic_to_pyresult<T, F, E>(
    context: &'static str,
    error_builder: E,
    operation: F,
) -> PyResult<T>
where
    F: FnOnce() -> PyResult<T>,
    E: Fn(String) -> PyErr,
{
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(result) => result,
        Err(payload) => Err(error_builder(format_panic_message(context, payload))),
    }
}

fn format_panic_message(context: &'static str, payload: Box<dyn Any + Send>) -> String {
    format!(
        "native panic while {context}: {}",
        panic_payload_message(payload.as_ref())
    )
}

fn panic_payload_message(payload: &(dyn Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        return message;
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.as_str();
    }

    "unknown panic payload"
}
