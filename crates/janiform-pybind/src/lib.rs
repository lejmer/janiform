mod bindings;
mod panic_boundary;
mod python_value;
mod types;

use std::sync::OnceLock;

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyValueError};
use pyo3::prelude::{Bound, PyResult};
use pyo3::pymodule;
use pyo3::types::{PyModule, PyModuleMethods};
use tracing_subscriber::EnvFilter;

create_exception!(_native, CalendarInvalidTimezoneError, PyException);
create_exception!(_native, CalendarMissingExchangeError, PyException);
create_exception!(_native, CalendarRegistryLoadError, PyException);
create_exception!(_native, CalendarTimestampParseError, PyValueError);
create_exception!(_native, PricingExecutionError, PyException);
create_exception!(_native, CalibrationExecutionError, PyException);
create_exception!(_native, SimulationExecutionError, PyException);
create_exception!(_native, SeasonalityInputValidationError, PyValueError);
create_exception!(_native, HourlyFactorProviderMissingInputError, PyValueError);
create_exception!(
    _native,
    HourlyFactorProviderUnsupportedRangeError,
    PyValueError
);

fn initialize_tracing_subscriber() {
    static INITIALIZED: OnceLock<()> = OnceLock::new();

    INITIALIZED.get_or_init(|| {
        let env_filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::builder().parse_lossy("error"));

        let _ = tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_target(true)
            .without_time()
            .try_init();
    });
}

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    initialize_tracing_subscriber();
    module.add("__doc__", "Rust bindings for Janiform.")?;
    module.add(
        "CalendarRegistryLoadError",
        module.py().get_type::<CalendarRegistryLoadError>(),
    )?;
    module.add(
        "CalendarMissingExchangeError",
        module.py().get_type::<CalendarMissingExchangeError>(),
    )?;
    module.add(
        "CalendarInvalidTimezoneError",
        module.py().get_type::<CalendarInvalidTimezoneError>(),
    )?;
    module.add(
        "CalendarTimestampParseError",
        module.py().get_type::<CalendarTimestampParseError>(),
    )?;
    module.add(
        "PricingExecutionError",
        module.py().get_type::<PricingExecutionError>(),
    )?;
    module.add(
        "CalibrationExecutionError",
        module.py().get_type::<CalibrationExecutionError>(),
    )?;
    module.add(
        "SimulationExecutionError",
        module.py().get_type::<SimulationExecutionError>(),
    )?;
    module.add(
        "SeasonalityInputValidationError",
        module.py().get_type::<SeasonalityInputValidationError>(),
    )?;
    module.add(
        "HourlyFactorProviderMissingInputError",
        module
            .py()
            .get_type::<HourlyFactorProviderMissingInputError>(),
    )?;
    module.add(
        "HourlyFactorProviderUnsupportedRangeError",
        module
            .py()
            .get_type::<HourlyFactorProviderUnsupportedRangeError>(),
    )?;

    types::register_classes(module)?;
    bindings::register_functions(module)?;

    Ok(())
}
