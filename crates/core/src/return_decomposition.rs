//! Canonical return-decomposition domain model shared by pricing and simulation.
//!
//! Equation reference:
//! \f[
//! X = L^+(A^+) - L^-(A^-) + M + G.
//! \f]

use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Deserialize;
use serde::Serialize;

use crate::nig::{
    ComplexScalar, NigBuySellDriverParameters, NigTradingTimeCharacteristicExponentContributions,
    NigTradingTimeIncrementContributions, NigTradingTimeIncrementSamplingError,
};
use crate::randomness::SimulationRandomnessStream;

pub const RETURN_DECOMPOSITION_INTERFACE_POLICY: &str =
    "explicit_a_plus_a_minus_open_jump_and_closed_gap_component_fields_without_tuple_passing.v1";
pub const RETURN_DECOMPOSITION_UNITS_POLICY: &str = "a_plus_and_a_minus_are_integrated_intensity_expected_event_counts_and_open_jump_and_closed_gap_are_log_return_increments.v1";
pub const RETURN_DECOMPOSITION_EDGE_MODE_POLICY: &str = "explicit_opt_in_buy_only_sell_only_open_jump_only_and_closed_gap_only_component_disable_modes_for_debug_and_test_workflows.v1";
pub const RETURN_DECOMPOSITION_INTERFACE_EVOLUTION_POLICY: &str = "additive_only_changes_for_existing_fields_and_behavior_breaking_changes_require_new_policy_identifier.v1";

/// Explicit component-disable modes for test and debug workflows.
///
/// `Disabled` preserves the canonical decomposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReturnDecompositionEdgeMode {
    Disabled,
    BuyOnly,
    SellOnly,
    OpenJumpOnly,
    ClosedGapOnly,
}

impl ReturnDecompositionEdgeMode {
    pub const fn policy() -> &'static str {
        RETURN_DECOMPOSITION_EDGE_MODE_POLICY
    }

    pub const fn buy_component_enabled(self) -> bool {
        matches!(self, Self::Disabled | Self::BuyOnly)
    }

    pub const fn sell_component_enabled(self) -> bool {
        matches!(self, Self::Disabled | Self::SellOnly)
    }

    pub const fn open_jump_component_enabled(self) -> bool {
        matches!(self, Self::Disabled | Self::OpenJumpOnly)
    }

    pub const fn closed_gap_component_enabled(self) -> bool {
        matches!(self, Self::Disabled | Self::ClosedGapOnly)
    }
}

impl Default for ReturnDecompositionEdgeMode {
    fn default() -> Self {
        Self::Disabled
    }
}

/// Explicit field assignment payload for constructing `ReturnDecomposition`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReturnDecompositionInput {
    pub a_plus: f64,
    pub a_minus: f64,
    pub open_jump: f64,
    pub closed_gap: f64,
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

    /// Open-window direct return-jump contribution \f$M\f$ over the interval.
    ///
    /// Units: log-return increment.
    pub open_jump: f64,

    /// Closed-window gap return contribution \f$G\f$ over the interval.
    ///
    /// Units: log-return increment.
    pub closed_gap: f64,
}

impl ReturnDecomposition {
    /// Build one validated return decomposition payload from explicit named fields.
    pub fn try_new(
        components: ReturnDecompositionInput,
    ) -> Result<Self, ReturnDecompositionValidationError> {
        validate_non_negative_finite("a_plus", components.a_plus)?;
        validate_non_negative_finite("a_minus", components.a_minus)?;
        validate_finite("open_jump", components.open_jump)?;
        validate_finite("closed_gap", components.closed_gap)?;

        Ok(Self {
            a_plus: components.a_plus,
            a_minus: components.a_minus,
            open_jump: components.open_jump,
            closed_gap: components.closed_gap,
        })
    }

    /// Re-run domain validation for an existing payload.
    pub fn validate(self) -> Result<(), ReturnDecompositionValidationError> {
        validate_non_negative_finite("a_plus", self.a_plus)?;
        validate_non_negative_finite("a_minus", self.a_minus)?;
        validate_finite("open_jump", self.open_jump)?;
        validate_finite("closed_gap", self.closed_gap)?;

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

    /// Return the canonical interface evolution policy identifier.
    pub const fn interface_evolution_policy() -> &'static str {
        RETURN_DECOMPOSITION_INTERFACE_EVOLUTION_POLICY
    }

