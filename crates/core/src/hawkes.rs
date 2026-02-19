//! Canonical bivariate Hawkes state and kernel parameter types.
//!
//! The de-seasonalized buy and sell intensities are:
//! `tilde_lambda_plus = lambda_bar + Y++ + Y+-`
//! `tilde_lambda_minus = lambda_bar + Y-+ + Y--`
//!
//! The sign mapping is:
//! - `plus` means buy (`+`)
//! - `minus` means sell (`-`)
//!
//! For component notation `X_ij`:
//! - first sign `i` is target intensity (`+` buy, `-` sell)
//! - second sign `j` is source event sign (`+` buy, `-` sell)
//!
//! This module uses explicit named fields for all 4 components to avoid
//! tuple or matrix-index ambiguity in public core interfaces.

use std::error::Error;
use std::fmt::{Display, Formatter};

/// Per-target source components with explicit buy/sell naming.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HawkesSourceComponents {
    /// Contribution from buy (`+`) source events.
    pub from_plus: f64,

    /// Contribution from sell (`-`) source events.
    pub from_minus: f64,
}

impl HawkesSourceComponents {
    pub const fn new(from_plus: f64, from_minus: f64) -> Self {
        Self {
            from_plus,
            from_minus,
        }
    }
}

/// Side label for endogenous buy/sell event jumps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HawkesEventSide {
    Buy,
    Sell,
}

impl TryFrom<&str> for HawkesEventSide {
    type Error = HawkesEventSidePayloadError;

    fn try_from(side_payload: &str) -> Result<Self, Self::Error> {
        match side_payload {
            "buy" => Ok(Self::Buy),
            "sell" => Ok(Self::Sell),
            _ => Err(HawkesEventSidePayloadError {
                value: side_payload.to_owned(),
            }),
        }
    }
}

/// Structured parse failure for event-side payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HawkesEventSidePayloadError {
    pub value: String,
}

impl Display for HawkesEventSidePayloadError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "unsupported event side '{}', expected one of: buy, sell",
            self.value
        )
    }
}

impl Error for HawkesEventSidePayloadError {}

/// Canonical 4D Hawkes excitation state with explicit `Yij` components.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HawkesExcitationState {
    /// `Y++`: buy-target intensity excitation from buy-source events.
    pub y_plus_plus: f64,

    /// `Y+-`: buy-target intensity excitation from sell-source events.
    pub y_plus_minus: f64,

    /// `Y-+`: sell-target intensity excitation from buy-source events.
    pub y_minus_plus: f64,

    /// `Y--`: sell-target intensity excitation from sell-source events.
    pub y_minus_minus: f64,
}

impl HawkesExcitationState {
    /// Build a state from explicit target/source component groups.
    ///
    /// This helper reduces accidental reordering versus positional 4-value inputs.
    pub const fn from_target_components(
        target_plus: HawkesSourceComponents,
        target_minus: HawkesSourceComponents,
    ) -> Self {
        Self {
            y_plus_plus: target_plus.from_plus,
            y_plus_minus: target_plus.from_minus,
            y_minus_plus: target_minus.from_plus,
            y_minus_minus: target_minus.from_minus,
        }
    }

    /// Return buy-target (`+`) state components grouped by source sign.
    pub const fn target_plus_components(self) -> HawkesSourceComponents {
        HawkesSourceComponents {
            from_plus: self.y_plus_plus,
            from_minus: self.y_plus_minus,
        }
    }

    /// Return sell-target (`-`) state components grouped by source sign.
    pub const fn target_minus_components(self) -> HawkesSourceComponents {
        HawkesSourceComponents {
            from_plus: self.y_minus_plus,
            from_minus: self.y_minus_minus,
        }
    }

    /// Return canonical model ordering: `[Y++, Y+-, Y-+, Y--]`.
    pub const fn as_model_ordered_array(self) -> [f64; 4] {
        [
            self.y_plus_plus,
            self.y_plus_minus,
            self.y_minus_plus,
            self.y_minus_minus,
        ]
    }

