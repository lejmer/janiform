use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use super::HawkesSourceComponents;
use super::parameters::{
    HawkesKernelParameterError, derive_self_to_cross_ratio, validate_non_negative_loadings,
};
/// Canonical 4-component loading map used for `alpha_ij` and `rho_ij`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

/// Structured base-plus-delta parameterization for endogenous `alpha`.
///
/// Mapping:
/// - `alpha_++ = a_self`
/// - `alpha_+- = a_cross`
/// - `alpha_-+ = a_cross + delta_cross`
/// - `alpha_-- = a_self + delta_self`
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HawkesStructuredAlphaParameterization {
    pub a_self: f64,
    pub a_cross: f64,
    pub delta_self: f64,
    pub delta_cross: f64,
}

impl HawkesStructuredAlphaParameterization {
    pub const fn new(a_self: f64, a_cross: f64, delta_self: f64, delta_cross: f64) -> Self {
        Self {
            a_self,
            a_cross,
            delta_self,
            delta_cross,
        }
    }
}

/// Structured base-plus-delta parameterization for exogenous loading `rho`.
///
/// Mapping:
/// - `rho_++ = r_self`
/// - `rho_+- = r_cross`
/// - `rho_-+ = r_cross + delta_cross_r`
/// - `rho_-- = r_self + delta_self_r`
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HawkesStructuredRhoParameterization {
    pub r_self: f64,
    pub r_cross: f64,
    pub delta_self_r: f64,
    pub delta_cross_r: f64,
}

impl HawkesStructuredRhoParameterization {
    pub const fn new(r_self: f64, r_cross: f64, delta_self_r: f64, delta_cross_r: f64) -> Self {
        Self {
            r_self,
            r_cross,
            delta_self_r,
            delta_cross_r,
        }
    }
}

/// Optional self-dominance policy for structured `rho` validation.
///
/// A policy value `m` enforces:
/// - `rho_++ / rho_+- >= m`
/// - `rho_-- / rho_-+ >= m`
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HawkesStructuredRhoSelfDominancePolicy {
    pub minimum_self_to_cross_ratio: f64,
}

impl HawkesStructuredRhoSelfDominancePolicy {
    pub const fn new(minimum_self_to_cross_ratio: f64) -> Self {
        Self {
            minimum_self_to_cross_ratio,
        }
    }
}

/// Machine-readable validation output for structured `rho` derivation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct HawkesStructuredRhoValidationOutput {
    pub derived_rho: HawkesKernelLoadings,
    pub target_plus_self_to_cross_ratio: f64,
    pub target_minus_self_to_cross_ratio: f64,
    pub applied_self_dominance_policy: Option<HawkesStructuredRhoSelfDominancePolicy>,
}

/// Structured validation failures for structured base-plus-delta `alpha` inputs.
#[derive(Debug, Clone, PartialEq)]
pub enum HawkesStructuredAlphaParameterError {
    InvalidASelf {
        value: f64,
    },
    InvalidACross {
        value: f64,
    },
    InvalidDeltaSelf {
        value: f64,
    },
    InvalidDeltaCross {
        value: f64,
    },
    InvalidDerivedComponent {
        field: &'static str,
        value: f64,
    },
    InvalidSelfAsymmetry {
        alpha_minus_minus: f64,
        alpha_plus_plus: f64,
    },
    InvalidCrossAsymmetry {
        alpha_minus_plus: f64,
        alpha_plus_minus: f64,
    },
}

