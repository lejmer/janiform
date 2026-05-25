use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use janiform_foundations::numeric::{
    validate_finite_number, validate_non_negative_number, validate_positive_number,
};

/// Fitted exposure parameters mapping endogenous activity into return-clock time.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReturnClockExposureParameters {
    pub buy_excitation_return_clock_scale: f64,
    pub sell_excitation_return_clock_scale: f64,
}

impl ReturnClockExposureParameters {
    /// Return the neutral exposure where endogenous activity enters the return clock unchanged.
    pub const fn neutral() -> Self {
        Self {
            buy_excitation_return_clock_scale: 1.0,
            sell_excitation_return_clock_scale: 1.0,
        }
    }

    /// Construct exposure parameters without validation.
    pub const fn new(
        buy_excitation_return_clock_scale: f64,
        sell_excitation_return_clock_scale: f64,
    ) -> Self {
        Self {
            buy_excitation_return_clock_scale,
            sell_excitation_return_clock_scale,
        }
    }

    /// Construct validated positive exposure parameters.
    pub fn try_new(
        buy_excitation_return_clock_scale: f64,
        sell_excitation_return_clock_scale: f64,
    ) -> Result<Self, ReturnClockExposureParametersError> {
        let parameters = Self::new(
            buy_excitation_return_clock_scale,
            sell_excitation_return_clock_scale,
        );
        parameters.validate()?;

        Ok(parameters)
    }

    /// Validate both exposure parameters.
    pub fn validate(self) -> Result<(), ReturnClockExposureParametersError> {
        validate_positive_number(
            self.buy_excitation_return_clock_scale,
            "buy_excitation_return_clock_scale",
        )
        .map_err(|source| ReturnClockExposureParametersError::InvalidPositiveScale { source })?;
        validate_positive_number(
            self.sell_excitation_return_clock_scale,
            "sell_excitation_return_clock_scale",
        )
        .map_err(|source| ReturnClockExposureParametersError::InvalidPositiveScale { source })?;

        Ok(())
    }
}

/// Baseline/endogenous decomposition of one side's integrated activity.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntegratedSideActivityDecomposition {
    pub baseline_activity: f64,
    pub endogenous_activity: f64,
}

impl IntegratedSideActivityDecomposition {
    /// Construct decomposed integrated activity without validation.
    pub const fn new(baseline_activity: f64, endogenous_activity: f64) -> Self {
        Self {
            baseline_activity,
            endogenous_activity,
        }
    }

    /// Construct validated non-negative decomposed integrated activity.
    pub fn try_new(
        baseline_activity: f64,
        endogenous_activity: f64,
    ) -> Result<Self, IntegratedSideActivityDecompositionError> {
        let decomposition = Self::new(baseline_activity, endogenous_activity);
        decomposition.validate()?;

        Ok(decomposition)
    }

    /// Validate the decomposed activity.
    pub fn validate(self) -> Result<(), IntegratedSideActivityDecompositionError> {
        validate_non_negative_number(self.baseline_activity, "baseline_activity").map_err(
            |source| IntegratedSideActivityDecompositionError::InvalidNonNegativeActivity {
                source,
            },
        )?;

        validate_non_negative_number(self.endogenous_activity, "endogenous_activity").map_err(
            |source| IntegratedSideActivityDecompositionError::InvalidNonNegativeActivity {
                source,
            },
        )?;

        validate_finite_number(self.total_activity(), "total_activity").map_err(|source| {
            IntegratedSideActivityDecompositionError::InvalidAggregateActivity { source }
        })?;

        Ok(())
    }

    /// Return total integrated activity for callers that still need the Hawkes activity measure.
    pub fn total_activity(self) -> f64 {
        self.baseline_activity + self.endogenous_activity
    }

    /// Return the effective NIG return clock for this side.
    pub fn effective_return_clock(
        self,
        excitation_return_clock_scale: f64,
    ) -> Result<f64, IntegratedSideActivityDecompositionError> {
        validate_positive_number(
            excitation_return_clock_scale,
            "excitation_return_clock_scale",
        )
        .map_err(|source| {
            IntegratedSideActivityDecompositionError::InvalidPositiveScale { source }
        })?;

        let return_clock =
            self.baseline_activity + excitation_return_clock_scale * self.endogenous_activity;

        validate_non_negative_number(return_clock, "return_clock").map_err(|source| {
            IntegratedSideActivityDecompositionError::InvalidReturnClock { source }
        })?;

        Ok(return_clock)
    }
}

