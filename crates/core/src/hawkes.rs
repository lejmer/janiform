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

use serde::Deserialize;
use serde::Serialize;

use crate::hourly_factors::HourlyFactorContractOutput;

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

/// Buy and sell intensity pair with explicit side naming.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HawkesSideIntensities {
    pub lambda_plus: f64,
    pub lambda_minus: f64,
}

impl HawkesSideIntensities {
    pub const fn scale(self, seasonality_factor: f64) -> Self {
        Self {
            lambda_plus: self.lambda_plus * seasonality_factor,
            lambda_minus: self.lambda_minus * seasonality_factor,
        }
    }
}

/// Full state-to-intensity projection output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HawkesIntensityProjection {
    pub deseasonalized: HawkesSideIntensities,
    pub seasonality_scaled: HawkesSideIntensities,
}

/// Structured failure when resolving seasonality from hourly contract intervals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HawkesSeasonalityIntervalError {
    pub interval_index: usize,
    pub interval_count: usize,
}

impl Display for HawkesSeasonalityIntervalError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "interval_index {} is out of range for hourly seasonality factor count {}",
            self.interval_index, self.interval_count
        )
    }
}

impl Error for HawkesSeasonalityIntervalError {}

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

/// Structured validation failure for exogenous news severity marks.
#[derive(Debug, Clone, PartialEq)]
pub struct HawkesNewsSeverityMarkError {
    pub value: f64,
}

impl Display for HawkesNewsSeverityMarkError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "news severity mark must be finite and non-negative, found {}",
            self.value
        )
    }
}

impl Error for HawkesNewsSeverityMarkError {}

/// Canonical transition step that composes drift and optional jumps.
///
/// Call-order policy is explicit and deterministic:
/// 1. shared-decay drift over `delta_t`
/// 2. optional endogenous jump by `endogenous_event_side`
/// 3. optional exogenous jump by `exogenous_news_severity_mark`
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HawkesTransitionStep {
    pub delta_t: f64,
    pub endogenous_event_side: Option<HawkesEventSide>,
    pub exogenous_news_severity_mark: Option<f64>,
}

impl HawkesTransitionStep {
    pub const fn new(
        delta_t: f64,
        endogenous_event_side: Option<HawkesEventSide>,
        exogenous_news_severity_mark: Option<f64>,
    ) -> Self {
        Self {
            delta_t,
            endogenous_event_side,
            exogenous_news_severity_mark,
        }
    }
}

/// Structured validation failures for canonical transition composition helpers.
#[derive(Debug, Clone, PartialEq)]
pub enum HawkesTransitionError {
    InvalidDeltaT { value: f64 },
    InvalidBeta { value: f64 },
    InvalidNewsSeverityMark { source: HawkesNewsSeverityMarkError },
}

impl Display for HawkesTransitionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDeltaT { value } => write!(
                formatter,
                "delta_t must be finite and non-negative for transition composition, found {value}"
            ),
            Self::InvalidBeta { value } => write!(
                formatter,
                "beta must be finite and strictly positive for transition composition, found {value}"
            ),
            Self::InvalidNewsSeverityMark { source } => write!(formatter, "{source}"),
        }
    }
}

impl Error for HawkesTransitionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidNewsSeverityMark { source } => Some(source),
            _ => None,
        }
    }
}

/// Canonical 4D Hawkes excitation state with explicit `Yij` components.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

    /// Project de-seasonalized intensities from the 4D excitation state.
    ///
    /// Equations:
    /// - `tilde_lambda_plus = lambda_bar + Y++ + Y+-`
    /// - `tilde_lambda_minus = lambda_bar + Y-+ + Y--`
    pub const fn project_deseasonalized_intensities(
        self,
        lambda_bar: f64,
    ) -> HawkesSideIntensities {
        HawkesSideIntensities {
            lambda_plus: lambda_bar + self.y_plus_plus + self.y_plus_minus,
            lambda_minus: lambda_bar + self.y_minus_plus + self.y_minus_minus,
        }
    }

    /// Project both de-seasonalized and seasonality-scaled intensities.
    ///
    /// Scaling equations:
    /// - `lambda_plus = s(t) * tilde_lambda_plus`
    /// - `lambda_minus = s(t) * tilde_lambda_minus`
    pub const fn project_intensities(
        self,
        lambda_bar: f64,
        seasonality_factor: f64,
    ) -> HawkesIntensityProjection {
        let deseasonalized = self.project_deseasonalized_intensities(lambda_bar);
        HawkesIntensityProjection {
            deseasonalized,
            seasonality_scaled: deseasonalized.scale(seasonality_factor),
        }
    }

    /// Project intensities for an interval index using shared hourly factor contract `s_h`.
    pub fn project_intensities_for_hourly_interval(
        self,
        lambda_bar: f64,
        hourly_factor_contract: &HourlyFactorContractOutput,
        interval_index: usize,
    ) -> Result<HawkesIntensityProjection, HawkesSeasonalityIntervalError> {
        let interval_count = hourly_factor_contract.s_h.len();
        let seasonality_factor = hourly_factor_contract
            .s_h
            .get(interval_index)
            .copied()
            .ok_or(HawkesSeasonalityIntervalError {
                interval_index,
                interval_count,
            })?;

        Ok(self.project_intensities(lambda_bar, seasonality_factor))
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

    /// Apply an exogenous news loading jump using `rho * U`.
    ///
    /// Mapping:
    /// - `delta_y_ij = rho_ij * severity_mark`
    ///
    /// Input semantics:
    /// - `severity_mark` must be finite and non-negative
    /// - `severity_mark == 0.0` returns the input state unchanged exactly
    pub fn apply_exogenous_news_loading_jump(
        self,
        rho: HawkesKernelLoadings,
        severity_mark: f64,
    ) -> Result<Self, HawkesNewsSeverityMarkError> {
        if !severity_mark.is_finite() || severity_mark < 0.0 {
            return Err(HawkesNewsSeverityMarkError {
                value: severity_mark,
            });
        }

        if severity_mark == 0.0 {
            return Ok(self);
        }

        Ok(Self {
            y_plus_plus: self.y_plus_plus + rho.plus_plus * severity_mark,
            y_plus_minus: self.y_plus_minus + rho.plus_minus * severity_mark,
            y_minus_plus: self.y_minus_plus + rho.minus_plus * severity_mark,
            y_minus_minus: self.y_minus_minus + rho.minus_minus * severity_mark,
        })
    }

    /// Apply one canonical transition step using deterministic call order.
    ///
    /// Call order:
    /// 1. shared-decay drift over `transition_step.delta_t`
    /// 2. optional endogenous jump using `kernel_parameters.alpha`
    /// 3. optional exogenous jump using `kernel_parameters.rho`
    pub fn apply_transition_step(
        self,
        kernel_parameters: HawkesKernelParameters,
        transition_step: HawkesTransitionStep,
    ) -> Result<Self, HawkesTransitionError> {
        if !transition_step.delta_t.is_finite() || transition_step.delta_t < 0.0 {
            return Err(HawkesTransitionError::InvalidDeltaT {
                value: transition_step.delta_t,
            });
        }
        if !kernel_parameters.beta.is_finite() || kernel_parameters.beta <= 0.0 {
            return Err(HawkesTransitionError::InvalidBeta {
                value: kernel_parameters.beta,
            });
        }

        let mut next_state =
            self.apply_shared_decay_drift(kernel_parameters.beta, transition_step.delta_t);

        if let Some(endogenous_event_side) = transition_step.endogenous_event_side {
            next_state = next_state
                .apply_endogenous_event_jump(endogenous_event_side, kernel_parameters.alpha);
        }

        if let Some(exogenous_news_severity_mark) = transition_step.exogenous_news_severity_mark {
            return next_state
                .apply_exogenous_news_loading_jump(
                    kernel_parameters.rho,
                    exogenous_news_severity_mark,
                )
                .map_err(|source| HawkesTransitionError::InvalidNewsSeverityMark { source });
        }

        Ok(next_state)
    }

    /// Apply a transition sequence step-by-step using `apply_transition_step`.
    pub fn apply_transition_sequence(
        self,
        kernel_parameters: HawkesKernelParameters,
        transition_steps: &[HawkesTransitionStep],
    ) -> Result<Self, HawkesTransitionError> {
        let mut next_state = self;

        for transition_step in transition_steps {
            next_state = next_state.apply_transition_step(kernel_parameters, *transition_step)?;
        }

        Ok(next_state)
    }
}

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

/// Shared-kernel parameter payload for the 4D Hawkes excitation dynamics.
///
/// Mapping to equations:
/// - `lambda_bar` is the shared de-seasonalized baseline intensity for buy and sell
/// - `beta` is shared exponential decay in `dY_t^{ij} = -beta * Y_t^{ij} dt + ...`
/// - `alpha` stores endogenous excitation loadings `alpha_ij`
/// - `rho` stores exogenous loading terms `rho_ij`
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct HawkesKernelParameters {
    pub lambda_bar: f64,
    pub beta: f64,
    pub alpha: HawkesKernelLoadings,
    pub rho: HawkesKernelLoadings,
}

pub const HAWKES_STRICT_STATIONARITY_THRESHOLD: f64 = 1.0;
pub const HAWKES_DEFAULT_STABILITY_NUMERIC_TOLERANCE: f64 = 0.0;
pub const HAWKES_KERNEL_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION: &str =
    "validation.hawkes.kernel_parameter_report.v1";
pub const HAWKES_KERNEL_PARAMETER_VALIDATION_DETERMINISM_POLICY: &str =
    "stable_check_order_and_aggregated_failures.v1";

/// Unified pass or fail status used by report-level and check-level validation payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HawkesKernelValidationStatus {
    Pass,
    Fail,
}

impl HawkesKernelValidationStatus {
    const fn from_bool(is_valid: bool) -> Self {
        if is_valid { Self::Pass } else { Self::Fail }
    }
}

/// High-level validation category used to group check diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HawkesKernelParameterCheckCategory {
    Positivity,
    Structural,
    Stability,
}

/// Per-check validation diagnostic entry for optimizer and CLI consumers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HawkesKernelParameterCheck {
    pub check_id: &'static str,
    pub status: HawkesKernelValidationStatus,
    pub category: HawkesKernelParameterCheckCategory,
    pub field: &'static str,
    pub comparison_field: Option<&'static str>,
    pub constraint: HawkesKernelParameterConstraint,
    pub observed_value: f64,
    pub threshold_value: Option<f64>,
    pub expected_condition: &'static str,
    pub diagnostics: String,
    pub remediation_hint: &'static str,
}

/// Summary metadata for aggregated validation reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HawkesKernelParameterValidationSummary {
    pub total_check_count: usize,
    pub passed_check_count: usize,
    pub failed_check_count: usize,
}

/// Machine-readable stability output for the de-seasonalized `Alpha / beta` kernel.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct HawkesSpectralRadiusStabilityCheck {
    pub spectral_radius_alpha_over_beta: f64,
    pub strict_stationarity_threshold: f64,
    pub numeric_tolerance: f64,
    pub effective_stationarity_threshold: f64,
    pub is_stable: bool,
}

impl HawkesKernelParameters {
    /// Run pure domain validation without constructing a kernel payload.
    ///
    /// This helper is intended for pre-checking candidate vectors before
    /// expensive calibration loops.
    pub fn validate_domain(
        lambda_bar: f64,
        beta: f64,
        alpha: HawkesKernelLoadings,
        rho: HawkesKernelLoadings,
    ) -> HawkesKernelParameterValidationReport {
        Self::validate_domain_with_stability_tolerance(
            lambda_bar,
            beta,
            alpha,
            rho,
            HAWKES_DEFAULT_STABILITY_NUMERIC_TOLERANCE,
        )
    }