    pub const fn integrated_buy_intensity(self) -> f64 {
        self.a_plus
    }

    pub const fn integrated_sell_intensity(self) -> f64 {
        self.a_minus
    }

    /// Compose trading-time weighted characteristic exponents through one shared decomposition map.
    pub fn compose_nig_trading_time_exponent(
        self,
        driver_parameters: NigBuySellDriverParameters,
        argument: ComplexScalar,
    ) -> NigTradingTimeCharacteristicExponentContributions {
        self.compose_nig_trading_time_exponent_with_edge_mode(
            driver_parameters,
            argument,
            ReturnDecompositionEdgeMode::Disabled,
        )
    }

    /// Compose trading-time weighted characteristic exponents with optional edge-mode gating.
    pub fn compose_nig_trading_time_exponent_with_edge_mode(
        self,
        driver_parameters: NigBuySellDriverParameters,
        argument: ComplexScalar,
        edge_mode: ReturnDecompositionEdgeMode,
    ) -> NigTradingTimeCharacteristicExponentContributions {
        let effective_decomposition = self.with_edge_mode(edge_mode);

        driver_parameters.compose_trading_time_weighted_characteristic_exponent(
            argument,
            effective_decomposition.integrated_buy_intensity(),
            effective_decomposition.integrated_sell_intensity(),
        )
    }

    /// Sample one trading-time return increment through the shared decomposition map.
    pub fn sample_nig_trading_time_increment<R>(
        self,
        driver_parameters: NigBuySellDriverParameters,
        randomness_stream: &mut R,
    ) -> Result<NigTradingTimeIncrementContributions, NigTradingTimeIncrementSamplingError>
    where
        R: SimulationRandomnessStream,
    {
        self.sample_nig_trading_time_increment_with_edge_mode(
            driver_parameters,
            ReturnDecompositionEdgeMode::Disabled,
            randomness_stream,
        )
    }

    /// Sample one trading-time return increment with optional edge-mode gating.
    pub fn sample_nig_trading_time_increment_with_edge_mode<R>(
        self,
        driver_parameters: NigBuySellDriverParameters,
        edge_mode: ReturnDecompositionEdgeMode,
        randomness_stream: &mut R,
    ) -> Result<NigTradingTimeIncrementContributions, NigTradingTimeIncrementSamplingError>
    where
        R: SimulationRandomnessStream,
    {
        let effective_decomposition = self.with_edge_mode(edge_mode);

        driver_parameters.sample_trading_time_return_increment(
            effective_decomposition.integrated_buy_intensity(),
            effective_decomposition.integrated_sell_intensity(),
            randomness_stream,
        )
    }

    /// Apply one explicit edge mode by zeroing disabled decomposition components.
    pub fn with_edge_mode(self, edge_mode: ReturnDecompositionEdgeMode) -> Self {
        let a_plus = if edge_mode.buy_component_enabled() {
            self.a_plus
        } else {
            0.0
        };

        let a_minus = if edge_mode.sell_component_enabled() {
            self.a_minus
        } else {
            0.0
        };

        let open_jump = if edge_mode.open_jump_component_enabled() {
            self.open_jump
        } else {
            0.0
        };

        let closed_gap = if edge_mode.closed_gap_component_enabled() {
            self.closed_gap
        } else {
            0.0
        };

        Self {
            a_plus,
            a_minus,
            open_jump,
            closed_gap,
        }
    }
}

/// Compose trading-time weighted characteristic exponents with shared edge-mode handling.
pub fn compose_nig_trading_time_exponent_with_edge_mode(
    driver_parameters: NigBuySellDriverParameters,
    argument: ComplexScalar,
    return_decomposition: ReturnDecomposition,
    edge_mode: ReturnDecompositionEdgeMode,
) -> NigTradingTimeCharacteristicExponentContributions {
    return_decomposition.compose_nig_trading_time_exponent_with_edge_mode(
        driver_parameters,
        argument,
        edge_mode,
    )
}

