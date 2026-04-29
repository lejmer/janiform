//! Shared string validation helpers.

use std::error::Error;
use std::fmt::{Display, Formatter};

/// Error raised when a string value is empty after trimming whitespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NonEmptyStringValidationError {
    /// Stable field or argument name associated with the rejected value.
    pub field: &'static str,
}

impl NonEmptyStringValidationError {
    /// Return the stable validation message without field context.
    pub fn message(self) -> &'static str {
        "must be a non-empty string"
    }
}

impl Display for NonEmptyStringValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{} {}", self.field, self.message())
    }
}

impl Error for NonEmptyStringValidationError {}

/// Validate that a string contains at least one non-whitespace character.
pub fn validate_non_empty_string(
    value: &str,
    field: &'static str,
) -> Result<(), NonEmptyStringValidationError> {
    if !is_non_empty_string(value) {
        return Err(NonEmptyStringValidationError { field });
    }

    Ok(())
}

/// Return whether a string contains at least one non-whitespace character.
pub fn is_non_empty_string(value: &str) -> bool {
    !value.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::{NonEmptyStringValidationError, is_non_empty_string, validate_non_empty_string};

    #[test]
    fn is_non_empty_string_classifies_trimmed_text() {
        assert!(is_non_empty_string(" ES "));
        assert!(!is_non_empty_string(" \t\n"));
    }

    #[test]
    fn validate_non_empty_string_accepts_non_whitespace_text() {
        assert_eq!(validate_non_empty_string(" ES ", "exchange_code"), Ok(()));
    }

    #[test]
    fn validate_non_empty_string_rejects_empty_text() {
        assert_eq!(
            validate_non_empty_string(" \t\n", "exchange_code"),
            Err(NonEmptyStringValidationError {
                field: "exchange_code"
            })
        );
    }
}