    /// Run domain validation with an explicit numerical tolerance around
    /// the strict stationarity threshold for `rho(Alpha / beta) < 1`.
    pub fn validate_domain_with_stability_tolerance(
        lambda_bar: f64,
        beta: f64,
        alpha: HawkesKernelLoadings,
        rho: HawkesKernelLoadings,
        stability_numeric_tolerance: f64,
    ) -> HawkesKernelParameterValidationReport {
        let mut checks: Vec<HawkesKernelParameterCheck> = Vec::new();

        validate_strictly_positive_check(
            &mut checks,
            "positivity.lambda_bar",
            HawkesKernelParameterCheckCategory::Positivity,
            "lambda_bar",
            lambda_bar,
            "lambda_bar must be finite and strictly positive",
            "increase lambda_bar to a finite value greater than 0",
        );

        let beta_is_valid = validate_strictly_positive_check(
            &mut checks,
            "positivity.beta",
            HawkesKernelParameterCheckCategory::Positivity,
            "beta",
            beta,
            "beta must be finite and strictly positive",
            "set beta to a finite value greater than 0",
        );

        let alpha_plus_plus_is_valid = validate_non_negative_check(
            &mut checks,
            "positivity.alpha.plus_plus",
            HawkesKernelParameterCheckCategory::Positivity,
            "alpha.plus_plus",
            alpha.plus_plus,
            "alpha.plus_plus must be finite and non-negative",
            "set alpha.plus_plus to a finite value greater than or equal to 0",
        );
        let alpha_plus_minus_is_valid = validate_non_negative_check(
            &mut checks,
            "positivity.alpha.plus_minus",
            HawkesKernelParameterCheckCategory::Positivity,
            "alpha.plus_minus",
            alpha.plus_minus,
            "alpha.plus_minus must be finite and non-negative",
            "set alpha.plus_minus to a finite value greater than or equal to 0",
        );
        let alpha_minus_plus_is_valid = validate_non_negative_check(
            &mut checks,
            "positivity.alpha.minus_plus",
            HawkesKernelParameterCheckCategory::Positivity,
            "alpha.minus_plus",
            alpha.minus_plus,
            "alpha.minus_plus must be finite and non-negative",
            "set alpha.minus_plus to a finite value greater than or equal to 0",
        );
        let alpha_minus_minus_is_valid = validate_non_negative_check(
            &mut checks,
            "positivity.alpha.minus_minus",
            HawkesKernelParameterCheckCategory::Positivity,
            "alpha.minus_minus",
            alpha.minus_minus,
            "alpha.minus_minus must be finite and non-negative",
            "set alpha.minus_minus to a finite value greater than or equal to 0",
        );
        let alpha_is_valid = alpha_plus_plus_is_valid
            && alpha_plus_minus_is_valid
            && alpha_minus_plus_is_valid
            && alpha_minus_minus_is_valid;

        validate_non_negative_check(
            &mut checks,
            "positivity.rho.plus_plus",
            HawkesKernelParameterCheckCategory::Positivity,
            "rho.plus_plus",
            rho.plus_plus,
            "rho.plus_plus must be finite and non-negative",
            "set rho.plus_plus to a finite value greater than or equal to 0",
        );
        validate_non_negative_check(
            &mut checks,
            "positivity.rho.plus_minus",
            HawkesKernelParameterCheckCategory::Positivity,
            "rho.plus_minus",
            rho.plus_minus,
            "rho.plus_minus must be finite and non-negative",
            "set rho.plus_minus to a finite value greater than or equal to 0",
        );
        validate_non_negative_check(
            &mut checks,
            "positivity.rho.minus_plus",
            HawkesKernelParameterCheckCategory::Positivity,
            "rho.minus_plus",
            rho.minus_plus,
            "rho.minus_plus must be finite and non-negative",
            "set rho.minus_plus to a finite value greater than or equal to 0",
        );
        validate_non_negative_check(
            &mut checks,
            "positivity.rho.minus_minus",
            HawkesKernelParameterCheckCategory::Positivity,
            "rho.minus_minus",
            rho.minus_minus,
            "rho.minus_minus must be finite and non-negative",
            "set rho.minus_minus to a finite value greater than or equal to 0",
        );

        let stability_numeric_tolerance_is_valid = validate_non_negative_check(
            &mut checks,
            "stability.numeric_tolerance",
            HawkesKernelParameterCheckCategory::Stability,
            "stability.numeric_tolerance",
            stability_numeric_tolerance,
            "stability numeric tolerance must be finite and non-negative",
            "set stability.numeric_tolerance to a finite value greater than or equal to 0",
        );

        if beta_is_valid && alpha_is_valid && stability_numeric_tolerance_is_valid {
            let stability_check =
                compute_spectral_radius_stability_check(alpha, beta, stability_numeric_tolerance);

            push_validation_check(
                &mut checks,
                "stability.alpha_over_beta.spectral_radius",
                HawkesKernelValidationStatus::from_bool(stability_check.is_stable),
                HawkesKernelParameterCheckCategory::Stability,
                "alpha_over_beta.spectral_radius",
                Some("stability.effective_stationarity_threshold"),
                HawkesKernelParameterConstraint::SpectralRadiusStrictlyBelowOne,
                stability_check.spectral_radius_alpha_over_beta,
                Some(stability_check.effective_stationarity_threshold),
                "spectral radius must be strictly below the effective stationarity threshold",
                format!(
                    "spectral radius={} and effective_threshold={} (strict_threshold={}, numeric_tolerance={})",
                    stability_check.spectral_radius_alpha_over_beta,
                    stability_check.effective_stationarity_threshold,
                    stability_check.strict_stationarity_threshold,
                    stability_check.numeric_tolerance
                ),
                "reduce alpha magnitudes or increase beta so spectral radius(alpha / beta) is below 1",
            );
        }

        HawkesKernelParameterValidationReport::from_checks(checks)
    }

    /// Run structured base-plus-delta validation with default stability tolerance.
    pub fn validate_structured_domain(
        lambda_bar: f64,
        beta: f64,
        structured_alpha: HawkesStructuredAlphaParameterization,
        structured_rho: HawkesStructuredRhoParameterization,
        self_dominance_policy: Option<HawkesStructuredRhoSelfDominancePolicy>,
    ) -> HawkesKernelParameterValidationReport {
        Self::validate_structured_domain_with_stability_tolerance(
            lambda_bar,
            beta,
            structured_alpha,
            structured_rho,
            self_dominance_policy,
            HAWKES_DEFAULT_STABILITY_NUMERIC_TOLERANCE,
        )
    }