/// Buy/sell effective NIG return clocks after applying return-clock exposure.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveReturnClocks {
    pub buy_return_clock: f64,
    pub sell_return_clock: f64,
}

impl EffectiveReturnClocks {
    /// Construct validated effective return clocks.
    pub fn try_new(
        buy_return_clock: f64,
        sell_return_clock: f64,
    ) -> Result<Self, EffectiveReturnClocksError> {
        validate_non_negative_number(buy_return_clock, "buy_return_clock")
            .map_err(|source| EffectiveReturnClocksError::InvalidNonNegativeClock { source })?;

        validate_non_negative_number(sell_return_clock, "sell_return_clock")
            .map_err(|source| EffectiveReturnClocksError::InvalidNonNegativeClock { source })?;

        Ok(Self {
            buy_return_clock,
            sell_return_clock,
        })
    }
}

/// Build buy/sell effective return clocks from decomposed integrated activity.
pub fn compute_effective_return_clocks(
    exposure_parameters: ReturnClockExposureParameters,
    buy_activity: IntegratedSideActivityDecomposition,
    sell_activity: IntegratedSideActivityDecomposition,
) -> Result<EffectiveReturnClocks, EffectiveReturnClocksBuildError> {
    exposure_parameters
        .validate()
        .map_err(|source| EffectiveReturnClocksBuildError::InvalidExposureParameters { source })?;

    buy_activity
        .validate()
        .map_err(|source| EffectiveReturnClocksBuildError::InvalidBuyActivity { source })?;

    sell_activity
        .validate()
        .map_err(|source| EffectiveReturnClocksBuildError::InvalidSellActivity { source })?;

    let buy_return_clock = buy_activity
        .effective_return_clock(exposure_parameters.buy_excitation_return_clock_scale)
        .map_err(|source| EffectiveReturnClocksBuildError::InvalidBuyActivity { source })?;

    let sell_return_clock = sell_activity
        .effective_return_clock(exposure_parameters.sell_excitation_return_clock_scale)
        .map_err(|source| EffectiveReturnClocksBuildError::InvalidSellActivity { source })?;

    EffectiveReturnClocks::try_new(buy_return_clock, sell_return_clock)
        .map_err(|source| EffectiveReturnClocksBuildError::InvalidEffectiveClocks { source })
}

/// Compute the stationary total return clock per unit buy/sell baseline intensity.
///
/// The stationary multipliers are expressed relative to each side's baseline
/// intensity, so each side has unit baseline activity and an endogenous
/// component equal to `stationary_multiplier - 1`.
pub fn stationary_total_return_clock(
    exposure_parameters: ReturnClockExposureParameters,
    stationary_buy_intensity_multiplier: f64,
    stationary_sell_intensity_multiplier: f64,
) -> Option<f64> {
    let buy_activity = IntegratedSideActivityDecomposition::try_new(
        1.0,
        stationary_buy_intensity_multiplier - 1.0,
    )
    .ok()?;

    let sell_activity = IntegratedSideActivityDecomposition::try_new(
        1.0,
        stationary_sell_intensity_multiplier - 1.0,
    )
    .ok()?;

    let effective_clocks =
        compute_effective_return_clocks(exposure_parameters, buy_activity, sell_activity).ok()?;

    let total_return_clock = effective_clocks.buy_return_clock + effective_clocks.sell_return_clock;

    total_return_clock.is_finite().then_some(total_return_clock)
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReturnClockExposureParametersError {
    InvalidPositiveScale {
        source: janiform_foundations::numeric::PositiveNumberValidationError,
    },
}

impl Display for ReturnClockExposureParametersError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPositiveScale { source } => write!(formatter, "{source}"),
        }
    }
}

impl Error for ReturnClockExposureParametersError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPositiveScale { source } => Some(source),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum IntegratedSideActivityDecompositionError {
    InvalidNonNegativeActivity {
        source: janiform_foundations::numeric::NonNegativeNumberValidationError,
    },
    InvalidAggregateActivity {
        source: janiform_foundations::numeric::FiniteNumberValidationError,
    },
    InvalidPositiveScale {
        source: janiform_foundations::numeric::PositiveNumberValidationError,
    },
    InvalidReturnClock {
        source: janiform_foundations::numeric::NonNegativeNumberValidationError,
    },
}

impl Display for IntegratedSideActivityDecompositionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidNonNegativeActivity { source } => write!(formatter, "{source}"),
            Self::InvalidAggregateActivity { source } => write!(formatter, "{source}"),
            Self::InvalidPositiveScale { source } => write!(formatter, "{source}"),
            Self::InvalidReturnClock { source } => write!(formatter, "{source}"),
        }
    }
}

impl Error for IntegratedSideActivityDecompositionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidNonNegativeActivity { source } => Some(source),
            Self::InvalidAggregateActivity { source } => Some(source),
            Self::InvalidPositiveScale { source } => Some(source),
            Self::InvalidReturnClock { source } => Some(source),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum EffectiveReturnClocksError {
    InvalidNonNegativeClock {
        source: janiform_foundations::numeric::NonNegativeNumberValidationError,
    },
}

impl Display for EffectiveReturnClocksError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidNonNegativeClock { source } => write!(formatter, "{source}"),
        }
    }
}

impl Error for EffectiveReturnClocksError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidNonNegativeClock { source } => Some(source),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum EffectiveReturnClocksBuildError {
    InvalidExposureParameters {
        source: ReturnClockExposureParametersError,
    },
    InvalidBuyActivity {
        source: IntegratedSideActivityDecompositionError,
    },
    InvalidSellActivity {
        source: IntegratedSideActivityDecompositionError,
    },
    InvalidEffectiveClocks {
        source: EffectiveReturnClocksError,
    },
}

impl Display for EffectiveReturnClocksBuildError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidExposureParameters { source } => {
                write!(
                    formatter,
                    "return-clock exposure parameters are invalid: {source}"
                )
            }
            Self::InvalidBuyActivity { source } => {
                write!(formatter, "buy activity decomposition is invalid: {source}")
            }
            Self::InvalidSellActivity { source } => {
                write!(
                    formatter,
                    "sell activity decomposition is invalid: {source}"
                )
            }
            Self::InvalidEffectiveClocks { source } => {
                write!(formatter, "effective return clocks are invalid: {source}")
            }
        }
    }
}

impl Error for EffectiveReturnClocksBuildError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidExposureParameters { source } => Some(source),
            Self::InvalidBuyActivity { source } => Some(source),
            Self::InvalidSellActivity { source } => Some(source),
            Self::InvalidEffectiveClocks { source } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        IntegratedSideActivityDecomposition, ReturnClockExposureParameters,
        compute_effective_return_clocks, stationary_total_return_clock,
    };

    #[test]
    fn neutral_exposure_preserves_total_activity() {
        let clocks = compute_effective_return_clocks(
            ReturnClockExposureParameters::neutral(),
            IntegratedSideActivityDecomposition::try_new(2.0, 3.0)
                .expect("buy decomposition should construct"),
            IntegratedSideActivityDecomposition::try_new(5.0, 7.0)
                .expect("sell decomposition should construct"),
        )
        .expect("effective return clocks should construct");

        assert_eq!(clocks.buy_return_clock, 5.0);
        assert_eq!(clocks.sell_return_clock, 12.0);
    }

    #[test]
    fn exposure_scales_only_endogenous_activity() {
        let clocks = compute_effective_return_clocks(
            ReturnClockExposureParameters::try_new(2.0, 0.5)
                .expect("exposure parameters should construct"),
            IntegratedSideActivityDecomposition::try_new(2.0, 3.0)
                .expect("buy decomposition should construct"),
            IntegratedSideActivityDecomposition::try_new(5.0, 8.0)
                .expect("sell decomposition should construct"),
        )
        .expect("effective return clocks should construct");

        assert_eq!(clocks.buy_return_clock, 8.0);
        assert_eq!(clocks.sell_return_clock, 9.0);
    }

    #[test]
    fn stationary_total_return_clock_uses_unit_baseline_activity() {
        let total_return_clock = stationary_total_return_clock(
            ReturnClockExposureParameters::try_new(2.0, 0.5)
                .expect("exposure parameters should construct"),
            3.0,
            5.0,
        )
        .expect("stationary total return clock should construct");

        assert_eq!(total_return_clock, 8.0);
    }
}
