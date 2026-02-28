//! Shared verification contracts and helpers.

use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegressionCaseStatus {
    Pass,
    Fail,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VerificationContractError {
    InvalidNonNegativeFiniteValue {
        field_name: &'static str,
        value: f64,
    },
}

impl Display for VerificationContractError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidNonNegativeFiniteValue { field_name, value } => {
                write!(
                    formatter,
                    "{field_name} must be finite and non-negative, found {value}"
                )
            }
        }
    }
}

impl Error for VerificationContractError {}
