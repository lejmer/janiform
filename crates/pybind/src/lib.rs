mod bindings;
mod types;

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyModule;

create_exception!(_native, CalendarInvalidTimezoneError, PyException);
create_exception!(_native, CalendarMissingExchangeError, PyException);
create_exception!(_native, CalendarRegistryLoadError, PyException);
create_exception!(_native, CalendarTimestampParseError, PyValueError);
create_exception!(_native, PricingExecutionError, PyException);
create_exception!(_native, CalibrationExecutionError, PyException);
create_exception!(_native, SeasonalityInputValidationError, PyValueError);
create_exception!(_native, HourlyFactorProviderMissingInputError, PyValueError);
create_exception!(
    _native,
    HourlyFactorProviderUnsupportedRangeError,
    PyValueError
);

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
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
