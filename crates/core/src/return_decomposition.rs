//! Canonical return-decomposition domain model shared by pricing and simulation.
//!
//! Equation reference:
//! \f[
//! X = L^+(A^+) - L^-(A^-) + G.
//! \f]

use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Deserialize;
use serde::Serialize;

pub const RETURN_DECOMPOSITION_INTERFACE_POLICY: &str =
    "explicit_a_plus_a_minus_and_gap_component_fields_without_tuple_passing.v1";
pub const RETURN_DECOMPOSITION_UNITS_POLICY: &str = "a_plus_and_a_minus_are_integrated_intensity_expected_event_counts_and_gap_is_log_return_increment.v1";

/// Explicit field assignment payload for constructing `ReturnDecomposition`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReturnDecompositionInput {
    pub a_plus: f64,
    pub a_minus: f64,
    pub gap: f64,
}

/// One validated return decomposition payload with explicit component fields.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ReturnDecomposition {
    /// Integrated buy intensity \f$A^+\f$ over the interval.
    ///
    /// Units: expected buy-event count in business time.
    pub a_plus: f64,

    /// Integrated sell intensity \f$A^-\f$ over the interval.
    ///
    /// Units: expected sell-event count in business time.
    pub a_minus: f64,

    /// Closed-market gap return contribution \f$G\f$ over the interval.
    ///
    /// Units: log-return increment.
    pub gap: f64,
}

impl ReturnDecomposition {
    /// Build one validated return decomposition payload from explicit named fields.
    pub fn try_new(
        components: ReturnDecompositionInput,
    ) -> Result<Self, ReturnDecompositionValidationError> {
        validate_non_negative_finite("a_plus", components.a_plus)?;
        validate_non_negative_finite("a_minus", components.a_minus)?;
        validate_finite("gap", components.gap)?;

        Ok(Self {
            a_plus: components.a_plus,
            a_minus: components.a_minus,
            gap: components.gap,
        })
    }

    /// Re-run domain validation for an existing payload.
    pub fn validate(self) -> Result<(), ReturnDecompositionValidationError> {
        validate_non_negative_finite("a_plus", self.a_plus)?;
        validate_non_negative_finite("a_minus", self.a_minus)?;
        validate_finite("gap", self.gap)?;

        Ok(())
    }

    /// Return the canonical interface policy identifier for this contract.
    pub const fn interface_policy() -> &'static str {
        RETURN_DECOMPOSITION_INTERFACE_POLICY
    }

    /// Return the canonical units policy identifier for this contract.
    pub const fn units_policy() -> &'static str {
        RETURN_DECOMPOSITION_UNITS_POLICY
    }

    pub const fn integrated_buy_intensity(self) -> f64 {
        self.a_plus
    }

    pub const fn integrated_sell_intensity(self) -> f64 {
        self.a_minus
    }
}

/// Validation rule for one return-decomposition component field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ReturnDecompositionConstraint {
    Finite,
    FiniteAndNonNegative,
}

impl Display for ReturnDecompositionConstraint {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Finite => write!(formatter, "finite"),
            Self::FiniteAndNonNegative => write!(formatter, "finite_and_non_negative"),
        }
    }
}

/// Structured validation failure for `ReturnDecomposition::try_new`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReturnDecompositionValidationError {
    DomainViolation {
        field: &'static str,
        constraint: ReturnDecompositionConstraint,
        value: f64,
    },
}

impl Display for ReturnDecompositionValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DomainViolation {
                field,
                constraint: ReturnDecompositionConstraint::Finite,
                value,
            } => write!(formatter, "{field} must be finite, found {value}"),
            Self::DomainViolation {
                field,
                constraint: ReturnDecompositionConstraint::FiniteAndNonNegative,
                value,
            } => write!(
                formatter,
                "{field} must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for ReturnDecompositionValidationError {}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReturnDecompositionPayload {
    a_plus: f64,
    a_minus: f64,
    gap: f64,
}

impl<'de> Deserialize<'de> for ReturnDecomposition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = ReturnDecompositionPayload::deserialize(deserializer)?;
        Self::try_new(ReturnDecompositionInput {
            a_plus: payload.a_plus,
            a_minus: payload.a_minus,
            gap: payload.gap,
        })
        .map_err(serde::de::Error::custom)
    }
}