impl Display for HawkesStructuredAlphaParameterError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidASelf { value } => write!(
                formatter,
                "a_self must be finite and strictly positive, found {value}"
            ),
            Self::InvalidACross { value } => write!(
                formatter,
                "a_cross must be finite and non-negative, found {value}"
            ),
            Self::InvalidDeltaSelf { value } => write!(
                formatter,
                "delta_self must be finite and non-negative to satisfy alpha_-- >= alpha_++, found {value}"
            ),
            Self::InvalidDeltaCross { value } => write!(
                formatter,
                "delta_cross must be finite and non-negative to satisfy alpha_-+ >= alpha_+-, found {value}"
            ),
            Self::InvalidDerivedComponent { field, value } => write!(
                formatter,
                "derived alpha component {field} must be finite and non-negative, found {value}"
            ),
            Self::InvalidSelfAsymmetry {
                alpha_minus_minus,
                alpha_plus_plus,
            } => write!(
                formatter,
                "structured alpha must satisfy alpha_-- >= alpha_++, found alpha_--={alpha_minus_minus} and alpha_++={alpha_plus_plus}"
            ),
            Self::InvalidCrossAsymmetry {
                alpha_minus_plus,
                alpha_plus_minus,
            } => write!(
                formatter,
                "structured alpha must satisfy alpha_-+ >= alpha_+-, found alpha_-+={alpha_minus_plus} and alpha_+-={alpha_plus_minus}"
            ),
        }
    }
}

impl Error for HawkesStructuredAlphaParameterError {}

/// Structured validation failures for structured base-plus-delta `rho` inputs.
#[derive(Debug, Clone, PartialEq)]
pub enum HawkesStructuredRhoParameterError {
    InvalidRSelf {
        value: f64,
    },
    InvalidRCross {
        value: f64,
    },
    InvalidDeltaSelfR {
        value: f64,
    },
    InvalidDeltaCrossR {
        value: f64,
    },
    InvalidDerivedComponent {
        field: &'static str,
        value: f64,
    },
    InvalidSelfAsymmetry {
        rho_minus_minus: f64,
        rho_plus_plus: f64,
    },
    InvalidCrossAsymmetry {
        rho_minus_plus: f64,
        rho_plus_minus: f64,
    },
    InvalidSelfDominanceRatioThreshold {
        value: f64,
    },
    SelfDominancePolicyViolation {
        minimum_self_to_cross_ratio: f64,
        target_plus_self_to_cross_ratio: f64,
        target_minus_self_to_cross_ratio: f64,
    },
}

impl Display for HawkesStructuredRhoParameterError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRSelf { value } => write!(
                formatter,
                "r_self must be finite and strictly positive, found {value}"
            ),
            Self::InvalidRCross { value } => write!(
                formatter,
                "r_cross must be finite and non-negative, found {value}"
            ),
            Self::InvalidDeltaSelfR { value } => write!(
                formatter,
                "delta_self_r must be finite and non-negative to satisfy rho_-- >= rho_++, found {value}"
            ),
            Self::InvalidDeltaCrossR { value } => write!(
                formatter,
                "delta_cross_r must be finite and non-negative to satisfy rho_-+ >= rho_+-, found {value}"
            ),
            Self::InvalidDerivedComponent { field, value } => write!(
                formatter,
                "derived rho component {field} must be finite and non-negative, found {value}"
            ),
            Self::InvalidSelfAsymmetry {
                rho_minus_minus,
                rho_plus_plus,
            } => write!(
                formatter,
                "structured rho must satisfy rho_-- >= rho_++, found rho_--={rho_minus_minus} and rho_++={rho_plus_plus}"
            ),
            Self::InvalidCrossAsymmetry {
                rho_minus_plus,
                rho_plus_minus,
            } => write!(
                formatter,
                "structured rho must satisfy rho_-+ >= rho_+-, found rho_-+={rho_minus_plus} and rho_+-={rho_plus_minus}"
            ),
            Self::InvalidSelfDominanceRatioThreshold { value } => write!(
                formatter,
                "minimum_self_to_cross_ratio must be finite and at least 1.0, found {value}"
            ),
            Self::SelfDominancePolicyViolation {
                minimum_self_to_cross_ratio,
                target_plus_self_to_cross_ratio,
                target_minus_self_to_cross_ratio,
            } => write!(
                formatter,
                "structured rho self-dominance policy failed with minimum_self_to_cross_ratio={minimum_self_to_cross_ratio}, target_plus_self_to_cross_ratio={target_plus_self_to_cross_ratio}, target_minus_self_to_cross_ratio={target_minus_self_to_cross_ratio}"
            ),
        }
    }
}