/// Sample one trading-time return increment with shared edge-mode handling.
pub fn sample_nig_trading_time_increment_with_edge_mode<R>(
    driver_parameters: NigBuySellDriverParameters,
    return_decomposition: ReturnDecomposition,
    edge_mode: ReturnDecompositionEdgeMode,
    randomness_stream: &mut R,
) -> Result<NigTradingTimeIncrementContributions, NigTradingTimeIncrementSamplingError>
where
    R: SimulationRandomnessStream,
{
    return_decomposition.sample_nig_trading_time_increment_with_edge_mode(
        driver_parameters,
        edge_mode,
        randomness_stream,
    )
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
    open_jump: f64,
    closed_gap: f64,
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
            open_jump: payload.open_jump,
            closed_gap: payload.closed_gap,
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
    use crate::nig::{ComplexScalar, NigSharedPlusShiftConfiguration};
    use crate::randomness::SplitMix64SimulationRandomnessStream;

    use super::{
        RETURN_DECOMPOSITION_EDGE_MODE_POLICY, RETURN_DECOMPOSITION_INTERFACE_EVOLUTION_POLICY,
        RETURN_DECOMPOSITION_INTERFACE_POLICY, RETURN_DECOMPOSITION_UNITS_POLICY,
        ReturnDecomposition, ReturnDecompositionEdgeMode, ReturnDecompositionInput,
        ReturnDecompositionValidationError,
    };

    #[test]
    fn valid_contract_construction_and_round_trip_serialization_preserve_component_mapping() {
        let decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 1.7,
            a_minus: 0.45,
            open_jump: -0.12,
            closed_gap: 0.05,
        })
        .expect("valid return decomposition payload should construct");

        assert_eq!(decomposition.a_plus, 1.7);
        assert_eq!(decomposition.a_minus, 0.45);
        assert_eq!(decomposition.open_jump, -0.12);
        assert_eq!(decomposition.closed_gap, 0.05);
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
        assert_eq!(serialized["open_jump"], serde_json::json!(-0.12));
        assert_eq!(serialized["closed_gap"], serde_json::json!(0.05));

        let parsed: ReturnDecomposition =
            serde_json::from_value(serialized).expect("serialized payload should deserialize");
        assert_eq!(parsed, decomposition);
    }

    #[test]
    fn validation_rejects_invalid_components() {
        let invalid_buy_side = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: -0.01,
            a_minus: 0.2,
            open_jump: 0.0,
            closed_gap: 0.0,
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

        let invalid_open_jump = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 0.4,
            a_minus: 0.3,
            open_jump: f64::NAN,
            closed_gap: 0.0,
        })
        .expect_err("non-finite open_jump must fail validation");

        match invalid_open_jump {
            ReturnDecompositionValidationError::DomainViolation {
                field,
                constraint,
                value,
            } => {
                assert_eq!(field, "open_jump");
                assert_eq!(constraint, super::ReturnDecompositionConstraint::Finite);
                assert!(value.is_nan());
            }
        }
    }

    #[test]
    fn deserialization_rejects_unknown_or_incomplete_payloads() {
        let missing_closed_gap_payload = serde_json::json!({
            "a_plus": 0.7,
            "a_minus": 0.5,
            "open_jump": 0.1
        });
        let missing_closed_gap_error =
            serde_json::from_value::<ReturnDecomposition>(missing_closed_gap_payload)
                .expect_err("missing closed_gap field must fail deserialization");

        assert!(
            missing_closed_gap_error
                .to_string()
                .contains("missing field `closed_gap`")
        );

        let unknown_field_payload = serde_json::json!({
            "a_plus": 0.7,
            "a_minus": 0.5,
            "open_jump": 0.1,
            "closed_gap": -0.2,
            "integrated_buy_intensity": 0.7
        });
        let unknown_field_error =
            serde_json::from_value::<ReturnDecomposition>(unknown_field_payload)
                .expect_err("unknown fields must fail deserialization");

        assert!(unknown_field_error.to_string().contains("unknown field"));
    }

    #[test]
    fn edge_mode_zeroing_is_explicit_and_deterministic() {
        let decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 1.5,
            a_minus: 0.8,
            open_jump: 0.15,
            closed_gap: -0.4,
        })
        .expect("test decomposition payload should construct");

        assert_eq!(
            ReturnDecompositionEdgeMode::policy(),
            RETURN_DECOMPOSITION_EDGE_MODE_POLICY
        );
        assert_eq!(
            decomposition.with_edge_mode(ReturnDecompositionEdgeMode::Disabled),
            decomposition
        );
        assert_eq!(
            decomposition.with_edge_mode(ReturnDecompositionEdgeMode::BuyOnly),
            ReturnDecomposition {
                a_plus: 1.5,
                a_minus: 0.0,
                open_jump: 0.0,
                closed_gap: 0.0,
            }
        );
        assert_eq!(
            decomposition.with_edge_mode(ReturnDecompositionEdgeMode::SellOnly),
            ReturnDecomposition {
                a_plus: 0.0,
                a_minus: 0.8,
                open_jump: 0.0,
                closed_gap: 0.0,
            }
        );
        assert_eq!(
            decomposition.with_edge_mode(ReturnDecompositionEdgeMode::OpenJumpOnly),
            ReturnDecomposition {
                a_plus: 0.0,
                a_minus: 0.0,
                open_jump: 0.15,
                closed_gap: 0.0,
            }
        );
        assert_eq!(
            decomposition.with_edge_mode(ReturnDecompositionEdgeMode::ClosedGapOnly),
            ReturnDecomposition {
                a_plus: 0.0,
                a_minus: 0.0,
                open_jump: 0.0,
                closed_gap: -0.4,
            }
        );
    }

    #[test]
    fn shared_trading_time_exponent_adapter_matches_direct_driver_composition() {
        let driver_parameters = valid_nig_driver_parameters();
        let decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 1.2,
            a_minus: 0.7,
            open_jump: -0.3,
            closed_gap: 0.2,
        })
        .expect("test decomposition should be valid");
        let argument = ComplexScalar::new(0.25, -0.4);

        let observed = decomposition.compose_nig_trading_time_exponent_with_edge_mode(
            driver_parameters,
            argument,
            ReturnDecompositionEdgeMode::SellOnly,
        );
        let expected = driver_parameters
            .compose_trading_time_weighted_characteristic_exponent(argument, 0.0, 0.7);

        assert_eq!(observed, expected);
    }

    #[test]
    fn shared_trading_time_increment_adapter_matches_direct_driver_composition() {
        let driver_parameters = valid_nig_driver_parameters();
        let decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 0.9,
            a_minus: 0.4,
            open_jump: 0.1,
            closed_gap: -0.2,
        })
        .expect("test decomposition should be valid");

        let mut observed_stream = SplitMix64SimulationRandomnessStream::from_seed(981_220);
        let mut expected_stream = SplitMix64SimulationRandomnessStream::from_seed(981_220);

        let observed = decomposition
            .sample_nig_trading_time_increment_with_edge_mode(
                driver_parameters,
                ReturnDecompositionEdgeMode::BuyOnly,
                &mut observed_stream,
            )
            .expect("adapter sampling should succeed");
        let expected = driver_parameters
            .sample_trading_time_return_increment(0.9, 0.0, &mut expected_stream)
            .expect("direct driver sampling should succeed");

        assert_eq!(observed, expected);
    }

    #[test]
    fn interface_evolution_policy_identifier_is_stable() {
        assert_eq!(
            ReturnDecomposition::interface_evolution_policy(),
            "additive_only_changes_for_existing_fields_and_behavior_breaking_changes_require_new_policy_identifier.v1"
        );
        assert_eq!(
            ReturnDecomposition::interface_evolution_policy(),
            RETURN_DECOMPOSITION_INTERFACE_EVOLUTION_POLICY
        );
    }

    fn valid_nig_driver_parameters() -> crate::nig::NigBuySellDriverParameters {
        NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("test shared-plus-shift payload should construct")
            .derive_driver_parameters()
            .expect("test shared-plus-shift payload should derive drivers")
    }
}