    /// Apply closed-form shared-decay drift over `delta_t`.
    ///
    /// The drift equation is:
    /// `Y_{t + delta_t}^{ij} = exp(-beta * delta_t) * Y_t^{ij}`.
    ///
    /// Edge-case semantics:
    /// - `delta_t == 0.0` returns the input state unchanged.
    /// - Large `delta_t` drives all components toward `0.0` and may underflow to
    ///   exact `0.0` based on IEEE-754 floating-point behavior.
    ///
    /// This operator handles drift only and intentionally excludes event-jump or
    /// exogenous loading updates.
    pub fn apply_shared_decay_drift(self, beta: f64, delta_t: f64) -> Self {
        debug_assert!(
            beta.is_finite() && beta > 0.0,
            "beta must be finite and strictly positive"
        );
        debug_assert!(
            delta_t.is_finite() && delta_t >= 0.0,
            "delta_t must be finite and non-negative"
        );

        if delta_t == 0.0 {
            return self;
        }

        let shared_decay_factor = (-beta * delta_t).exp();

        Self {
            y_plus_plus: self.y_plus_plus * shared_decay_factor,
            y_plus_minus: self.y_plus_minus * shared_decay_factor,
            y_minus_plus: self.y_minus_plus * shared_decay_factor,
            y_minus_minus: self.y_minus_minus * shared_decay_factor,
        }
    }

    /// Apply a buy-event endogenous jump using `k_plus`.
    ///
    /// Mapping:
    /// - `k_plus = (alpha_++, 0, alpha_-+, 0)`
    pub const fn apply_buy_event_jump(self, alpha: HawkesKernelLoadings) -> Self {
        self.apply_endogenous_event_jump(HawkesEventSide::Buy, alpha)
    }

    /// Apply a sell-event endogenous jump using `k_minus`.
    ///
    /// Mapping:
    /// - `k_minus = (0, alpha_+-, 0, alpha_--)`
    pub const fn apply_sell_event_jump(self, alpha: HawkesKernelLoadings) -> Self {
        self.apply_endogenous_event_jump(HawkesEventSide::Sell, alpha)
    }

    /// Apply an endogenous event jump by explicit side value.
    ///
    /// This operator handles endogenous `alpha` increments only.
    /// Exogenous `rho` loading is intentionally excluded.
    pub const fn apply_endogenous_event_jump(
        self,
        side: HawkesEventSide,
        alpha: HawkesKernelLoadings,
    ) -> Self {
        match side {
            HawkesEventSide::Buy => Self {
                y_plus_plus: self.y_plus_plus + alpha.plus_plus,
                y_plus_minus: self.y_plus_minus,
                y_minus_plus: self.y_minus_plus + alpha.minus_plus,
                y_minus_minus: self.y_minus_minus,
            },
            HawkesEventSide::Sell => Self {
                y_plus_plus: self.y_plus_plus,
                y_plus_minus: self.y_plus_minus + alpha.plus_minus,
                y_minus_plus: self.y_minus_plus,
                y_minus_minus: self.y_minus_minus + alpha.minus_minus,
            },
        }
    }

    /// Parse a string event-side payload and apply endogenous jump routing.
    pub fn apply_endogenous_event_jump_from_payload(
        self,
        side_payload: &str,
        alpha: HawkesKernelLoadings,
    ) -> Result<Self, HawkesEventSidePayloadError> {
        let side = HawkesEventSide::try_from(side_payload)?;
        Ok(self.apply_endogenous_event_jump(side, alpha))
    }
}

/// Canonical 4-component loading map used for `alpha_ij` and `rho_ij`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HawkesKernelLoadings {
    /// `w++`: buy-target loading from buy-source events.
    pub plus_plus: f64,

    /// `w+-`: buy-target loading from sell-source events.
    pub plus_minus: f64,

    /// `w-+`: sell-target loading from buy-source events.
    pub minus_plus: f64,

    /// `w--`: sell-target loading from sell-source events.
    pub minus_minus: f64,
}

