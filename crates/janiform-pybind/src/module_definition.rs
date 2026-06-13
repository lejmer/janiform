use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyValueError};
use pyo3::prelude::{Bound, PyResult};
use pyo3::pymodule;
use pyo3::types::{PyModule, PyModuleMethods};

use crate::{bindings, types};

create_exception!(_native, CalendarInvalidTimezoneError, PyException);
create_exception!(_native, CalendarMissingExchangeError, PyException);
create_exception!(_native, CalendarRegistryLoadError, PyException);
create_exception!(_native, CalendarTimestampParseError, PyValueError);
create_exception!(_native, PricingExecutionError, PyException);
create_exception!(_native, CalibrationExecutionError, PyException);
create_exception!(_native, CalibrationValidationError, PyValueError);
create_exception!(_native, SimulationExecutionError, PyException);
create_exception!(_native, SeasonalityInputValidationError, PyValueError);
create_exception!(
    _native,
    IntervalFactorGridProviderMissingInputError,
    PyValueError
);
create_exception!(
    _native,
    IntervalFactorGridProviderUnsupportedRangeError,
    PyValueError
);

#[pymodule]
pub(crate) fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
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
        "CalibrationValidationError",
        module.py().get_type::<CalibrationValidationError>(),
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
        "IntervalFactorGridProviderMissingInputError",
        module
            .py()
            .get_type::<IntervalFactorGridProviderMissingInputError>(),
    )?;

    module.add(
        "IntervalFactorGridProviderUnsupportedRangeError",
        module
            .py()
            .get_type::<IntervalFactorGridProviderUnsupportedRangeError>(),
    )?;

    types::register_classes(module)?;
    bindings::register_functions(module)?;

    Ok(())
}
