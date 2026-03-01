use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::hourly_factors::HourlyFactorContractOutput;

use super::{HawkesKernelLoadings, HawkesKernelParameters};
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

/// Apply one Hawkes transition step using canonical drift and jump call order.
pub fn evolve_hawkes_state(
    state: HawkesExcitationState,
    kernel_parameters: HawkesKernelParameters,
    transition_step: HawkesTransitionStep,
) -> Result<HawkesExcitationState, HawkesTransitionError> {
    state.apply_transition_step(kernel_parameters, transition_step)
}
