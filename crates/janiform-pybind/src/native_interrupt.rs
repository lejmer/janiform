use std::thread;

use janiform_foundations::cancellation::{
    CancellationRequest, CancellationSource, CancellationToken, NATIVE_INTERRUPT_POLL_INTERVAL,
};
use pyo3::PyErr;
use pyo3::exceptions::{PyKeyboardInterrupt, PyRuntimeError};
use pyo3::prelude::{PyResult, Python};

const CLI_INTERRUPT_EXIT_CODE: i32 = 130;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeInterruptMode {
    Cli,
    ImportedApi,
}

pub(crate) fn run_interruptible_native<T, Operation>(
    py: Python<'_>,
    mode: NativeInterruptMode,
    operation: Operation,
) -> PyResult<T>
where
    T: Send,
    Operation: FnOnce(CancellationToken) -> PyResult<T> + Send,
{
    run_interruptible_native_with_poll(py, mode, operation, |_| Ok(()))
}

pub(crate) fn run_interruptible_native_with_poll<T, Operation, Poll>(
    py: Python<'_>,
    mode: NativeInterruptMode,
    operation: Operation,
    mut poll: Poll,
) -> PyResult<T>
where
    T: Send,
    Operation: FnOnce(CancellationToken) -> PyResult<T> + Send,
    Poll: FnMut(Python<'_>) -> PyResult<()>,
{
    let cancellation_source = CancellationSource::new();

    thread::scope(|scope| {
        let cancellation_token = cancellation_source.token();
        let worker = scope.spawn(move || operation(cancellation_token));

        let mut interrupt_error = None::<PyErr>;

        loop {
            poll(py)?;

            if worker.is_finished() {
                let worker_result = match worker.join() {
                    Ok(result) => result,
                    Err(_) => Err(PyRuntimeError::new_err(
                        "native worker terminated unexpectedly during interruptible execution",
                    )),
                };

                poll(py)?;

                if mode == NativeInterruptMode::ImportedApi
                    && let Some(error) = interrupt_error
                {
                    return Err(error);
                }

                return worker_result;
            }

            py.detach(|| thread::sleep(NATIVE_INTERRUPT_POLL_INTERVAL));

            poll(py)?;

            match py.check_signals() {
                Ok(()) => {}
                Err(error) if error.is_instance_of::<PyKeyboardInterrupt>(py) => {
                    handle_keyboard_interrupt(
                        mode,
                        &cancellation_source,
                        error,
                        &mut interrupt_error,
                    );
                }
                Err(error) => return Err(error),
            }
        }
    })
}

fn handle_keyboard_interrupt(
    mode: NativeInterruptMode,
    cancellation_source: &CancellationSource,
    error: PyErr,
    interrupt_error: &mut Option<PyErr>,
) {
    if interrupt_error.is_none() {
        *interrupt_error = Some(error);
    }

    if cancellation_source.request_cancellation() != CancellationRequest::ForceRequested {
        return;
    }

    if mode == NativeInterruptMode::Cli {
        std::process::exit(CLI_INTERRUPT_EXIT_CODE);
    }
}