    /// Run structured base-plus-delta validation with explicit stability tolerance.
    pub fn validate_structured_domain_with_stability_tolerance(
        lambda_bar: f64,
        beta: f64,
        structured_alpha: HawkesStructuredAlphaParameterization,
        structured_rho: HawkesStructuredRhoParameterization,
        self_dominance_policy: Option<HawkesStructuredRhoSelfDominancePolicy>,
        stability_numeric_tolerance: f64,
    ) -> HawkesKernelParameterValidationReport {
        let mut checks: Vec<HawkesKernelParameterCheck> = Vec::new();

        validate_strictly_positive_check(
            &mut checks,
            "positivity.lambda_bar",
            HawkesKernelParameterCheckCategory::Positivity,
            "lambda_bar",
            lambda_bar,
            "lambda_bar must be finite and strictly positive",
            "increase lambda_bar to a finite value greater than 0",
        );

        let beta_is_valid = validate_strictly_positive_check(
            &mut checks,
            "positivity.beta",
            HawkesKernelParameterCheckCategory::Positivity,
            "beta",
            beta,
            "beta must be finite and strictly positive",
            "set beta to a finite value greater than 0",
        );

        validate_strictly_positive_check(
            &mut checks,
            "positivity.structured_alpha.a_self",
            HawkesKernelParameterCheckCategory::Positivity,
            "structured_alpha.a_self",
            structured_alpha.a_self,
            "structured_alpha.a_self must be finite and strictly positive",
            "set structured_alpha.a_self to a finite value greater than 0",
        );
        validate_non_negative_check(
            &mut checks,
            "positivity.structured_alpha.a_cross",
            HawkesKernelParameterCheckCategory::Positivity,
            "structured_alpha.a_cross",
            structured_alpha.a_cross,
            "structured_alpha.a_cross must be finite and non-negative",
            "set structured_alpha.a_cross to a finite value greater than or equal to 0",
        );
        validate_non_negative_check(
            &mut checks,
            "positivity.structured_alpha.delta_self",
            HawkesKernelParameterCheckCategory::Positivity,
            "structured_alpha.delta_self",
            structured_alpha.delta_self,
            "structured_alpha.delta_self must be finite and non-negative",
            "set structured_alpha.delta_self to a finite value greater than or equal to 0",
        );
        validate_non_negative_check(
            &mut checks,
            "positivity.structured_alpha.delta_cross",
            HawkesKernelParameterCheckCategory::Positivity,
            "structured_alpha.delta_cross",
            structured_alpha.delta_cross,
            "structured_alpha.delta_cross must be finite and non-negative",
            "set structured_alpha.delta_cross to a finite value greater than or equal to 0",
        );

        let derived_alpha = HawkesKernelLoadings {
            plus_plus: structured_alpha.a_self,
            plus_minus: structured_alpha.a_cross,
            minus_plus: structured_alpha.a_cross + structured_alpha.delta_cross,
            minus_minus: structured_alpha.a_self + structured_alpha.delta_self,
        };
        let derived_alpha_plus_plus_is_valid = validate_non_negative_check(
            &mut checks,
            "positivity.alpha.plus_plus",
            HawkesKernelParameterCheckCategory::Positivity,
            "alpha.plus_plus",
            derived_alpha.plus_plus,
            "derived alpha.plus_plus must be finite and non-negative",
            "adjust structured_alpha inputs to keep alpha.plus_plus finite and non-negative",
        );
        let derived_alpha_plus_minus_is_valid = validate_non_negative_check(
            &mut checks,
            "positivity.alpha.plus_minus",
            HawkesKernelParameterCheckCategory::Positivity,
            "alpha.plus_minus",
            derived_alpha.plus_minus,
            "derived alpha.plus_minus must be finite and non-negative",
            "adjust structured_alpha inputs to keep alpha.plus_minus finite and non-negative",
        );
        let derived_alpha_minus_plus_is_valid = validate_non_negative_check(
            &mut checks,
            "positivity.alpha.minus_plus",
            HawkesKernelParameterCheckCategory::Positivity,
            "alpha.minus_plus",
            derived_alpha.minus_plus,
            "derived alpha.minus_plus must be finite and non-negative",
            "adjust structured_alpha inputs to keep alpha.minus_plus finite and non-negative",
        );
        let derived_alpha_minus_minus_is_valid = validate_non_negative_check(
            &mut checks,
            "positivity.alpha.minus_minus",
            HawkesKernelParameterCheckCategory::Positivity,
            "alpha.minus_minus",
            derived_alpha.minus_minus,
            "derived alpha.minus_minus must be finite and non-negative",
            "adjust structured_alpha inputs to keep alpha.minus_minus finite and non-negative",
        );

        if derived_alpha_minus_minus_is_valid && derived_alpha_plus_plus_is_valid {
            validate_greater_or_equal_to_comparison_field_check(
                &mut checks,
                "structural.alpha.self_asymmetry",
                HawkesKernelParameterCheckCategory::Structural,
                "alpha.minus_minus",
                derived_alpha.minus_minus,
                "alpha.plus_plus",
                derived_alpha.plus_plus,
                "alpha self asymmetry requires alpha.minus_minus >= alpha.plus_plus",
                "increase structured_alpha.delta_self so alpha.minus_minus is at least alpha.plus_plus",
            );
        }

        if derived_alpha_minus_plus_is_valid && derived_alpha_plus_minus_is_valid {
            validate_greater_or_equal_to_comparison_field_check(
                &mut checks,
                "structural.alpha.cross_asymmetry",
                HawkesKernelParameterCheckCategory::Structural,
                "alpha.minus_plus",
                derived_alpha.minus_plus,
                "alpha.plus_minus",
                derived_alpha.plus_minus,
                "alpha cross asymmetry requires alpha.minus_plus >= alpha.plus_minus",
                "increase structured_alpha.delta_cross so alpha.minus_plus is at least alpha.plus_minus",
            );
        }

        validate_strictly_positive_check(
            &mut checks,
            "positivity.structured_rho.r_self",
            HawkesKernelParameterCheckCategory::Positivity,
            "structured_rho.r_self",
            structured_rho.r_self,
            "structured_rho.r_self must be finite and strictly positive",
            "set structured_rho.r_self to a finite value greater than 0",
        );
        validate_non_negative_check(
            &mut checks,
            "positivity.structured_rho.r_cross",
            HawkesKernelParameterCheckCategory::Positivity,
            "structured_rho.r_cross",
            structured_rho.r_cross,
            "structured_rho.r_cross must be finite and non-negative",
            "set structured_rho.r_cross to a finite value greater than or equal to 0",
        );
        validate_non_negative_check(
            &mut checks,
            "positivity.structured_rho.delta_self_r",
            HawkesKernelParameterCheckCategory::Positivity,
            "structured_rho.delta_self_r",
            structured_rho.delta_self_r,
            "structured_rho.delta_self_r must be finite and non-negative",
            "set structured_rho.delta_self_r to a finite value greater than or equal to 0",
        );
        validate_non_negative_check(
            &mut checks,
            "positivity.structured_rho.delta_cross_r",
            HawkesKernelParameterCheckCategory::Positivity,
            "structured_rho.delta_cross_r",
            structured_rho.delta_cross_r,
            "structured_rho.delta_cross_r must be finite and non-negative",
            "set structured_rho.delta_cross_r to a finite value greater than or equal to 0",
        );

        let derived_rho = HawkesKernelLoadings {
            plus_plus: structured_rho.r_self,
            plus_minus: structured_rho.r_cross,
            minus_plus: structured_rho.r_cross + structured_rho.delta_cross_r,
            minus_minus: structured_rho.r_self + structured_rho.delta_self_r,
        };
        let derived_rho_plus_plus_is_valid = validate_non_negative_check(
            &mut checks,
            "positivity.rho.plus_plus",
            HawkesKernelParameterCheckCategory::Positivity,
            "rho.plus_plus",
            derived_rho.plus_plus,
            "derived rho.plus_plus must be finite and non-negative",
            "adjust structured_rho inputs to keep rho.plus_plus finite and non-negative",
        );
        let derived_rho_plus_minus_is_valid = validate_non_negative_check(
            &mut checks,
            "positivity.rho.plus_minus",
            HawkesKernelParameterCheckCategory::Positivity,
            "rho.plus_minus",
            derived_rho.plus_minus,
            "derived rho.plus_minus must be finite and non-negative",
            "adjust structured_rho inputs to keep rho.plus_minus finite and non-negative",
        );
        let derived_rho_minus_plus_is_valid = validate_non_negative_check(
            &mut checks,
            "positivity.rho.minus_plus",
            HawkesKernelParameterCheckCategory::Positivity,
            "rho.minus_plus",
            derived_rho.minus_plus,
            "derived rho.minus_plus must be finite and non-negative",
            "adjust structured_rho inputs to keep rho.minus_plus finite and non-negative",
        );
        let derived_rho_minus_minus_is_valid = validate_non_negative_check(
            &mut checks,
            "positivity.rho.minus_minus",
            HawkesKernelParameterCheckCategory::Positivity,
            "rho.minus_minus",
            derived_rho.minus_minus,
            "derived rho.minus_minus must be finite and non-negative",
            "adjust structured_rho inputs to keep rho.minus_minus finite and non-negative",
        );

        if derived_rho_minus_minus_is_valid && derived_rho_plus_plus_is_valid {
            validate_greater_or_equal_to_comparison_field_check(
                &mut checks,
                "structural.rho.self_asymmetry",
                HawkesKernelParameterCheckCategory::Structural,
                "rho.minus_minus",
                derived_rho.minus_minus,
                "rho.plus_plus",
                derived_rho.plus_plus,
                "rho self asymmetry requires rho.minus_minus >= rho.plus_plus",
                "increase structured_rho.delta_self_r so rho.minus_minus is at least rho.plus_plus",
            );
        }

        if derived_rho_minus_plus_is_valid && derived_rho_plus_minus_is_valid {
            validate_greater_or_equal_to_comparison_field_check(
                &mut checks,
                "structural.rho.cross_asymmetry",
                HawkesKernelParameterCheckCategory::Structural,
                "rho.minus_plus",
                derived_rho.minus_plus,
                "rho.plus_minus",
                derived_rho.plus_minus,
                "rho cross asymmetry requires rho.minus_plus >= rho.plus_minus",
                "increase structured_rho.delta_cross_r so rho.minus_plus is at least rho.plus_minus",
            );
        }

        if let Some(policy) = self_dominance_policy {
            let minimum_ratio_is_valid = validate_greater_or_equal_threshold_check(
                &mut checks,
                "structural.rho.self_dominance.minimum_ratio",
                HawkesKernelParameterCheckCategory::Structural,
                "structured_rho.self_dominance_policy.minimum_self_to_cross_ratio",
                policy.minimum_self_to_cross_ratio,
                1.0,
                "self-dominance policy minimum ratio requires a threshold >= 1.0",
                "set self-dominance minimum ratio to a finite value greater than or equal to 1.0",
            );

            if minimum_ratio_is_valid
                && derived_rho_plus_plus_is_valid
                && derived_rho_plus_minus_is_valid
                && derived_rho_minus_minus_is_valid
                && derived_rho_minus_plus_is_valid
            {
                let target_plus_ratio =
                    derive_self_to_cross_ratio(derived_rho.plus_plus, derived_rho.plus_minus);
                let target_minus_ratio =
                    derive_self_to_cross_ratio(derived_rho.minus_minus, derived_rho.minus_plus);

                validate_greater_or_equal_threshold_check(
                    &mut checks,
                    "structural.rho.self_dominance.target_plus_ratio",
                    HawkesKernelParameterCheckCategory::Structural,
                    "rho.target_plus_self_to_cross_ratio",
                    target_plus_ratio,
                    policy.minimum_self_to_cross_ratio,
                    "target plus self-dominance ratio requires rho.plus_plus / rho.plus_minus >= configured minimum ratio",
                    "increase rho.plus_plus or decrease rho.plus_minus to satisfy the configured minimum self-dominance ratio",
                );
                validate_greater_or_equal_threshold_check(
                    &mut checks,
                    "structural.rho.self_dominance.target_minus_ratio",
                    HawkesKernelParameterCheckCategory::Structural,
                    "rho.target_minus_self_to_cross_ratio",
                    target_minus_ratio,
                    policy.minimum_self_to_cross_ratio,
                    "target minus self-dominance ratio requires rho.minus_minus / rho.minus_plus >= configured minimum ratio",
                    "increase rho.minus_minus or decrease rho.minus_plus to satisfy the configured minimum self-dominance ratio",
                );
            }
        }

        let stability_numeric_tolerance_is_valid = validate_non_negative_check(
            &mut checks,
            "stability.numeric_tolerance",
            HawkesKernelParameterCheckCategory::Stability,
            "stability.numeric_tolerance",
            stability_numeric_tolerance,
            "stability numeric tolerance must be finite and non-negative",
            "set stability.numeric_tolerance to a finite value greater than or equal to 0",
        );

        let derived_alpha_is_valid = derived_alpha_plus_plus_is_valid
            && derived_alpha_plus_minus_is_valid
            && derived_alpha_minus_plus_is_valid
            && derived_alpha_minus_minus_is_valid;

        if beta_is_valid && derived_alpha_is_valid && stability_numeric_tolerance_is_valid {
            let stability_check = compute_spectral_radius_stability_check(
                derived_alpha,
                beta,
                stability_numeric_tolerance,
            );

            push_validation_check(
                &mut checks,
                "stability.alpha_over_beta.spectral_radius",
                HawkesKernelValidationStatus::from_bool(stability_check.is_stable),
                HawkesKernelParameterCheckCategory::Stability,
                "alpha_over_beta.spectral_radius",
                Some("stability.effective_stationarity_threshold"),
                HawkesKernelParameterConstraint::SpectralRadiusStrictlyBelowOne,
                stability_check.spectral_radius_alpha_over_beta,
                Some(stability_check.effective_stationarity_threshold),
                "spectral radius must be strictly below the effective stationarity threshold",
                format!(
                    "spectral radius={} and effective_threshold={} (strict_threshold={}, numeric_tolerance={})",
                    stability_check.spectral_radius_alpha_over_beta,
                    stability_check.effective_stationarity_threshold,
                    stability_check.strict_stationarity_threshold,
                    stability_check.numeric_tolerance
                ),
                "reduce alpha magnitudes or increase beta so spectral radius(alpha / beta) is below 1",
            );
        }

        let derived_rho_is_valid = derived_rho_plus_plus_is_valid
            && derived_rho_plus_minus_is_valid
            && derived_rho_minus_plus_is_valid
            && derived_rho_minus_minus_is_valid;

        HawkesKernelParameterValidationReport::from_checks_with_derived_loadings(
            checks,
            if derived_alpha_is_valid {
                Some(derived_alpha)
            } else {
                None
            },
            if derived_rho_is_valid {
                Some(derived_rho)
            } else {
                None
            },
        )
    }

    /// Evaluate de-seasonalized Hawkes stationarity via
    /// `spectral_radius(Alpha / beta) < 1 - numeric_tolerance`.
    ///
    /// The tolerance policy is intentionally strict and one-sided:
    /// larger tolerance values tighten acceptance near the threshold.
    pub fn check_spectral_radius_stability(
        beta: f64,
        alpha: HawkesKernelLoadings,
        numeric_tolerance: f64,
    ) -> Result<HawkesSpectralRadiusStabilityCheck, HawkesKernelParameterError> {
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

        if !numeric_tolerance.is_finite() || numeric_tolerance < 0.0 {
            return Err(
                HawkesKernelParameterError::InvalidStabilityNumericTolerance {
                    value: numeric_tolerance,
                },
            );
        }

        Ok(compute_spectral_radius_stability_check(
            alpha,
            beta,
            numeric_tolerance,
        ))
    }

    /// Validate and construct kernel parameters.
    ///
    /// Domain checks:
    /// - `lambda_bar > 0` and finite
    /// - `beta > 0` and finite
    /// - all `alpha_ij >= 0` and finite
    /// - all `rho_ij >= 0` and finite
    /// - `spectral_radius(Alpha / beta) < 1`
    pub fn try_new(
        lambda_bar: f64,
        beta: f64,
        alpha: HawkesKernelLoadings,
        rho: HawkesKernelLoadings,
    ) -> Result<Self, HawkesKernelParameterError> {
        Self::try_new_with_stability_tolerance(
            lambda_bar,
            beta,
            alpha,
            rho,
            HAWKES_DEFAULT_STABILITY_NUMERIC_TOLERANCE,
        )
    }

    /// Validate and construct kernel parameters using an explicit stationarity
    /// tolerance around the strict threshold.
    pub fn try_new_with_stability_tolerance(
        lambda_bar: f64,
        beta: f64,
        alpha: HawkesKernelLoadings,
        rho: HawkesKernelLoadings,
        stability_numeric_tolerance: f64,
    ) -> Result<Self, HawkesKernelParameterError> {
        let validation_report = Self::validate_domain_with_stability_tolerance(
            lambda_bar,
            beta,
            alpha,
            rho,
            stability_numeric_tolerance,
        );
        if let Some(violation) = validation_report.first_violation() {
            return Err(map_parameter_violation_to_error_with_stability_tolerance(
                violation,
                stability_numeric_tolerance,
            ));
        }

        Ok(Self {
            lambda_bar,
            beta,
            alpha,
            rho,
        })
    }

