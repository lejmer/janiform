use pyo3::IntoPyObject;
use pyo3::prelude::{Bound, Py, PyErr, PyResult, Python};
use pyo3::types::{
    PyAny, PyAnyMethods, PyBool, PyDict, PyDictMethods, PyFloat, PyInt, PyList, PyListMethods,
    PyString, PyStringMethods, PyTuple, PyTupleMethods,
};
use serde_json::{Map, Number, Value};

pub(crate) fn py_any_to_json_value(value: &Bound<'_, PyAny>) -> PyResult<Value> {
    if value.is_none() {
        return Ok(Value::Null);
    }

    if let Ok(boolean_value) = value.cast::<PyBool>() {
        return Ok(Value::Bool(boolean_value.extract::<bool>()?));
    }

    if let Ok(integer_value) = value.cast::<PyInt>() {
        if let Ok(parsed_value) = integer_value.extract::<i64>() {
            return Ok(Value::Number(Number::from(parsed_value)));
        }
        if let Ok(parsed_value) = integer_value.extract::<u64>() {
            return Ok(Value::Number(Number::from(parsed_value)));
        }
    }

    if let Ok(float_value) = value.cast::<PyFloat>() {
        let parsed_value = float_value.extract::<f64>()?;

        let number = Number::from_f64(parsed_value).ok_or_else(|| {
            PyErr::new::<pyo3::exceptions::PyValueError, _>(
                "floating-point payload values must be finite",
            )
        })?;

        return Ok(Value::Number(number));
    }

    if let Ok(string_value) = value.cast::<PyString>() {
        return Ok(Value::String(string_value.to_str()?.to_owned()));
    }

    if let Ok(dict_value) = value.cast::<PyDict>() {
        let mut mapped_value = Map::with_capacity(dict_value.len());

        for (raw_key, raw_item) in dict_value.iter() {
            let key = raw_key
                .cast::<PyString>()
                .map_err(|_| {
                    PyErr::new::<pyo3::exceptions::PyTypeError, _>(
                        "mapping payload keys must be strings",
                    )
                })?
                .to_str()?
                .to_owned();
            mapped_value.insert(key, py_any_to_json_value(&raw_item)?);
        }

        return Ok(Value::Object(mapped_value));
    }

    if let Ok(list_value) = value.cast::<PyList>() {
        let mut items = Vec::with_capacity(list_value.len());

        for raw_item in list_value.iter() {
            items.push(py_any_to_json_value(&raw_item)?);
        }

        return Ok(Value::Array(items));
    }

    if let Ok(tuple_value) = value.cast::<PyTuple>() {
        let mut items = Vec::with_capacity(tuple_value.len());

        for raw_item in tuple_value.iter() {
            items.push(py_any_to_json_value(&raw_item)?);
        }

        return Ok(Value::Array(items));
    }

    Err(PyErr::new::<pyo3::exceptions::PyTypeError, _>(
        "payload values must be built from Python dict, list, tuple, string, number, bool, or None",
    ))
}

pub(crate) fn json_value_to_py_object(py: Python<'_>, value: &Value) -> PyResult<Py<PyAny>> {
    match value {
        Value::Null => Ok(py.None()),
        Value::Bool(boolean_value) => Ok(PyBool::new(py, *boolean_value)
            .to_owned()
            .into_any()
            .unbind()),
        Value::Number(number_value) => {
            if let Some(integer_value) = number_value.as_i64() {
                Ok(integer_value.into_pyobject(py)?.into_any().unbind())
            } else if let Some(integer_value) = number_value.as_u64() {
                Ok(integer_value.into_pyobject(py)?.into_any().unbind())
            } else if let Some(float_value) = number_value.as_f64() {
                Ok(float_value.into_pyobject(py)?.into_any().unbind())
            } else {
                Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
                    "numeric payload could not be converted to Python",
                ))
            }
        }
        Value::String(string_value) => Ok(PyString::new(py, string_value).into_any().unbind()),
        Value::Array(array_value) => {
            let list = PyList::empty(py);
            for item in array_value {
                list.append(json_value_to_py_object(py, item)?)?;
            }
            Ok(list.into_any().unbind())
        }
        Value::Object(object_value) => {
            let dict = PyDict::new(py);
            for (key, item) in object_value {
                dict.set_item(key, json_value_to_py_object(py, item)?)?;
            }
            Ok(dict.into_any().unbind())
        }
    }
}
