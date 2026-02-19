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
        HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameterError,
        HawkesKernelParameters, HawkesSourceComponents,
    };

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