impl HawkesKernelLoadings {
    /// Build loadings from explicit target/source component groups.
    ///
    /// This helper reduces accidental reordering versus positional 4-value inputs.
    pub const fn from_target_components(
        target_plus: HawkesSourceComponents,
        target_minus: HawkesSourceComponents,
    ) -> Self {
        Self {
            plus_plus: target_plus.from_plus,
            plus_minus: target_plus.from_minus,
            minus_plus: target_minus.from_plus,
            minus_minus: target_minus.from_minus,
        }
    }

    /// Return buy-target (`+`) loadings grouped by source sign.
    pub const fn target_plus_components(self) -> HawkesSourceComponents {
        HawkesSourceComponents {
            from_plus: self.plus_plus,
            from_minus: self.plus_minus,
        }
    }

    /// Return sell-target (`-`) loadings grouped by source sign.
    pub const fn target_minus_components(self) -> HawkesSourceComponents {
        HawkesSourceComponents {
            from_plus: self.minus_plus,
            from_minus: self.minus_minus,
        }
    }

    /// Return canonical model ordering: `[++, +-, -+, --]`.
    pub const fn as_model_ordered_array(self) -> [f64; 4] {
        [
            self.plus_plus,
            self.plus_minus,
            self.minus_plus,
            self.minus_minus,
        ]
    }
}

/// Shared-kernel parameter payload for the 4D Hawkes excitation dynamics.
///
/// Mapping to equations:
/// - `beta` is shared exponential decay in `dY_t^{ij} = -beta * Y_t^{ij} dt + ...`
/// - `alpha` stores endogenous excitation loadings `alpha_ij`
/// - `rho` stores exogenous loading terms `rho_ij`
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HawkesKernelParameters {
    pub beta: f64,
    pub alpha: HawkesKernelLoadings,
    pub rho: HawkesKernelLoadings,
}

impl HawkesKernelParameters {
    /// Validate and construct kernel parameters.
    ///
    /// Domain checks:
    /// - `beta > 0` and finite
    /// - all `alpha_ij >= 0` and finite
    /// - all `rho_ij >= 0` and finite
    pub fn try_new(
        beta: f64,
        alpha: HawkesKernelLoadings,
        rho: HawkesKernelLoadings,
    ) -> Result<Self, HawkesKernelParameterError> {
        if !beta.is_finite() || beta <= 0.0 {
            return Err(HawkesKernelParameterError::InvalidBeta { value: beta });
        }

        validate_non_negative_loadings(
            alpha,
            [
                "alpha.plus_plus",
                "alpha.plus_minus",
                "alpha.minus_plus",
                "alpha.minus_minus",
            ],
        )?;
        validate_non_negative_loadings(
            rho,
            [
                "rho.plus_plus",
                "rho.plus_minus",
                "rho.minus_plus",
                "rho.minus_minus",
            ],
        )?;

        Ok(Self { beta, alpha, rho })
    }
}