    /// Validate and construct parameters from typed target/source component groups.
    ///
    /// This builder removes 4-value positional ambiguity when constructing `alpha`
    /// and `rho`.
    pub fn try_new_from_target_components(
        lambda_bar: f64,
        beta: f64,
        alpha_target_plus: HawkesSourceComponents,
        alpha_target_minus: HawkesSourceComponents,
        rho_target_plus: HawkesSourceComponents,
        rho_target_minus: HawkesSourceComponents,
    ) -> Result<Self, HawkesKernelParameterError> {
        Self::try_new(
            lambda_bar,
            beta,
            HawkesKernelLoadings::from_target_components(alpha_target_plus, alpha_target_minus),
            HawkesKernelLoadings::from_target_components(rho_target_plus, rho_target_minus),
        )
    }
}

/// Machine-readable constraint labels for kernel validation checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum HawkesKernelParameterConstraint {
    FiniteAndStrictlyPositive,
    FiniteAndNonNegative,
    GreaterThanOrEqualToComparisonField,
    GreaterThanOrEqualToThreshold,
    SpectralRadiusStrictlyBelowOne,
}

impl Display for HawkesKernelParameterConstraint {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FiniteAndStrictlyPositive => {
                write!(formatter, "finite_and_strictly_positive")
            }
            Self::FiniteAndNonNegative => write!(formatter, "finite_and_non_negative"),
            Self::GreaterThanOrEqualToComparisonField => {
                write!(formatter, "greater_than_or_equal_to_comparison_field")
            }
            Self::GreaterThanOrEqualToThreshold => {
                write!(formatter, "greater_than_or_equal_to_threshold")
            }
            Self::SpectralRadiusStrictlyBelowOne => {
                write!(formatter, "spectral_radius_strictly_below_one")
            }
        }
    }
}

/// Structured machine-readable violation payload for failed validation checks.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct HawkesKernelParameterViolation {
    pub field: &'static str,
    pub constraint: HawkesKernelParameterConstraint,
    pub value: f64,
}

/// Full report for standalone kernel validation calls.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HawkesKernelParameterValidationReport {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub status: HawkesKernelValidationStatus,
    pub summary: HawkesKernelParameterValidationSummary,
    pub derived_alpha: Option<HawkesKernelLoadings>,
    pub derived_rho: Option<HawkesKernelLoadings>,
    pub checks: Vec<HawkesKernelParameterCheck>,
    pub violations: Vec<HawkesKernelParameterViolation>,
}

impl HawkesKernelParameterValidationReport {
    pub fn is_valid(&self) -> bool {
        self.status == HawkesKernelValidationStatus::Pass
    }

    pub fn first_violation(&self) -> Option<HawkesKernelParameterViolation> {
        self.violations.first().copied()
    }

    fn from_checks(checks: Vec<HawkesKernelParameterCheck>) -> Self {
        Self::from_checks_with_derived_loadings(checks, None, None)
    }

    fn from_checks_with_derived_loadings(
        checks: Vec<HawkesKernelParameterCheck>,
        derived_alpha: Option<HawkesKernelLoadings>,
        derived_rho: Option<HawkesKernelLoadings>,
    ) -> Self {
        let total_check_count = checks.len();
        let failed_check_count = checks
            .iter()
            .filter(|check| check.status == HawkesKernelValidationStatus::Fail)
            .count();

        let passed_check_count = total_check_count - failed_check_count;

        let status = if failed_check_count == 0 {
            HawkesKernelValidationStatus::Pass
        } else {
            HawkesKernelValidationStatus::Fail
        };

        let violations = checks
            .iter()
            .filter(|check| check.status == HawkesKernelValidationStatus::Fail)
            .map(|check| HawkesKernelParameterViolation {
                field: check.field,
                constraint: check.constraint,
                value: check.observed_value,
            })
            .collect::<Vec<_>>();

        Self {
            schema_version: HAWKES_KERNEL_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION,
            determinism_policy: HAWKES_KERNEL_PARAMETER_VALIDATION_DETERMINISM_POLICY,
            status,
            summary: HawkesKernelParameterValidationSummary {
                total_check_count,
                passed_check_count,
                failed_check_count,
            },
            derived_alpha,
            derived_rho,
            checks,
            violations,
        }
    }
}

/// Structured validation failures for `HawkesKernelParameters::try_new`.
#[derive(Debug, Clone, PartialEq)]
pub enum HawkesKernelParameterError {
    InvalidLambdaBar {
        value: f64,
    },
    InvalidBeta {
        value: f64,
    },
    InvalidLoadingComponent {
        field: &'static str,
        value: f64,
    },
    InvalidStabilityNumericTolerance {
        value: f64,
    },
    UnstableSpectralRadius {
        spectral_radius_alpha_over_beta: f64,
        strict_stationarity_threshold: f64,
        numeric_tolerance: f64,
        effective_stationarity_threshold: f64,
    },
}

impl Display for HawkesKernelParameterError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLambdaBar { value } => {
                write!(
                    formatter,
                    "lambda_bar must be finite and strictly positive, found {value}"
                )
            }
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
            Self::InvalidStabilityNumericTolerance { value } => write!(
                formatter,
                "stability numeric tolerance must be finite and non-negative, found {value}"
            ),
            Self::UnstableSpectralRadius {
                spectral_radius_alpha_over_beta,
                strict_stationarity_threshold,
                numeric_tolerance,
                effective_stationarity_threshold,
            } => write!(
                formatter,
                "spectral radius stability failed with spectral_radius_alpha_over_beta={spectral_radius_alpha_over_beta}, strict_stationarity_threshold={strict_stationarity_threshold}, numeric_tolerance={numeric_tolerance}, effective_stationarity_threshold={effective_stationarity_threshold}"
            ),
        }
    }
}

impl Error for HawkesKernelParameterError {}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct HawkesKernelParametersPayload {
    lambda_bar: f64,
    beta: f64,
    alpha: HawkesKernelLoadings,
    rho: HawkesKernelLoadings,
}

impl<'de> Deserialize<'de> for HawkesKernelParameters {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = HawkesKernelParametersPayload::deserialize(deserializer)?;
        Self::try_new(payload.lambda_bar, payload.beta, payload.alpha, payload.rho)
            .map_err(serde::de::Error::custom)
    }
}

fn push_validation_check(
    checks: &mut Vec<HawkesKernelParameterCheck>,
    check_id: &'static str,
    status: HawkesKernelValidationStatus,
    category: HawkesKernelParameterCheckCategory,
    field: &'static str,
    comparison_field: Option<&'static str>,
    constraint: HawkesKernelParameterConstraint,
    observed_value: f64,
    threshold_value: Option<f64>,
    expected_condition: &'static str,
    diagnostics: String,
    remediation_hint: &'static str,
) {
    checks.push(HawkesKernelParameterCheck {
        check_id,
        status,
        category,
        field,
        comparison_field,
        constraint,
        observed_value,
        threshold_value,
        expected_condition,
        diagnostics,
        remediation_hint,
    });
}

fn validate_strictly_positive_check(
    checks: &mut Vec<HawkesKernelParameterCheck>,
    check_id: &'static str,
    category: HawkesKernelParameterCheckCategory,
    field: &'static str,
    value: f64,
    diagnostics_label: &'static str,
    remediation_hint: &'static str,
) -> bool {
    let is_valid = value.is_finite() && value > 0.0;
    let status = HawkesKernelValidationStatus::from_bool(is_valid);

    let diagnostics = if is_valid {
        format!("{diagnostics_label}: observed value {value} satisfies > 0")
    } else {
        format!("{diagnostics_label}: observed value {value} violates > 0")
    };

    push_validation_check(
        checks,
        check_id,
        status,
        category,
        field,
        None,
        HawkesKernelParameterConstraint::FiniteAndStrictlyPositive,
        value,
        Some(0.0),
        "value is finite and strictly greater than 0",
        diagnostics,
        remediation_hint,
    );

    is_valid
}

fn validate_non_negative_check(
    checks: &mut Vec<HawkesKernelParameterCheck>,
    check_id: &'static str,
    category: HawkesKernelParameterCheckCategory,
    field: &'static str,
    value: f64,
    diagnostics_label: &'static str,
    remediation_hint: &'static str,
) -> bool {
    let is_valid = value.is_finite() && value >= 0.0;
    let status = HawkesKernelValidationStatus::from_bool(is_valid);

    let diagnostics = if is_valid {
        format!("{diagnostics_label}: observed value {value} satisfies >= 0")
    } else {
        format!("{diagnostics_label}: observed value {value} violates >= 0")
    };

    push_validation_check(
        checks,
        check_id,
        status,
        category,
        field,
        None,
        HawkesKernelParameterConstraint::FiniteAndNonNegative,
        value,
        Some(0.0),
        "value is finite and greater than or equal to 0",
        diagnostics,
        remediation_hint,
    );

    is_valid
}

fn validate_greater_or_equal_to_comparison_field_check(
    checks: &mut Vec<HawkesKernelParameterCheck>,
    check_id: &'static str,
    category: HawkesKernelParameterCheckCategory,
    field: &'static str,
    value: f64,
    comparison_field: &'static str,
    comparison_value: f64,
    diagnostics_label: &'static str,
    remediation_hint: &'static str,
) -> bool {
    let is_valid = value.is_finite() && comparison_value.is_finite() && value >= comparison_value;
    let status = HawkesKernelValidationStatus::from_bool(is_valid);

    let diagnostics = if is_valid {
        format!(
            "{diagnostics_label}: observed value {value} satisfies >= comparison value {comparison_value}"
        )
    } else {
        format!(
            "{diagnostics_label}: observed value {value} violates >= comparison value {comparison_value}"
        )
    };

    push_validation_check(
        checks,
        check_id,
        status,
        category,
        field,
        Some(comparison_field),
        HawkesKernelParameterConstraint::GreaterThanOrEqualToComparisonField,
        value,
        Some(comparison_value),
        "value is finite and greater than or equal to comparison field value",
        diagnostics,
        remediation_hint,
    );

    is_valid
}

fn validate_greater_or_equal_threshold_check(
    checks: &mut Vec<HawkesKernelParameterCheck>,
    check_id: &'static str,
    category: HawkesKernelParameterCheckCategory,
    field: &'static str,
    value: f64,
    threshold_value: f64,
    diagnostics_label: &'static str,
    remediation_hint: &'static str,
) -> bool {
    let is_valid = value.is_finite() && value >= threshold_value;
    let status = HawkesKernelValidationStatus::from_bool(is_valid);

    let diagnostics = if is_valid {
        format!(
            "{diagnostics_label}: observed value {value} satisfies >= threshold {threshold_value}"
        )
    } else {
        format!(
            "{diagnostics_label}: observed value {value} violates >= threshold {threshold_value}"
        )
    };

    push_validation_check(
        checks,
        check_id,
        status,
        category,
        field,
        None,
        HawkesKernelParameterConstraint::GreaterThanOrEqualToThreshold,
        value,
        Some(threshold_value),
        "value is finite and greater than or equal to configured threshold",
        diagnostics,
        remediation_hint,
    );

    is_valid
}