fn validate_non_negative_finite(
    field: &'static str,
    value: f64,
) -> Result<(), ReturnDecompositionValidationError> {
    if !value.is_finite() || value < 0.0 {
        return Err(ReturnDecompositionValidationError::DomainViolation {
            field,
            constraint: ReturnDecompositionConstraint::FiniteAndNonNegative,
            value,
        });
    }

    Ok(())
}

fn validate_finite(
    field: &'static str,
    value: f64,
) -> Result<(), ReturnDecompositionValidationError> {
    if !value.is_finite() {
        return Err(ReturnDecompositionValidationError::DomainViolation {
            field,
            constraint: ReturnDecompositionConstraint::Finite,
            value,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        RETURN_DECOMPOSITION_INTERFACE_POLICY, RETURN_DECOMPOSITION_UNITS_POLICY,
        ReturnDecomposition, ReturnDecompositionInput, ReturnDecompositionValidationError,
    };

    #[test]
    fn valid_contract_construction_and_round_trip_serialization_preserve_component_mapping() {
        let decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 1.7,
            a_minus: 0.45,
            gap: -0.12,
        })
        .expect("valid return decomposition payload should construct");

        assert_eq!(decomposition.a_plus, 1.7);
        assert_eq!(decomposition.a_minus, 0.45);
        assert_eq!(decomposition.gap, -0.12);
        assert_eq!(
            ReturnDecomposition::interface_policy(),
            RETURN_DECOMPOSITION_INTERFACE_POLICY
        );
        assert_eq!(
            ReturnDecomposition::units_policy(),
            RETURN_DECOMPOSITION_UNITS_POLICY
        );

        let serialized = serde_json::to_value(decomposition)
            .expect("return decomposition payload should serialize");
        assert_eq!(serialized["a_plus"], serde_json::json!(1.7));
        assert_eq!(serialized["a_minus"], serde_json::json!(0.45));
        assert_eq!(serialized["gap"], serde_json::json!(-0.12));

        let parsed: ReturnDecomposition =
            serde_json::from_value(serialized).expect("serialized payload should deserialize");
        assert_eq!(parsed, decomposition);
    }

    #[test]
    fn validation_rejects_invalid_components() {
        let invalid_buy_side = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: -0.01,
            a_minus: 0.2,
            gap: 0.0,
        })
        .expect_err("negative a_plus must fail validation");
        assert_eq!(
            invalid_buy_side,
            ReturnDecompositionValidationError::DomainViolation {
                field: "a_plus",
                constraint: super::ReturnDecompositionConstraint::FiniteAndNonNegative,
                value: -0.01,
            }
        );

        let invalid_gap = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 0.4,
            a_minus: 0.3,
            gap: f64::NAN,
        })
        .expect_err("non-finite gap must fail validation");

        match invalid_gap {
            ReturnDecompositionValidationError::DomainViolation {
                field,
                constraint,
                value,
            } => {
                assert_eq!(field, "gap");
                assert_eq!(constraint, super::ReturnDecompositionConstraint::Finite);
                assert!(value.is_nan());
            }
        }
    }

    #[test]
    fn deserialization_rejects_unknown_or_incomplete_payloads() {
        let missing_gap_payload = serde_json::json!({
            "a_plus": 0.7,
            "a_minus": 0.5
        });
        let missing_gap_error = serde_json::from_value::<ReturnDecomposition>(missing_gap_payload)
            .expect_err("missing gap field must fail deserialization");

        assert!(
            missing_gap_error
                .to_string()
                .contains("missing field `gap`")
        );

        let unknown_field_payload = serde_json::json!({
            "a_plus": 0.7,
            "a_minus": 0.5,
            "gap": 0.1,
            "integrated_buy_intensity": 0.7
        });
        let unknown_field_error =
            serde_json::from_value::<ReturnDecomposition>(unknown_field_payload)
                .expect_err("unknown fields must fail deserialization");

        assert!(unknown_field_error.to_string().contains("unknown field"));
    }
}