/// Structured validation failures for `HawkesKernelParameters::try_new`.
#[derive(Debug, Clone, PartialEq)]
pub enum HawkesKernelParameterError {
    InvalidBeta { value: f64 },
    InvalidLoadingComponent { field: &'static str, value: f64 },
}

impl Display for HawkesKernelParameterError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidBeta { value } => {
                write!(
                    formatter,
                    "beta must be finite and strictly positive, found {value}"
                )
            }
            Self::InvalidLoadingComponent { field, value } => write!(
                formatter,
                "{field} must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for HawkesKernelParameterError {}

fn validate_non_negative_loadings(
    loadings: HawkesKernelLoadings,
    component_fields: [&'static str; 4],
) -> Result<(), HawkesKernelParameterError> {
    let component_values = loadings.as_model_ordered_array();

    for (component_field_name, component_value) in
        component_fields.into_iter().zip(component_values)
    {
        if !component_value.is_finite() || component_value < 0.0 {
            return Err(HawkesKernelParameterError::InvalidLoadingComponent {
                field: component_field_name,
                value: component_value,
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::hawkes::{
        HawkesEventSide, HawkesEventSidePayloadError, HawkesExcitationState, HawkesKernelLoadings,
        HawkesKernelParameterError, HawkesKernelParameters, HawkesSourceComponents,
    };

    const DECAY_TOLERANCE: f64 = 1e-12;

    fn assert_within_tolerance(actual: f64, expected: f64, tolerance: f64, field_name: &str) {
        let absolute_difference = (actual - expected).abs();
        assert!(
            absolute_difference <= tolerance,
            "{field_name} mismatch expected {expected}, found {actual}, absolute_difference={absolute_difference}, tolerance={tolerance}"
        );
    }

    #[test]
    fn excitation_state_from_target_components_preserves_component_mapping() {
        let target_plus = HawkesSourceComponents::new(1.0, 2.0);
        let target_minus = HawkesSourceComponents::new(3.0, 4.0);

        let state = HawkesExcitationState::from_target_components(target_plus, target_minus);

        assert_eq!(state.y_plus_plus, 1.0);
        assert_eq!(state.y_plus_minus, 2.0);
        assert_eq!(state.y_minus_plus, 3.0);
        assert_eq!(state.y_minus_minus, 4.0);
        assert_eq!(state.target_plus_components(), target_plus);
        assert_eq!(state.target_minus_components(), target_minus);
        assert_eq!(state.as_model_ordered_array(), [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn shared_decay_drift_matches_closed_form_per_component() {
        let state = HawkesExcitationState {
            y_plus_plus: 0.75,
            y_plus_minus: 1.2,
            y_minus_plus: 0.05,
            y_minus_minus: 2.8,
        };
        let beta = 1.7;
        let delta_t = 0.35;

        let drifted_state = state.apply_shared_decay_drift(beta, delta_t);
        let decay_factor = (-beta * delta_t).exp();

        assert_within_tolerance(
            drifted_state.y_plus_plus,
            state.y_plus_plus * decay_factor,
            DECAY_TOLERANCE,
            "y_plus_plus",
        );
        assert_within_tolerance(
            drifted_state.y_plus_minus,
            state.y_plus_minus * decay_factor,
            DECAY_TOLERANCE,
            "y_plus_minus",
        );
        assert_within_tolerance(
            drifted_state.y_minus_plus,
            state.y_minus_plus * decay_factor,
            DECAY_TOLERANCE,
            "y_minus_plus",
        );
        assert_within_tolerance(
            drifted_state.y_minus_minus,
            state.y_minus_minus * decay_factor,
            DECAY_TOLERANCE,
            "y_minus_minus",
        );
    }

    #[test]
    fn shared_decay_drift_with_zero_delta_t_preserves_state_exactly() {
        let state = HawkesExcitationState {
            y_plus_plus: 0.9,
            y_plus_minus: 0.4,
            y_minus_plus: 0.8,
            y_minus_minus: 1.1,
        };

        let drifted_state = state.apply_shared_decay_drift(1.25, 0.0);

        assert_eq!(drifted_state, state);
    }

    #[test]
    fn shared_decay_drift_with_large_delta_t_underflows_to_zero() {
        let state = HawkesExcitationState {
            y_plus_plus: 0.9,
            y_plus_minus: 0.4,
            y_minus_plus: 0.8,
            y_minus_minus: 1.1,
        };

        let drifted_state = state.apply_shared_decay_drift(1.0, 1_000.0);

        assert_eq!(
            drifted_state,
            HawkesExcitationState {
                y_plus_plus: 0.0,
                y_plus_minus: 0.0,
                y_minus_plus: 0.0,
                y_minus_minus: 0.0,
            }
        );
    }

    #[test]
    fn buy_event_jump_applies_k_plus_component_mapping() {
        let state = HawkesExcitationState {
            y_plus_plus: 0.5,
            y_plus_minus: 0.7,
            y_minus_plus: 0.9,
            y_minus_minus: 1.1,
        };
        let alpha = HawkesKernelLoadings {
            plus_plus: 0.2,
            plus_minus: 0.3,
            minus_plus: 0.4,
            minus_minus: 0.6,
        };

        let updated_state = state.apply_buy_event_jump(alpha);

        assert_eq!(
            updated_state,
            HawkesExcitationState {
                y_plus_plus: 0.7,
                y_plus_minus: 0.7,
                y_minus_plus: 1.3,
                y_minus_minus: 1.1,
            }
        );
    }

    #[test]
    fn sell_event_jump_applies_k_minus_component_mapping() {
        let state = HawkesExcitationState {
            y_plus_plus: 0.5,
            y_plus_minus: 0.7,
            y_minus_plus: 0.9,
            y_minus_minus: 1.1,
        };
        let alpha = HawkesKernelLoadings {
            plus_plus: 0.2,
            plus_minus: 0.3,
            minus_plus: 0.4,
            minus_minus: 0.6,
        };

        let updated_state = state.apply_sell_event_jump(alpha);

        assert_within_tolerance(
            updated_state.y_plus_plus,
            0.5,
            DECAY_TOLERANCE,
            "y_plus_plus",
        );
        assert_within_tolerance(
            updated_state.y_plus_minus,
            1.0,
            DECAY_TOLERANCE,
            "y_plus_minus",
        );
        assert_within_tolerance(
            updated_state.y_minus_plus,
            0.9,
            DECAY_TOLERANCE,
            "y_minus_plus",
        );
        assert_within_tolerance(
            updated_state.y_minus_minus,
            1.7,
            DECAY_TOLERANCE,
            "y_minus_minus",
        );
    }

    #[test]
    fn interleaved_event_jump_sequence_composes_with_drift() {
        let state = HawkesExcitationState {
            y_plus_plus: 1.0,
            y_plus_minus: 2.0,
            y_minus_plus: 3.0,
            y_minus_minus: 4.0,
        };
        let alpha = HawkesKernelLoadings {
            plus_plus: 0.6,
            plus_minus: 0.1,
            minus_plus: 0.2,
            minus_minus: 0.7,
        };

        let updated_state = state
            .apply_shared_decay_drift(1.0, 0.5)
            .apply_endogenous_event_jump(HawkesEventSide::Buy, alpha)
            .apply_endogenous_event_jump(HawkesEventSide::Sell, alpha)
            .apply_endogenous_event_jump(HawkesEventSide::Buy, alpha);

        let decay_factor = (-0.5f64).exp();

        assert_within_tolerance(
            updated_state.y_plus_plus,
            1.0 * decay_factor + 1.2,
            DECAY_TOLERANCE,
            "y_plus_plus",
        );
        assert_within_tolerance(
            updated_state.y_plus_minus,
            2.0 * decay_factor + 0.1,
            DECAY_TOLERANCE,
            "y_plus_minus",
        );
        assert_within_tolerance(
            updated_state.y_minus_plus,
            3.0 * decay_factor + 0.4,
            DECAY_TOLERANCE,
            "y_minus_plus",
        );
        assert_within_tolerance(
            updated_state.y_minus_minus,
            4.0 * decay_factor + 0.7,
            DECAY_TOLERANCE,
            "y_minus_minus",
        );
    }

    #[test]
    fn event_side_payload_parsing_rejects_unsupported_value() {
        let error =
            HawkesEventSide::try_from("neutral").expect_err("unsupported side payload must fail");

        assert_eq!(
            error,
            HawkesEventSidePayloadError {
                value: "neutral".to_owned(),
            }
        );
        assert_eq!(
            error.to_string(),
            "unsupported event side 'neutral', expected one of: buy, sell"
        );
    }

    #[test]
    fn payload_based_event_jump_returns_error_for_unsupported_side() {
        let state = HawkesExcitationState {
            y_plus_plus: 0.0,
            y_plus_minus: 0.0,
            y_minus_plus: 0.0,
            y_minus_minus: 0.0,
        };
        let alpha = HawkesKernelLoadings {
            plus_plus: 0.2,
            plus_minus: 0.3,
            minus_plus: 0.4,
            minus_minus: 0.6,
        };

        let error = state
            .apply_endogenous_event_jump_from_payload("BUY", alpha)
            .expect_err("unsupported payload must fail");
        assert_eq!(
            error,
            HawkesEventSidePayloadError {
                value: "BUY".to_owned(),
            }
        );
    }

    #[test]
    fn loadings_from_target_components_preserve_model_ordering() {
        let target_plus = HawkesSourceComponents::new(0.1, 0.2);
        let target_minus = HawkesSourceComponents::new(0.3, 0.4);

        let loadings = HawkesKernelLoadings::from_target_components(target_plus, target_minus);

        assert_eq!(
            loadings,
            HawkesKernelLoadings {
                plus_plus: 0.1,
                plus_minus: 0.2,
                minus_plus: 0.3,
                minus_minus: 0.4,
            }
        );
        assert_eq!(loadings.target_plus_components(), target_plus);
        assert_eq!(loadings.target_minus_components(), target_minus);
        assert_eq!(loadings.as_model_ordered_array(), [0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn kernel_parameter_construction_accepts_valid_payload() {
        let alpha = HawkesKernelLoadings {
            plus_plus: 0.6,
            plus_minus: 0.1,
            minus_plus: 0.2,
            minus_minus: 0.7,
        };
        let rho = HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.05,
            minus_plus: 0.12,
            minus_minus: 0.0,
        };

        let parameters = HawkesKernelParameters::try_new(1.25, alpha, rho)
            .expect("valid Hawkes kernel payload should construct");

        assert_eq!(parameters.beta, 1.25);
        assert_eq!(parameters.alpha, alpha);
        assert_eq!(parameters.rho, rho);
    }

    #[test]
    fn kernel_parameter_construction_rejects_non_positive_or_non_finite_beta() {
        let loadings = HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.0,
            minus_plus: 0.0,
            minus_minus: 0.0,
        };

        let zero_beta_error =
            HawkesKernelParameters::try_new(0.0, loadings, loadings).expect_err("beta=0 must fail");
        assert_eq!(
            zero_beta_error,
            HawkesKernelParameterError::InvalidBeta { value: 0.0 }
        );

        let nan_beta_error = HawkesKernelParameters::try_new(f64::NAN, loadings, loadings)
            .expect_err("non-finite beta must fail");
        assert!(matches!(
            nan_beta_error,
            HawkesKernelParameterError::InvalidBeta { value } if value.is_nan()
        ));
    }

    #[test]
    fn kernel_parameter_construction_rejects_invalid_alpha_component() {
        let alpha = HawkesKernelLoadings {
            plus_plus: 0.2,
            plus_minus: -0.01,
            minus_plus: 0.1,
            minus_minus: 0.1,
        };
        let rho = HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.0,
            minus_plus: 0.0,
            minus_minus: 0.0,
        };

        let error =
            HawkesKernelParameters::try_new(0.9, alpha, rho).expect_err("negative alpha must fail");

        assert_eq!(
            error,
            HawkesKernelParameterError::InvalidLoadingComponent {
                field: "alpha.plus_minus",
                value: -0.01,
            }
        );
    }

    #[test]
    fn kernel_parameter_construction_rejects_invalid_rho_component() {
        let alpha = HawkesKernelLoadings {
            plus_plus: 0.2,
            plus_minus: 0.1,
            minus_plus: 0.1,
            minus_minus: 0.1,
        };
        let rho = HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.0,
            minus_plus: 0.0,
            minus_minus: f64::INFINITY,
        };

        let error =
            HawkesKernelParameters::try_new(0.9, alpha, rho).expect_err("non-finite rho must fail");

        assert_eq!(
            error,
            HawkesKernelParameterError::InvalidLoadingComponent {
                field: "rho.minus_minus",
                value: f64::INFINITY,
            }
        );
    }
}