fn validate_non_negative_loadings(
    loadings: HawkesKernelLoadings,
    component_fields: [&'static str; 4],
) -> Result<(), HawkesKernelParameterError> {
    let mut violations: Vec<HawkesKernelParameterViolation> = Vec::new();
    collect_non_negative_loading_violations(loadings, component_fields, &mut violations);

    if let Some(violation) = violations.first().copied() {
        return Err(map_parameter_violation_to_error(violation));
    }

    Ok(())
}

fn collect_non_negative_loading_violations(
    loadings: HawkesKernelLoadings,
    component_fields: [&'static str; 4],
    violations: &mut Vec<HawkesKernelParameterViolation>,
) {
    for (component_field_name, component_value) in component_fields
        .into_iter()
        .zip(loadings.as_model_ordered_array())
    {
        if !component_value.is_finite() || component_value < 0.0 {
            violations.push(HawkesKernelParameterViolation {
                field: component_field_name,
                constraint: HawkesKernelParameterConstraint::FiniteAndNonNegative,
                value: component_value,
            });
        }
    }
}

fn map_parameter_violation_to_error(
    violation: HawkesKernelParameterViolation,
) -> HawkesKernelParameterError {
    map_parameter_violation_to_error_with_stability_tolerance(
        violation,
        HAWKES_DEFAULT_STABILITY_NUMERIC_TOLERANCE,
    )
}

fn map_parameter_violation_to_error_with_stability_tolerance(
    violation: HawkesKernelParameterViolation,
    stability_numeric_tolerance: f64,
) -> HawkesKernelParameterError {
    match violation.field {
        "lambda_bar" => HawkesKernelParameterError::InvalidLambdaBar {
            value: violation.value,
        },
        "beta" => HawkesKernelParameterError::InvalidBeta {
            value: violation.value,
        },
        "stability.numeric_tolerance" => {
            HawkesKernelParameterError::InvalidStabilityNumericTolerance {
                value: violation.value,
            }
        }
        "alpha_over_beta.spectral_radius" => HawkesKernelParameterError::UnstableSpectralRadius {
            spectral_radius_alpha_over_beta: violation.value,
            strict_stationarity_threshold: HAWKES_STRICT_STATIONARITY_THRESHOLD,
            numeric_tolerance: stability_numeric_tolerance,
            effective_stationarity_threshold: HAWKES_STRICT_STATIONARITY_THRESHOLD
                - stability_numeric_tolerance,
        },
        _ => HawkesKernelParameterError::InvalidLoadingComponent {
            field: violation.field,
            value: violation.value,
        },
    }
}

fn compute_spectral_radius_stability_check(
    alpha: HawkesKernelLoadings,
    beta: f64,
    numeric_tolerance: f64,
) -> HawkesSpectralRadiusStabilityCheck {
    let spectral_radius_alpha_over_beta = compute_spectral_radius_alpha_over_beta(alpha, beta);
    let effective_stationarity_threshold = HAWKES_STRICT_STATIONARITY_THRESHOLD - numeric_tolerance;

    HawkesSpectralRadiusStabilityCheck {
        spectral_radius_alpha_over_beta,
        strict_stationarity_threshold: HAWKES_STRICT_STATIONARITY_THRESHOLD,
        numeric_tolerance,
        effective_stationarity_threshold,
        is_stable: spectral_radius_alpha_over_beta < effective_stationarity_threshold,
    }
}

fn compute_spectral_radius_alpha_over_beta(alpha: HawkesKernelLoadings, beta: f64) -> f64 {
    let scaled_plus_plus = alpha.plus_plus / beta;
    let scaled_plus_minus = alpha.plus_minus / beta;
    let scaled_minus_plus = alpha.minus_plus / beta;
    let scaled_minus_minus = alpha.minus_minus / beta;

    let trace = scaled_plus_plus + scaled_minus_minus;
    let discriminant = ((scaled_plus_plus - scaled_minus_minus).powi(2)
        + 4.0 * scaled_plus_minus * scaled_minus_plus)
        .max(0.0);
    let square_root_discriminant = discriminant.sqrt();

    let first_eigenvalue = 0.5 * (trace + square_root_discriminant);
    let second_eigenvalue = 0.5 * (trace - square_root_discriminant);

    first_eigenvalue.abs().max(second_eigenvalue.abs())
}

fn derive_self_to_cross_ratio(self_loading: f64, cross_loading: f64) -> f64 {
    if cross_loading == 0.0 {
        return f64::INFINITY;
    }

    self_loading / cross_loading
}

#[cfg(test)]
mod tests {
    use crate::hawkes::{
        HAWKES_KERNEL_PARAMETER_VALIDATION_DETERMINISM_POLICY,
        HAWKES_KERNEL_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION,
        HAWKES_STRICT_STATIONARITY_THRESHOLD, HawkesEventSide, HawkesEventSidePayloadError,
        HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameterConstraint,
        HawkesKernelParameterError, HawkesKernelParameterViolation, HawkesKernelParameters,
        HawkesKernelValidationStatus, HawkesNewsSeverityMarkError, HawkesSeasonalityIntervalError,
        HawkesSourceComponents, HawkesStructuredAlphaParameterError,
        HawkesStructuredAlphaParameterization, HawkesStructuredRhoParameterError,
        HawkesStructuredRhoParameterization, HawkesStructuredRhoSelfDominancePolicy,
        HawkesTransitionError, HawkesTransitionStep,
    };
    use crate::hourly_factors::{
        HourlyFactorContractDiagnostics, HourlyFactorContractOutput,
        HourlyFactorMaterializationMetadata,
    };
    use serde_json::json;

    const DECAY_TOLERANCE: f64 = 1e-12;

    fn assert_within_tolerance(actual: f64, expected: f64, tolerance: f64, field_name: &str) {
        let absolute_difference = (actual - expected).abs();
        assert!(
            absolute_difference <= tolerance,
            "{field_name} mismatch expected {expected}, found {actual}, absolute_difference={absolute_difference}, tolerance={tolerance}"
        );
    }

    fn build_hourly_factor_contract_with_seasonality(
        seasonality_factors: Vec<f64>,
    ) -> HourlyFactorContractOutput {
        let interval_count = seasonality_factors.len();
        let interval_indices = (0..interval_count)
            .map(|interval_index| {
                u64::try_from(interval_index).expect("test interval index must fit in u64")
            })
            .collect::<Vec<_>>();
        let interval_start_utc_nanos = (0..interval_count)
            .map(|interval_index| {
                i64::try_from(interval_index).expect("test interval index must fit in i64")
                    * 3_600_000_000_000
            })
            .collect::<Vec<_>>();
        let interval_end_utc_nanos = interval_start_utc_nanos
            .iter()
            .copied()
            .map(|interval_start| interval_start + 3_600_000_000_000)
            .collect::<Vec<_>>();
        let interval_start_exchange_local = (0..interval_count)
            .map(|hour| format!("2026-02-02 {hour:02}:00:00-05:00"))
            .collect::<Vec<_>>();
        let interval_end_exchange_local = (0..interval_count)
            .map(|hour| format!("2026-02-02 {:02}:00:00-05:00", hour + 1))
            .collect::<Vec<_>>();

        HourlyFactorContractOutput {
            schema_version: "test.hourly_factor.contract.v1",
            exchange_code: "XNYS".to_string(),
            exchange_timezone_name: "America/New_York".to_string(),
            request_start_utc_nanos: 0,
            request_end_utc_nanos: i64::try_from(interval_count)
                .expect("test interval count must fit in i64")
                * 3_600_000_000_000,
            interval_indices,
            interval_start_utc_nanos,
            interval_end_utc_nanos,
            interval_start_exchange_local,
            interval_end_exchange_local,
            s_h: seasonality_factors,
            nu_h: vec![0.0; interval_count],
            metadata: HourlyFactorMaterializationMetadata {
                seasonality_materialization_method: "test.seasonality.v1".to_string(),
                news_intensity_materialization_method: "test.news.v1".to_string(),
                interval_boundary_policy: "test.interval_boundary_policy.v1",
                interval_alignment_policy: "test.interval_alignment_policy.v1",
                timezone_transition_policy: "test.timezone_transition_policy.v1",
                spring_forward_policy: "test.spring_forward_policy.v1",
                fall_back_policy: "test.fall_back_policy.v1",
            },
            diagnostics: HourlyFactorContractDiagnostics {
                interval_count,
                first_interval_start_utc_nanos: 0,
                last_interval_end_utc_nanos: i64::try_from(interval_count)
                    .expect("test interval count must fit in i64")
                    * 3_600_000_000_000,
                skipped_local_hour_interval_count: 0,
                repeated_local_hour_interval_count: 0,
            },
        }
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
    fn intensity_projection_matches_deseasonalized_and_scaled_equations() {
        let state = HawkesExcitationState {
            y_plus_plus: 1.2,
            y_plus_minus: 0.4,
            y_minus_plus: 0.3,
            y_minus_minus: 0.5,
        };

        let projection = state.project_intensities(0.8, 1.25);

        assert_within_tolerance(
            projection.deseasonalized.lambda_plus,
            2.4,
            DECAY_TOLERANCE,
            "deseasonalized.lambda_plus",
        );
        assert_within_tolerance(
            projection.deseasonalized.lambda_minus,
            1.6,
            DECAY_TOLERANCE,
            "deseasonalized.lambda_minus",
        );
        assert_within_tolerance(
            projection.seasonality_scaled.lambda_plus,
            3.0,
            DECAY_TOLERANCE,
            "seasonality_scaled.lambda_plus",
        );
        assert_within_tolerance(
            projection.seasonality_scaled.lambda_minus,
            2.0,
            DECAY_TOLERANCE,
            "seasonality_scaled.lambda_minus",
        );
    }

    #[test]
    fn intensity_projection_handles_hourly_piecewise_seasonality_edge_values() {
        let state = HawkesExcitationState {
            y_plus_plus: 0.15,
            y_plus_minus: 0.05,
            y_minus_plus: 0.10,
            y_minus_minus: 0.20,
        };
        let lambda_bar = 0.40;
        let seasonality_factors = [0.25, 1.0, 3.5];
        let expected_scaled_plus = [0.15, 0.60, 2.10];
        let expected_scaled_minus = [0.175, 0.70, 2.45];

        for (position, seasonality_factor) in seasonality_factors.iter().copied().enumerate() {
            let projection = state.project_intensities(lambda_bar, seasonality_factor);

            assert_within_tolerance(
                projection.deseasonalized.lambda_plus,
                0.60,
                DECAY_TOLERANCE,
                "deseasonalized.lambda_plus",
            );
            assert_within_tolerance(
                projection.deseasonalized.lambda_minus,
                0.70,
                DECAY_TOLERANCE,
                "deseasonalized.lambda_minus",
            );
            assert_within_tolerance(
                projection.seasonality_scaled.lambda_plus,
                expected_scaled_plus[position],
                DECAY_TOLERANCE,
                "seasonality_scaled.lambda_plus",
            );
            assert_within_tolerance(
                projection.seasonality_scaled.lambda_minus,
                expected_scaled_minus[position],
                DECAY_TOLERANCE,
                "seasonality_scaled.lambda_minus",
            );
        }
    }

    #[test]
    fn intensity_projection_is_symmetric_when_side_state_components_match() {
        let symmetric_state = HawkesExcitationState {
            y_plus_plus: 1.1,
            y_plus_minus: 0.9,
            y_minus_plus: 1.1,
            y_minus_minus: 0.9,
        };

        let projection = symmetric_state.project_intensities(0.7, 1.4);

        assert_within_tolerance(
            projection.deseasonalized.lambda_plus,
            projection.deseasonalized.lambda_minus,
            DECAY_TOLERANCE,
            "deseasonalized symmetry",
        );
        assert_within_tolerance(
            projection.seasonality_scaled.lambda_plus,
            projection.seasonality_scaled.lambda_minus,
            DECAY_TOLERANCE,
            "seasonality_scaled symmetry",
        );
    }

    #[test]
    fn hourly_interval_projection_reads_seasonality_from_shared_contract() {
        let state = HawkesExcitationState {
            y_plus_plus: 0.2,
            y_plus_minus: 0.1,
            y_minus_plus: 0.4,
            y_minus_minus: 0.3,
        };
        let hourly_factor_contract =
            build_hourly_factor_contract_with_seasonality(vec![0.8, 1.3, 0.6]);

        let projection = state
            .project_intensities_for_hourly_interval(0.5, &hourly_factor_contract, 1)
            .expect("interval 1 must resolve from shared hourly contract");

        assert_within_tolerance(
            projection.deseasonalized.lambda_plus,
            0.8,
            DECAY_TOLERANCE,
            "deseasonalized.lambda_plus",
        );
        assert_within_tolerance(
            projection.deseasonalized.lambda_minus,
            1.2,
            DECAY_TOLERANCE,
            "deseasonalized.lambda_minus",
        );
        assert_within_tolerance(
            projection.seasonality_scaled.lambda_plus,
            1.04,
            DECAY_TOLERANCE,
            "seasonality_scaled.lambda_plus",
        );
        assert_within_tolerance(
            projection.seasonality_scaled.lambda_minus,
            1.56,
            DECAY_TOLERANCE,
            "seasonality_scaled.lambda_minus",
        );
    }

    #[test]
    fn hourly_interval_projection_rejects_out_of_range_interval_index() {
        let state = HawkesExcitationState {
            y_plus_plus: 0.2,
            y_plus_minus: 0.1,
            y_minus_plus: 0.4,
            y_minus_minus: 0.3,
        };
        let hourly_factor_contract = build_hourly_factor_contract_with_seasonality(vec![1.0, 0.9]);

        let error = state
            .project_intensities_for_hourly_interval(0.5, &hourly_factor_contract, 2)
            .expect_err("out-of-range interval index must fail");

        assert_eq!(
            error,
            HawkesSeasonalityIntervalError {
                interval_index: 2,
                interval_count: 2,
            }
        );
        assert_eq!(
            error.to_string(),
            "interval_index 2 is out of range for hourly seasonality factor count 2"
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
    fn exogenous_news_loading_jump_applies_component_wise_rho_scaled_by_severity_mark() {
        let state = HawkesExcitationState {
            y_plus_plus: 1.0,
            y_plus_minus: 1.5,
            y_minus_plus: 2.0,
            y_minus_minus: 2.5,
        };
        let rho = HawkesKernelLoadings {
            plus_plus: 0.2,
            plus_minus: 0.1,
            minus_plus: 0.05,
            minus_minus: 0.3,
        };
        let severity_mark = 2.5;

        let updated_state = state
            .apply_exogenous_news_loading_jump(rho, severity_mark)
            .expect("valid non-negative severity mark must succeed");

        assert_within_tolerance(
            updated_state.y_plus_plus,
            1.0 + 0.2 * severity_mark,
            DECAY_TOLERANCE,
            "y_plus_plus",
        );
        assert_within_tolerance(
            updated_state.y_plus_minus,
            1.5 + 0.1 * severity_mark,
            DECAY_TOLERANCE,
            "y_plus_minus",
        );
        assert_within_tolerance(
            updated_state.y_minus_plus,
            2.0 + 0.05 * severity_mark,
            DECAY_TOLERANCE,
            "y_minus_plus",
        );
        assert_within_tolerance(
            updated_state.y_minus_minus,
            2.5 + 0.3 * severity_mark,
            DECAY_TOLERANCE,
            "y_minus_minus",
        );
    }

    #[test]
    fn exogenous_news_loading_jump_with_zero_severity_preserves_state_exactly() {
        let state = HawkesExcitationState {
            y_plus_plus: 0.8,
            y_plus_minus: 0.7,
            y_minus_plus: 0.6,
            y_minus_minus: 0.5,
        };
        let rho = HawkesKernelLoadings {
            plus_plus: 0.2,
            plus_minus: 0.1,
            minus_plus: 0.3,
            minus_minus: 0.4,
        };

        let updated_state = state
            .apply_exogenous_news_loading_jump(rho, 0.0)
            .expect("zero severity mark is valid and deterministic");

        assert_eq!(updated_state, state);
    }

    #[test]
    fn exogenous_news_loading_jump_rejects_negative_severity_mark() {
        let state = HawkesExcitationState {
            y_plus_plus: 0.1,
            y_plus_minus: 0.2,
            y_minus_plus: 0.3,
            y_minus_minus: 0.4,
        };
        let rho = HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.0,
            minus_plus: 0.0,
            minus_minus: 0.0,
        };

        let error = state
            .apply_exogenous_news_loading_jump(rho, -0.000_1)
            .expect_err("negative severity mark must fail");

        assert_eq!(error, HawkesNewsSeverityMarkError { value: -0.000_1 });
        assert_eq!(
            error.to_string(),
            "news severity mark must be finite and non-negative, found -0.0001"
        );
    }

    #[test]
    fn exogenous_and_endogenous_jumps_compose_with_drift() {
        let state = HawkesExcitationState {
            y_plus_plus: 1.0,
            y_plus_minus: 2.0,
            y_minus_plus: 3.0,
            y_minus_minus: 4.0,
        };
        let alpha = HawkesKernelLoadings {
            plus_plus: 0.3,
            plus_minus: 0.7,
            minus_plus: 0.5,
            minus_minus: 0.9,
        };
        let rho = HawkesKernelLoadings {
            plus_plus: 0.2,
            plus_minus: 0.1,
            minus_plus: 0.05,
            minus_minus: 0.12,
        };
        let severity_mark = 1.5;
        let decay_factor = (-0.4f64).exp();

        let updated_state = state
            .apply_shared_decay_drift(1.0, 0.4)
            .apply_endogenous_event_jump(HawkesEventSide::Buy, alpha)
            .apply_exogenous_news_loading_jump(rho, severity_mark)
            .expect("valid non-negative severity mark must succeed")
            .apply_endogenous_event_jump(HawkesEventSide::Sell, alpha);

        assert_within_tolerance(
            updated_state.y_plus_plus,
            1.0 * decay_factor + 0.3 + 0.2 * severity_mark,
            DECAY_TOLERANCE,
            "y_plus_plus",
        );
        assert_within_tolerance(
            updated_state.y_plus_minus,
            2.0 * decay_factor + 0.1 * severity_mark + 0.7,
            DECAY_TOLERANCE,
            "y_plus_minus",
        );
        assert_within_tolerance(
            updated_state.y_minus_plus,
            3.0 * decay_factor + 0.5 + 0.05 * severity_mark,
            DECAY_TOLERANCE,
            "y_minus_plus",
        );
        assert_within_tolerance(
            updated_state.y_minus_minus,
            4.0 * decay_factor + 0.12 * severity_mark + 0.9,
            DECAY_TOLERANCE,
            "y_minus_minus",
        );
    }

    #[test]
    fn composed_transition_step_matches_reference_call_order_sequence() {
        let state = HawkesExcitationState {
            y_plus_plus: 1.0,
            y_plus_minus: 2.0,
            y_minus_plus: 3.0,
            y_minus_minus: 4.0,
        };
        let kernel_parameters = HawkesKernelParameters::try_new(
            0.75,
            1.0,
            HawkesKernelLoadings {
                plus_plus: 0.3,
                plus_minus: 0.2,
                minus_plus: 0.1,
                minus_minus: 0.4,
            },
            HawkesKernelLoadings {
                plus_plus: 0.2,
                plus_minus: 0.1,
                minus_plus: 0.05,
                minus_minus: 0.12,
            },
        )
        .expect("kernel fixture must be valid");
        let transition_step = HawkesTransitionStep::new(0.4, Some(HawkesEventSide::Buy), Some(1.5));

        let expected_state = state
            .apply_shared_decay_drift(kernel_parameters.beta, transition_step.delta_t)
            .apply_endogenous_event_jump(HawkesEventSide::Buy, kernel_parameters.alpha)
            .apply_exogenous_news_loading_jump(
                kernel_parameters.rho,
                transition_step
                    .exogenous_news_severity_mark
                    .expect("fixture must define exogenous severity mark"),
            )
            .expect("fixture severity mark must be valid");

        let composed_state = state
            .apply_transition_step(kernel_parameters, transition_step)
            .expect("canonical transition composition should succeed");

        assert_eq!(composed_state, expected_state);
    }

    #[test]
    fn composed_transition_sequence_matches_manual_step_iteration() {
        let state = HawkesExcitationState {
            y_plus_plus: 0.8,
            y_plus_minus: 0.4,
            y_minus_plus: 0.2,
            y_minus_minus: 0.5,
        };

        let kernel_parameters = HawkesKernelParameters::try_new(
            0.60,
            1.2,
            HawkesKernelLoadings {
                plus_plus: 0.15,
                plus_minus: 0.05,
                minus_plus: 0.25,
                minus_minus: 0.2,
            },
            HawkesKernelLoadings {
                plus_plus: 0.2,
                plus_minus: 0.15,
                minus_plus: 0.1,
                minus_minus: 0.05,
            },
        )
        .expect("kernel fixture must be valid");

        let transition_steps = [
            HawkesTransitionStep::new(0.3, Some(HawkesEventSide::Sell), None),
            HawkesTransitionStep::new(0.1, None, Some(0.75)),
            HawkesTransitionStep::new(0.0, Some(HawkesEventSide::Buy), Some(0.0)),
        ];

        let expected_state = transition_steps
            .into_iter()
            .try_fold(state, |next_state, transition_step| {
                let mut manual_state = next_state
                    .apply_shared_decay_drift(kernel_parameters.beta, transition_step.delta_t);
                if let Some(endogenous_event_side) = transition_step.endogenous_event_side {
                    manual_state = manual_state.apply_endogenous_event_jump(
                        endogenous_event_side,
                        kernel_parameters.alpha,
                    );
                }
                if let Some(exogenous_news_severity_mark) =
                    transition_step.exogenous_news_severity_mark
                {
                    return manual_state
                        .apply_exogenous_news_loading_jump(
                            kernel_parameters.rho,
                            exogenous_news_severity_mark,
                        )
                        .map_err(|source| HawkesTransitionError::InvalidNewsSeverityMark {
                            source,
                        });
                }
                Ok(manual_state)
            })
            .expect("manual transition sequence should succeed");

        let composed_state = state
            .apply_transition_sequence(kernel_parameters, &transition_steps)
            .expect("canonical transition sequence should succeed");

        assert_eq!(composed_state, expected_state);
    }

    #[test]
    fn composed_transition_step_rejects_negative_drift_duration() {
        let state = HawkesExcitationState {
            y_plus_plus: 0.0,
            y_plus_minus: 0.0,
            y_minus_plus: 0.0,
            y_minus_minus: 0.0,
        };

        let kernel_parameters = HawkesKernelParameters::try_new(
            0.60,
            1.0,
            HawkesKernelLoadings {
                plus_plus: 0.0,
                plus_minus: 0.0,
                minus_plus: 0.0,
                minus_minus: 0.0,
            },
            HawkesKernelLoadings {
                plus_plus: 0.0,
                plus_minus: 0.0,
                minus_plus: 0.0,
                minus_minus: 0.0,
            },
        )
        .expect("kernel fixture must be valid");

        let error = state
            .apply_transition_step(
                kernel_parameters,
                HawkesTransitionStep::new(-0.01, None, None),
            )
            .expect_err("negative drift duration must fail");

        assert_eq!(error, HawkesTransitionError::InvalidDeltaT { value: -0.01 });
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
    fn structured_alpha_derivation_builds_expected_canonical_matrix() {
        let structured_alpha = HawkesStructuredAlphaParameterization::new(0.75, 0.25, 0.5, 0.5);

        let alpha = HawkesKernelLoadings::try_from_structured_alpha(structured_alpha)
            .expect("valid structured alpha must derive canonical loadings");

        assert_eq!(
            alpha,
            HawkesKernelLoadings {
                plus_plus: 0.75,
                plus_minus: 0.25,
                minus_plus: 0.75,
                minus_minus: 1.25,
            }
        );
        assert_eq!(alpha.as_model_ordered_array(), [0.75, 0.25, 0.75, 1.25]);
    }

    #[test]
    fn structured_alpha_derivation_accepts_zero_delta_boundary_case() {
        let structured_alpha = HawkesStructuredAlphaParameterization::new(0.5, 0.25, 0.0, 0.0);

        let alpha = HawkesKernelLoadings::try_from_structured_alpha(structured_alpha)
            .expect("zero deltas should be valid boundary values");

        assert_eq!(
            alpha,
            HawkesKernelLoadings {
                plus_plus: 0.5,
                plus_minus: 0.25,
                minus_plus: 0.25,
                minus_minus: 0.5,
            }
        );
        assert_eq!(alpha.minus_minus, alpha.plus_plus);
        assert_eq!(alpha.minus_plus, alpha.plus_minus);
    }

    #[test]
    fn structured_alpha_derivation_rejects_invalid_base_and_delta_values() {
        let invalid_a_self_error = HawkesKernelLoadings::try_from_structured_alpha(
            HawkesStructuredAlphaParameterization::new(0.0, 0.1, 0.1, 0.1),
        )
        .expect_err("a_self must be strictly positive");
        assert_eq!(
            invalid_a_self_error,
            HawkesStructuredAlphaParameterError::InvalidASelf { value: 0.0 }
        );

        let invalid_a_cross_error = HawkesKernelLoadings::try_from_structured_alpha(
            HawkesStructuredAlphaParameterization::new(0.2, -0.1, 0.1, 0.1),
        )
        .expect_err("a_cross must be non-negative");
        assert_eq!(
            invalid_a_cross_error,
            HawkesStructuredAlphaParameterError::InvalidACross { value: -0.1 }
        );

        let invalid_delta_self_error = HawkesKernelLoadings::try_from_structured_alpha(
            HawkesStructuredAlphaParameterization::new(0.2, 0.1, -0.001, 0.0),
        )
        .expect_err("delta_self must be non-negative");
        assert_eq!(
            invalid_delta_self_error,
            HawkesStructuredAlphaParameterError::InvalidDeltaSelf { value: -0.001 }
        );

        let invalid_delta_cross_error = HawkesKernelLoadings::try_from_structured_alpha(
            HawkesStructuredAlphaParameterization::new(0.2, 0.1, 0.0, -0.002),
        )
        .expect_err("delta_cross must be non-negative");
        assert_eq!(
            invalid_delta_cross_error,
            HawkesStructuredAlphaParameterError::InvalidDeltaCross { value: -0.002 }
        );
    }

    #[test]
    fn structured_alpha_derivation_rejects_derived_component_overflow() {
        let error = HawkesKernelLoadings::try_from_structured_alpha(
            HawkesStructuredAlphaParameterization::new(f64::MAX, 0.1, f64::MAX, 0.2),
        )
        .expect_err("overflowed derived alpha component must fail");

        assert_eq!(
            error,
            HawkesStructuredAlphaParameterError::InvalidDerivedComponent {
                field: "alpha.minus_minus",
                value: f64::INFINITY,
            }
        );
    }

    #[test]
    fn structured_rho_validation_builds_expected_canonical_matrix_and_ratios() {
        let structured_rho = HawkesStructuredRhoParameterization::new(1.2, 0.3, 0.4, 0.2);

        let validation_output = HawkesKernelLoadings::validate_structured_rho(structured_rho, None)
            .expect("valid structured rho must produce machine-readable validation output");

        assert_eq!(
            validation_output.derived_rho,
            HawkesKernelLoadings {
                plus_plus: 1.2,
                plus_minus: 0.3,
                minus_plus: 0.5,
                minus_minus: 1.6,
            }
        );
        assert_within_tolerance(
            validation_output.target_plus_self_to_cross_ratio,
            4.0,
            DECAY_TOLERANCE,
            "target_plus_self_to_cross_ratio",
        );
        assert_within_tolerance(
            validation_output.target_minus_self_to_cross_ratio,
            3.2,
            DECAY_TOLERANCE,
            "target_minus_self_to_cross_ratio",
        );
        assert_eq!(validation_output.applied_self_dominance_policy, None);

        let derived_rho = HawkesKernelLoadings::try_from_structured_rho(structured_rho, None)
            .expect("valid structured rho should derive canonical loadings");
        assert_eq!(derived_rho, validation_output.derived_rho);
        assert_eq!(derived_rho.as_model_ordered_array(), [1.2, 0.3, 0.5, 1.6]);
    }

    #[test]
    fn structured_rho_validation_accepts_configurable_self_dominance_policy() {
        let structured_rho = HawkesStructuredRhoParameterization::new(2.0, 0.5, 0.4, 0.0);
        let policy = HawkesStructuredRhoSelfDominancePolicy::new(3.5);

        let validation_output =
            HawkesKernelLoadings::validate_structured_rho(structured_rho, Some(policy))
                .expect("rho ratios above threshold should satisfy policy");

        assert_eq!(
            validation_output.applied_self_dominance_policy,
            Some(policy)
        );
        assert_within_tolerance(
            validation_output.target_plus_self_to_cross_ratio,
            4.0,
            DECAY_TOLERANCE,
            "target_plus_self_to_cross_ratio",
        );
        assert_within_tolerance(
            validation_output.target_minus_self_to_cross_ratio,
            4.8,
            DECAY_TOLERANCE,
            "target_minus_self_to_cross_ratio",
        );
    }

    #[test]
    fn structured_rho_validation_rejects_self_dominance_policy_violation() {
        let structured_rho = HawkesStructuredRhoParameterization::new(0.8, 0.5, 0.2, 0.3);
        let policy = HawkesStructuredRhoSelfDominancePolicy::new(1.3);

        let error = HawkesKernelLoadings::validate_structured_rho(structured_rho, Some(policy))
            .expect_err("ratio below configured threshold must fail");

        assert_eq!(
            error,
            HawkesStructuredRhoParameterError::SelfDominancePolicyViolation {
                minimum_self_to_cross_ratio: 1.3,
                target_plus_self_to_cross_ratio: 1.6,
                target_minus_self_to_cross_ratio: 1.25,
            }
        );
    }

    #[test]
    fn structured_rho_derivation_rejects_invalid_base_and_delta_values() {
        let invalid_r_self_error = HawkesKernelLoadings::validate_structured_rho(
            HawkesStructuredRhoParameterization::new(0.0, 0.1, 0.1, 0.1),
            None,
        )
        .expect_err("r_self must be strictly positive");
        assert_eq!(
            invalid_r_self_error,
            HawkesStructuredRhoParameterError::InvalidRSelf { value: 0.0 }
        );

        let invalid_r_cross_error = HawkesKernelLoadings::validate_structured_rho(
            HawkesStructuredRhoParameterization::new(0.2, -0.1, 0.1, 0.1),
            None,
        )
        .expect_err("r_cross must be non-negative");
        assert_eq!(
            invalid_r_cross_error,
            HawkesStructuredRhoParameterError::InvalidRCross { value: -0.1 }
        );

        let invalid_delta_self_r_error = HawkesKernelLoadings::validate_structured_rho(
            HawkesStructuredRhoParameterization::new(0.2, 0.1, -0.001, 0.0),
            None,
        )
        .expect_err("delta_self_r must be non-negative");
        assert_eq!(
            invalid_delta_self_r_error,
            HawkesStructuredRhoParameterError::InvalidDeltaSelfR { value: -0.001 }
        );

        let invalid_delta_cross_r_error = HawkesKernelLoadings::validate_structured_rho(
            HawkesStructuredRhoParameterization::new(0.2, 0.1, 0.0, -0.002),
            None,
        )
        .expect_err("delta_cross_r must be non-negative");
        assert_eq!(
            invalid_delta_cross_r_error,
            HawkesStructuredRhoParameterError::InvalidDeltaCrossR { value: -0.002 }
        );
    }

    #[test]
    fn structured_rho_validation_rejects_derived_component_overflow() {
        let error = HawkesKernelLoadings::validate_structured_rho(
            HawkesStructuredRhoParameterization::new(f64::MAX, 0.1, f64::MAX, 0.2),
            None,
        )
        .expect_err("overflowed derived rho component must fail");

        assert_eq!(
            error,
            HawkesStructuredRhoParameterError::InvalidDerivedComponent {
                field: "rho.minus_minus",
                value: f64::INFINITY,
            }
        );
    }

    #[test]
    fn structured_rho_validation_rejects_invalid_self_dominance_policy_threshold() {
        let error = HawkesKernelLoadings::validate_structured_rho(
            HawkesStructuredRhoParameterization::new(0.5, 0.25, 0.1, 0.1),
            Some(HawkesStructuredRhoSelfDominancePolicy::new(0.99)),
        )
        .expect_err("minimum_self_to_cross_ratio < 1.0 must fail");

        assert_eq!(
            error,
            HawkesStructuredRhoParameterError::InvalidSelfDominanceRatioThreshold { value: 0.99 }
        );
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

        let parameters = HawkesKernelParameters::try_new(0.85, 1.25, alpha, rho)
            .expect("valid Hawkes kernel payload should construct");

        assert_eq!(parameters.lambda_bar, 0.85);
        assert_eq!(parameters.beta, 1.25);
        assert_eq!(parameters.alpha, alpha);
        assert_eq!(parameters.rho, rho);
    }

    #[test]
    fn kernel_parameter_domain_precheck_accepts_interior_and_boundary_adjacent_values() {
        let alpha = HawkesKernelLoadings {
            plus_plus: f64::EPSILON * 0.5,
            plus_minus: 0.0,
            minus_plus: f64::EPSILON * 0.5,
            minus_minus: f64::EPSILON * 0.5,
        };
        let rho = HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.0,
            minus_plus: 0.25,
            minus_minus: 0.0,
        };

        let interior_report = HawkesKernelParameters::validate_domain(0.85, 1.25, alpha, rho);
        assert!(interior_report.is_valid());
        assert_eq!(
            interior_report.first_violation(),
            None,
            "valid interior payload should not report violations"
        );

        let boundary_adjacent_report =
            HawkesKernelParameters::validate_domain(f64::EPSILON, f64::EPSILON, alpha, rho);
        assert!(boundary_adjacent_report.is_valid());
        assert_eq!(
            boundary_adjacent_report.violations,
            Vec::<HawkesKernelParameterViolation>::new(),
            "strict positivity must accept tiny positive values at the boundary"
        );
    }

    #[test]
    fn kernel_parameter_domain_precheck_returns_optimizer_friendly_report_payload_for_valid_input()
    {
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

        let report = HawkesKernelParameters::validate_domain(0.85, 1.25, alpha, rho);

        assert_eq!(
            report.schema_version,
            HAWKES_KERNEL_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION
        );
        assert_eq!(
            report.determinism_policy,
            HAWKES_KERNEL_PARAMETER_VALIDATION_DETERMINISM_POLICY
        );
        assert_eq!(report.status, HawkesKernelValidationStatus::Pass);
        assert_eq!(report.summary.total_check_count, 12);
        assert_eq!(report.summary.passed_check_count, 12);
        assert_eq!(report.summary.failed_check_count, 0);
        assert_eq!(report.derived_alpha, None);
        assert_eq!(report.derived_rho, None);
        assert_eq!(report.checks.len(), report.summary.total_check_count);
        assert!(
            report
                .checks
                .iter()
                .all(|check| check.status == HawkesKernelValidationStatus::Pass)
        );
    }

    #[test]
    fn structured_domain_validation_report_orders_multi_failure_checks_deterministically() {
        let report = HawkesKernelParameters::validate_structured_domain_with_stability_tolerance(
            0.0,
            -0.5,
            HawkesStructuredAlphaParameterization::new(0.0, -0.1, -0.2, -0.3),
            HawkesStructuredRhoParameterization::new(0.0, -0.2, -0.1, -0.4),
            Some(HawkesStructuredRhoSelfDominancePolicy::new(0.9)),
            -1.0,
        );

        assert_eq!(report.status, HawkesKernelValidationStatus::Fail);
        assert_eq!(report.derived_alpha, None);
        assert_eq!(report.derived_rho, None);
        assert_eq!(report.summary.failed_check_count, report.violations.len());
        assert_eq!(
            report
                .checks
                .iter()
                .filter(|check| check.status == HawkesKernelValidationStatus::Fail)
                .map(|check| check.check_id)
                .collect::<Vec<_>>(),
            vec![
                "positivity.lambda_bar",
                "positivity.beta",
                "positivity.structured_alpha.a_self",
                "positivity.structured_alpha.a_cross",
                "positivity.structured_alpha.delta_self",
                "positivity.structured_alpha.delta_cross",
                "positivity.alpha.plus_minus",
                "positivity.alpha.minus_plus",
                "positivity.alpha.minus_minus",
                "positivity.structured_rho.r_self",
                "positivity.structured_rho.r_cross",
                "positivity.structured_rho.delta_self_r",
                "positivity.structured_rho.delta_cross_r",
                "positivity.rho.plus_minus",
                "positivity.rho.minus_plus",
                "positivity.rho.minus_minus",
                "structural.rho.self_dominance.minimum_ratio",
                "stability.numeric_tolerance",
            ]
        );
    }

    #[test]
    fn kernel_parameter_domain_precheck_reports_stability_violation_and_spectral_radius() {
        let alpha = HawkesKernelLoadings {
            plus_plus: 0.9,
            plus_minus: 0.2,
            minus_plus: 0.1,
            minus_minus: 0.8,
        };
        let rho = HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.0,
            minus_plus: 0.0,
            minus_minus: 0.0,
        };

        let report = HawkesKernelParameters::validate_domain(0.85, 0.8, alpha, rho);

        assert!(!report.is_valid());
        assert_eq!(
            report.violations,
            vec![HawkesKernelParameterViolation {
                field: "alpha_over_beta.spectral_radius",
                constraint: HawkesKernelParameterConstraint::SpectralRadiusStrictlyBelowOne,
                value: 1.25,
            }]
        );
    }

    #[test]
    fn spectral_radius_stability_checker_supports_tolerance_policy_near_threshold() {
        let alpha = HawkesKernelLoadings {
            plus_plus: 0.999_999_999_6,
            plus_minus: 0.0,
            minus_plus: 0.0,
            minus_minus: 0.2,
        };

        let strict_check = HawkesKernelParameters::check_spectral_radius_stability(1.0, alpha, 0.0)
            .expect("strict threshold check should evaluate");
        assert_eq!(
            strict_check.spectral_radius_alpha_over_beta,
            0.999_999_999_6
        );
        assert!(strict_check.is_stable);

        let tolerant_check =
            HawkesKernelParameters::check_spectral_radius_stability(1.0, alpha, 1e-9)
                .expect("tightened tolerance policy should evaluate");
        assert_eq!(
            tolerant_check.spectral_radius_alpha_over_beta,
            0.999_999_999_6
        );
        assert_eq!(
            tolerant_check.effective_stationarity_threshold,
            HAWKES_STRICT_STATIONARITY_THRESHOLD - 1e-9
        );
        assert!(!tolerant_check.is_stable);
    }

    #[test]
    fn spectral_radius_stability_checker_reports_deterministic_numeric_output() {
        let alpha = HawkesKernelLoadings {
            plus_plus: 0.6,
            plus_minus: 0.1,
            minus_plus: 0.2,
            minus_minus: 0.7,
        };

        let reference_check =
            HawkesKernelParameters::check_spectral_radius_stability(1.25, alpha, 0.0)
                .expect("valid payload should evaluate deterministic spectral radius");
        assert!(reference_check.is_stable);
        assert_eq!(reference_check.spectral_radius_alpha_over_beta, 0.64);

        for _ in 0..16 {
            let repeated_check =
                HawkesKernelParameters::check_spectral_radius_stability(1.25, alpha, 0.0)
                    .expect("repeated checks should not fail");

            assert_eq!(
                repeated_check.spectral_radius_alpha_over_beta.to_bits(),
                reference_check.spectral_radius_alpha_over_beta.to_bits()
            );
            assert_eq!(repeated_check, reference_check);
        }
    }

    #[test]
    fn kernel_parameter_domain_precheck_reports_exact_violation_payload_structure() {
        let alpha = HawkesKernelLoadings {
            plus_plus: 0.6,
            plus_minus: -0.01,
            minus_plus: 0.2,
            minus_minus: 0.7,
        };
        let rho = HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.05,
            minus_plus: 0.12,
            minus_minus: -0.001,
        };

        let report = HawkesKernelParameters::validate_domain(0.0, -0.5, alpha, rho);

        assert!(!report.is_valid());
        assert_eq!(
            report.violations,
            vec![
                HawkesKernelParameterViolation {
                    field: "lambda_bar",
                    constraint: HawkesKernelParameterConstraint::FiniteAndStrictlyPositive,
                    value: 0.0,
                },
                HawkesKernelParameterViolation {
                    field: "beta",
                    constraint: HawkesKernelParameterConstraint::FiniteAndStrictlyPositive,
                    value: -0.5,
                },
                HawkesKernelParameterViolation {
                    field: "alpha.plus_minus",
                    constraint: HawkesKernelParameterConstraint::FiniteAndNonNegative,
                    value: -0.01,
                },
                HawkesKernelParameterViolation {
                    field: "rho.minus_minus",
                    constraint: HawkesKernelParameterConstraint::FiniteAndNonNegative,
                    value: -0.001,
                },
            ]
        );
        assert_eq!(
            report.first_violation(),
            Some(HawkesKernelParameterViolation {
                field: "lambda_bar",
                constraint: HawkesKernelParameterConstraint::FiniteAndStrictlyPositive,
                value: 0.0,
            })
        );
    }

    #[test]
    fn kernel_parameter_construction_rejects_non_positive_or_non_finite_lambda_bar() {
        let loadings = HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.0,
            minus_plus: 0.0,
            minus_minus: 0.0,
        };

        let zero_lambda_bar_error = HawkesKernelParameters::try_new(0.0, 1.0, loadings, loadings)
            .expect_err("lambda_bar=0 must fail");
        assert_eq!(
            zero_lambda_bar_error,
            HawkesKernelParameterError::InvalidLambdaBar { value: 0.0 }
        );

        let nan_lambda_bar_error =
            HawkesKernelParameters::try_new(f64::NAN, 1.0, loadings, loadings)
                .expect_err("non-finite lambda_bar must fail");
        assert!(matches!(
            nan_lambda_bar_error,
            HawkesKernelParameterError::InvalidLambdaBar { value } if value.is_nan()
        ));
    }

    #[test]
    fn kernel_parameter_construction_rejects_non_positive_or_non_finite_beta() {
        let loadings = HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.0,
            minus_plus: 0.0,
            minus_minus: 0.0,
        };

        let zero_beta_error = HawkesKernelParameters::try_new(1.0, 0.0, loadings, loadings)
            .expect_err("beta=0 must fail");
        assert_eq!(
            zero_beta_error,
            HawkesKernelParameterError::InvalidBeta { value: 0.0 }
        );

        let nan_beta_error = HawkesKernelParameters::try_new(1.0, f64::NAN, loadings, loadings)
            .expect_err("non-finite beta must fail");
        assert!(matches!(
            nan_beta_error,
            HawkesKernelParameterError::InvalidBeta { value } if value.is_nan()
        ));
    }

    #[test]
    fn kernel_parameter_construction_rejects_unstable_spectral_radius() {
        let alpha = HawkesKernelLoadings {
            plus_plus: 0.9,
            plus_minus: 0.2,
            minus_plus: 0.1,
            minus_minus: 0.8,
        };
        let rho = HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.0,
            minus_plus: 0.0,
            minus_minus: 0.0,
        };

        let error = HawkesKernelParameters::try_new(1.0, 0.8, alpha, rho)
            .expect_err("unstable spectral radius must fail construction");

        assert_eq!(
            error,
            HawkesKernelParameterError::UnstableSpectralRadius {
                spectral_radius_alpha_over_beta: 1.25,
                strict_stationarity_threshold: 1.0,
                numeric_tolerance: 0.0,
                effective_stationarity_threshold: 1.0,
            }
        );
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

        let error = HawkesKernelParameters::try_new(1.0, 0.9, alpha, rho)
            .expect_err("negative alpha must fail");

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

        let error = HawkesKernelParameters::try_new(1.0, 0.9, alpha, rho)
            .expect_err("non-finite rho must fail");

        assert_eq!(
            error,
            HawkesKernelParameterError::InvalidLoadingComponent {
                field: "rho.minus_minus",
                value: f64::INFINITY,
            }
        );
    }

    #[test]
    fn kernel_parameter_builder_from_target_components_preserves_mapping() {
        let parameters = HawkesKernelParameters::try_new_from_target_components(
            0.85,
            1.25,
            HawkesSourceComponents::new(0.6, 0.1),
            HawkesSourceComponents::new(0.2, 0.7),
            HawkesSourceComponents::new(0.0, 0.05),
            HawkesSourceComponents::new(0.12, 0.0),
        )
        .expect("typed component groups must construct canonical parameters");

        assert_eq!(parameters.lambda_bar, 0.85);
        assert_eq!(parameters.beta, 1.25);
        assert_eq!(
            parameters.alpha,
            HawkesKernelLoadings {
                plus_plus: 0.6,
                plus_minus: 0.1,
                minus_plus: 0.2,
                minus_minus: 0.7,
            }
        );
        assert_eq!(
            parameters.rho,
            HawkesKernelLoadings {
                plus_plus: 0.0,
                plus_minus: 0.05,
                minus_plus: 0.12,
                minus_minus: 0.0,
            }
        );
    }

    #[test]
    fn kernel_parameter_serialization_round_trip_preserves_canonical_payload() {
        let parameters = HawkesKernelParameters::try_new_from_target_components(
            0.85,
            1.25,
            HawkesSourceComponents::new(0.6, 0.1),
            HawkesSourceComponents::new(0.2, 0.7),
            HawkesSourceComponents::new(0.0, 0.05),
            HawkesSourceComponents::new(0.12, 0.0),
        )
        .expect("valid typed parameters must serialize");

        let serialized = serde_json::to_string(&parameters)
            .expect("canonical parameter payload should serialize");
        let parsed: HawkesKernelParameters =
            serde_json::from_str(&serialized).expect("serialized payload should deserialize");

        assert_eq!(parsed, parameters);
    }

    #[test]
    fn kernel_parameter_deserialization_reports_missing_required_field() {
        let payload = json!({
            "lambda_bar": 0.85,
            "beta": 1.25,
            "alpha": {
                "plus_plus": 0.6,
                "plus_minus": 0.1,
                "minus_plus": 0.2,
                "minus_minus": 0.7
            }
        });

        let parse_result = serde_json::from_value::<HawkesKernelParameters>(payload);
        let error_text = parse_result
            .expect_err("missing rho field must fail")
            .to_string();

        assert!(
            error_text.contains("missing field `rho`"),
            "unexpected error message: {error_text}"
        );
    }

    #[test]
    fn kernel_parameter_deserialization_reports_malformed_field_type() {
        let payload = json!({
            "lambda_bar": 0.85,
            "beta": "fast",
            "alpha": {
                "plus_plus": 0.6,
                "plus_minus": 0.1,
                "minus_plus": 0.2,
                "minus_minus": 0.7
            },
            "rho": {
                "plus_plus": 0.0,
                "plus_minus": 0.05,
                "minus_plus": 0.12,
                "minus_minus": 0.0
            }
        });

        let parse_result = serde_json::from_value::<HawkesKernelParameters>(payload);
        let error_text = parse_result
            .expect_err("malformed beta field type must fail")
            .to_string();

        assert!(
            error_text.contains("invalid type"),
            "unexpected error message: {error_text}"
        );
    }
}
