use std::error::Error;
use std::fmt::{Display, Formatter};

/// Error returned when an integer cannot be converted to an exactly represented `f64`.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct IntegerToF64ExactConversionError {
    /// Field or contract name being converted.
    pub field: &'static str,
    /// Original integer value.
    pub value: u64,
}

impl Display for IntegerToF64ExactConversionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} exceeds the exactly representable integer range for f64: {}",
            self.field, self.value
        )
    }
}

impl Error for IntegerToF64ExactConversionError {}

/// Convert a `usize` to `f64` only when the integer is exactly representable.
pub fn usize_to_f64_exact(
    value: usize,
    field: &'static str,
) -> Result<f64, IntegerToF64ExactConversionError> {
    let Ok(value_u64) = u64::try_from(value) else {
        return Err(IntegerToF64ExactConversionError {
            field,
            value: u64::MAX,
        });
    };

    u64_to_f64_exact(value_u64, field)
}

/// Convert a `u64` to `f64` only when the integer is exactly representable.
pub fn u64_to_f64_exact(
    value: u64,
    field: &'static str,
) -> Result<f64, IntegerToF64ExactConversionError> {
    let exact_upper_bound = 1_u64 << f64::MANTISSA_DIGITS;

    if value > exact_upper_bound {
        return Err(IntegerToF64ExactConversionError { field, value });
    }

    Ok(value as f64)
}