impl Error for HawkesStructuredRhoParameterError {}

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

    /// Validate and derive canonical `alpha` loadings from base-plus-delta inputs.
    ///
    /// The returned loadings preserve canonical model ordering `[++, +-, -+, --]`
    /// through `HawkesKernelLoadings::as_model_ordered_array`.
    pub fn try_from_structured_alpha(
        structured_alpha: HawkesStructuredAlphaParameterization,
    ) -> Result<Self, HawkesStructuredAlphaParameterError> {
        if !structured_alpha.a_self.is_finite() || structured_alpha.a_self <= 0.0 {
            return Err(HawkesStructuredAlphaParameterError::InvalidASelf {
                value: structured_alpha.a_self,
            });
        }

        if !structured_alpha.a_cross.is_finite() || structured_alpha.a_cross < 0.0 {
            return Err(HawkesStructuredAlphaParameterError::InvalidACross {
                value: structured_alpha.a_cross,
            });
        }

        if !structured_alpha.delta_self.is_finite() || structured_alpha.delta_self < 0.0 {
            return Err(HawkesStructuredAlphaParameterError::InvalidDeltaSelf {
                value: structured_alpha.delta_self,
            });
        }

        if !structured_alpha.delta_cross.is_finite() || structured_alpha.delta_cross < 0.0 {
            return Err(HawkesStructuredAlphaParameterError::InvalidDeltaCross {
                value: structured_alpha.delta_cross,
            });
        }

        let alpha_plus_plus = structured_alpha.a_self;
        let alpha_plus_minus = structured_alpha.a_cross;
        let alpha_minus_plus = structured_alpha.a_cross + structured_alpha.delta_cross;
        let alpha_minus_minus = structured_alpha.a_self + structured_alpha.delta_self;

        let derived_loadings = Self {
            plus_plus: alpha_plus_plus,
            plus_minus: alpha_plus_minus,
            minus_plus: alpha_minus_plus,
            minus_minus: alpha_minus_minus,
        };

        validate_non_negative_loadings(
            derived_loadings,
            [
                "alpha.plus_plus",
                "alpha.plus_minus",
                "alpha.minus_plus",
                "alpha.minus_minus",
            ],
        )
        .map_err(|error| match error {
            HawkesKernelParameterError::InvalidLoadingComponent { field, value } => {
                HawkesStructuredAlphaParameterError::InvalidDerivedComponent { field, value }
            }
            _ => unreachable!("structured alpha derivation only validates loading components"),
        })?;

        if derived_loadings.minus_minus < derived_loadings.plus_plus {
            return Err(HawkesStructuredAlphaParameterError::InvalidSelfAsymmetry {
                alpha_minus_minus: derived_loadings.minus_minus,
                alpha_plus_plus: derived_loadings.plus_plus,
            });
        }

        if derived_loadings.minus_plus < derived_loadings.plus_minus {
            return Err(HawkesStructuredAlphaParameterError::InvalidCrossAsymmetry {
                alpha_minus_plus: derived_loadings.minus_plus,
                alpha_plus_minus: derived_loadings.plus_minus,
            });
        }

        Ok(derived_loadings)
    }

    /// Validate and derive canonical `rho` loadings from base-plus-delta inputs.
    ///
    /// The returned output includes derived loadings and machine-readable ratio
    /// diagnostics for optional self-dominance policy enforcement.
    pub fn validate_structured_rho(
        structured_rho: HawkesStructuredRhoParameterization,
        self_dominance_policy: Option<HawkesStructuredRhoSelfDominancePolicy>,
    ) -> Result<HawkesStructuredRhoValidationOutput, HawkesStructuredRhoParameterError> {
        if !structured_rho.r_self.is_finite() || structured_rho.r_self <= 0.0 {
            return Err(HawkesStructuredRhoParameterError::InvalidRSelf {
                value: structured_rho.r_self,
            });
        }

        if !structured_rho.r_cross.is_finite() || structured_rho.r_cross < 0.0 {
            return Err(HawkesStructuredRhoParameterError::InvalidRCross {
                value: structured_rho.r_cross,
            });
        }

        if !structured_rho.delta_self_r.is_finite() || structured_rho.delta_self_r < 0.0 {
            return Err(HawkesStructuredRhoParameterError::InvalidDeltaSelfR {
                value: structured_rho.delta_self_r,
            });
        }

        if !structured_rho.delta_cross_r.is_finite() || structured_rho.delta_cross_r < 0.0 {
            return Err(HawkesStructuredRhoParameterError::InvalidDeltaCrossR {
                value: structured_rho.delta_cross_r,
            });
        }

        let rho_plus_plus = structured_rho.r_self;
        let rho_plus_minus = structured_rho.r_cross;
        let rho_minus_plus = structured_rho.r_cross + structured_rho.delta_cross_r;
        let rho_minus_minus = structured_rho.r_self + structured_rho.delta_self_r;

        let derived_rho = Self {
            plus_plus: rho_plus_plus,
            plus_minus: rho_plus_minus,
            minus_plus: rho_minus_plus,
            minus_minus: rho_minus_minus,
        };

        validate_non_negative_loadings(
            derived_rho,
            [
                "rho.plus_plus",
                "rho.plus_minus",
                "rho.minus_plus",
                "rho.minus_minus",
            ],
        )
        .map_err(|error| match error {
            HawkesKernelParameterError::InvalidLoadingComponent { field, value } => {
                HawkesStructuredRhoParameterError::InvalidDerivedComponent { field, value }
            }
            _ => unreachable!("structured rho derivation only validates loading components"),
        })?;

        if derived_rho.minus_minus < derived_rho.plus_plus {
            return Err(HawkesStructuredRhoParameterError::InvalidSelfAsymmetry {
                rho_minus_minus: derived_rho.minus_minus,
                rho_plus_plus: derived_rho.plus_plus,
            });
        }

        if derived_rho.minus_plus < derived_rho.plus_minus {
            return Err(HawkesStructuredRhoParameterError::InvalidCrossAsymmetry {
                rho_minus_plus: derived_rho.minus_plus,
                rho_plus_minus: derived_rho.plus_minus,
            });
        }

        let target_plus_self_to_cross_ratio =
            derive_self_to_cross_ratio(derived_rho.plus_plus, derived_rho.plus_minus);
        let target_minus_self_to_cross_ratio =
            derive_self_to_cross_ratio(derived_rho.minus_minus, derived_rho.minus_plus);

        if let Some(policy) = self_dominance_policy {
            if !policy.minimum_self_to_cross_ratio.is_finite()
                || policy.minimum_self_to_cross_ratio < 1.0
            {
                return Err(
                    HawkesStructuredRhoParameterError::InvalidSelfDominanceRatioThreshold {
                        value: policy.minimum_self_to_cross_ratio,
                    },
                );
            }

            if target_plus_self_to_cross_ratio < policy.minimum_self_to_cross_ratio
                || target_minus_self_to_cross_ratio < policy.minimum_self_to_cross_ratio
            {
                return Err(
                    HawkesStructuredRhoParameterError::SelfDominancePolicyViolation {
                        minimum_self_to_cross_ratio: policy.minimum_self_to_cross_ratio,
                        target_plus_self_to_cross_ratio,
                        target_minus_self_to_cross_ratio,
                    },
                );
            }
        }

        Ok(HawkesStructuredRhoValidationOutput {
            derived_rho,
            target_plus_self_to_cross_ratio,
            target_minus_self_to_cross_ratio,
            applied_self_dominance_policy: self_dominance_policy,
        })
    }

    /// Validate and derive canonical `rho` loadings from base-plus-delta inputs.
    pub fn try_from_structured_rho(
        structured_rho: HawkesStructuredRhoParameterization,
        self_dominance_policy: Option<HawkesStructuredRhoSelfDominancePolicy>,
    ) -> Result<Self, HawkesStructuredRhoParameterError> {
        Self::validate_structured_rho(structured_rho, self_dominance_policy)
            .map(|validation_output| validation_output.derived_rho)
    }
}
