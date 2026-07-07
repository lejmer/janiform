use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use janiform_foundations::numeric::{
    validate_finite_number, validate_non_negative_number, validate_positive_number,
};

use crate::hawkes::{
    HawkesEventSide, HawkesKernelParameters, HawkesStationaryIntensityMultipliers,
    HawkesStationaryIntensityRates, HawkesStationaryTargetSourceRates, stationary_intensity_rates,
};
use crate::nig::NigBuySellDriverParameters;
use crate::return_clock::{
    IntegratedSideActivityDecomposition, ReturnClockExposureParameters,
    compute_effective_return_clocks,
};

// With zero diffusion and zero event loadings, unit initial state remains fixed for any
// positive mean-reversion rate. The neutral contract uses one inverse hour as its valid
// inert rate.
const NEUTRAL_LONG_VARIANCE_MEAN_REVERSION_RATE: f64 = 1.0;

/// Physical long-variance parameters for the return-clock activity state.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LongVarianceParameters {
    pub mean_reversion_rate: f64,
    pub vol_of_vol: f64,
    pub stationary_return_energy_rate_per_hour: f64,
    pub return_energy_loading: f64,
    pub buy_event_loading: f64,
    pub sell_event_loading: f64,
}

impl LongVarianceParameters {
    /// Return the neutral unit-variance configuration.
    pub const fn neutral() -> Self {
        Self {
            mean_reversion_rate: NEUTRAL_LONG_VARIANCE_MEAN_REVERSION_RATE,
            vol_of_vol: 0.0,
            stationary_return_energy_rate_per_hour: 0.0,
            return_energy_loading: 0.0,
            buy_event_loading: 0.0,
            sell_event_loading: 0.0,
        }
    }

    /// Construct parameters without validation.
    pub const fn new(
        mean_reversion_rate: f64,
        vol_of_vol: f64,
        stationary_return_energy_rate_per_hour: f64,
        return_energy_loading: f64,
        buy_event_loading: f64,
        sell_event_loading: f64,
    ) -> Self {
        Self {
            mean_reversion_rate,
            vol_of_vol,
            stationary_return_energy_rate_per_hour,
            return_energy_loading,
            buy_event_loading,
            sell_event_loading,
        }
    }

    /// Construct validated parameters.
    pub fn try_new(
        mean_reversion_rate: f64,
        vol_of_vol: f64,
        stationary_return_energy_rate_per_hour: f64,
        return_energy_loading: f64,
        buy_event_loading: f64,
        sell_event_loading: f64,
    ) -> Result<Self, LongVarianceParametersError> {
        let parameters = Self::new(
            mean_reversion_rate,
            vol_of_vol,
            stationary_return_energy_rate_per_hour,
            return_energy_loading,
            buy_event_loading,
            sell_event_loading,
        );
        parameters.validate()?;

        Ok(parameters)
    }

    /// Validate physical parameter constraints.
    pub fn validate(self) -> Result<(), LongVarianceParametersError> {
        validate_positive_number(self.mean_reversion_rate, "mean_reversion_rate").map_err(
            |source| LongVarianceParametersError::InvalidPositiveParameter {
                field: "mean_reversion_rate",
                source,
            },
        )?;

        validate_non_negative_number(self.vol_of_vol, "vol_of_vol").map_err(|source| {
            LongVarianceParametersError::InvalidNonNegativeParameter {
                field: "vol_of_vol",
                source,
            }
        })?;

        validate_non_negative_number(
            self.stationary_return_energy_rate_per_hour,
            "stationary_return_energy_rate_per_hour",
        )
        .map_err(
            |source| LongVarianceParametersError::InvalidNonNegativeParameter {
                field: "stationary_return_energy_rate_per_hour",
                source,
            },
        )?;

        validate_non_negative_number(self.return_energy_loading, "return_energy_loading").map_err(
            |source| LongVarianceParametersError::InvalidNonNegativeParameter {
                field: "return_energy_loading",
                source,
            },
        )?;

        validate_non_negative_number(self.buy_event_loading, "buy_event_loading").map_err(
            |source| LongVarianceParametersError::InvalidNonNegativeParameter {
                field: "buy_event_loading",
                source,
            },
        )?;

        validate_non_negative_number(self.sell_event_loading, "sell_event_loading").map_err(
            |source| LongVarianceParametersError::InvalidNonNegativeParameter {
                field: "sell_event_loading",
                source,
            },
        )?;

        Ok(())
    }

    /// Derive the CIR mean-reversion level that keeps `E[Z] = 1`.
    pub fn stationary_mean_reversion_level(
        self,
        baseline_intensity: f64,
        stationary_intensity_multipliers: HawkesStationaryIntensityMultipliers,
    ) -> Result<f64, LongVarianceStationaryLevelError> {
        self.validate().map_err(|source| {
            LongVarianceStationaryLevelError::InvalidLongVarianceParameters { source }
        })?;

        validate_positive_number(baseline_intensity, "baseline_intensity").map_err(|source| {
            LongVarianceStationaryLevelError::InvalidBaselineIntensity { source }
        })?;

        let stationary_buy_rate =
            baseline_intensity * stationary_intensity_multipliers.buy_intensity_multiplier;

        let stationary_sell_rate =
            baseline_intensity * stationary_intensity_multipliers.sell_intensity_multiplier;

        validate_non_negative_number(stationary_buy_rate, "stationary_buy_event_rate").map_err(
            |source| LongVarianceStationaryLevelError::InvalidStationaryEventRate {
                field: "stationary_buy_event_rate",
                source,
            },
        )?;

        validate_non_negative_number(stationary_sell_rate, "stationary_sell_event_rate").map_err(
            |source| LongVarianceStationaryLevelError::InvalidStationaryEventRate {
                field: "stationary_sell_event_rate",
                source,
            },
        )?;

        let feedback_loading_drift =
            self.stationary_feedback_loading_drift(stationary_buy_rate, stationary_sell_rate);

        let level = 1.0 - feedback_loading_drift / self.mean_reversion_rate;

        validate_positive_number(level, "stationary_mean_reversion_level").map_err(|source| {
            LongVarianceStationaryLevelError::InvalidDerivedMeanReversionLevel { source }
        })?;

        Ok(level)
    }

    /// Derive the CIR mean-reversion level from stationary event rates.
    pub fn stationary_mean_reversion_level_for_event_rates(
        self,
        stationary_intensity_rates: HawkesStationaryIntensityRates,
    ) -> Result<f64, LongVarianceStationaryLevelError> {
        self.validate().map_err(|source| {
            LongVarianceStationaryLevelError::InvalidLongVarianceParameters { source }
        })?;

        validate_non_negative_number(
            stationary_intensity_rates.buy_event_rate_per_hour,
            "stationary_buy_event_rate",
        )
        .map_err(|source| {
            LongVarianceStationaryLevelError::InvalidStationaryEventRate {
                field: "stationary_buy_event_rate",
                source,
            }
        })?;

        validate_non_negative_number(
            stationary_intensity_rates.sell_event_rate_per_hour,
            "stationary_sell_event_rate",
        )
        .map_err(|source| {
            LongVarianceStationaryLevelError::InvalidStationaryEventRate {
                field: "stationary_sell_event_rate",
                source,
            }
        })?;

        let feedback_loading_drift = self.stationary_feedback_loading_drift(
            stationary_intensity_rates.buy_event_rate_per_hour,
            stationary_intensity_rates.sell_event_rate_per_hour,
        );

        let level = 1.0 - feedback_loading_drift / self.mean_reversion_rate;

        validate_positive_number(level, "stationary_mean_reversion_level").map_err(|source| {
            LongVarianceStationaryLevelError::InvalidDerivedMeanReversionLevel { source }
        })?;

        Ok(level)
    }

    /// Derive the stationary mean-reversion level from a mean-normalized Hawkes kernel.
    pub fn stationary_mean_reversion_level_for_hawkes_kernel(
        self,
        kernel_parameters: HawkesKernelParameters,
        exogenous_source_rates: HawkesStationaryTargetSourceRates,
    ) -> Result<f64, LongVarianceStationaryLevelForHawkesKernelError> {
        let stationary_event_rates = stationary_intensity_rates(
            kernel_parameters.alpha,
            kernel_parameters.beta,
            kernel_parameters.lambda_bar,
            exogenous_source_rates,
        )
        .ok_or(
            LongVarianceStationaryLevelForHawkesKernelError::InvalidStationaryIntensityMultipliers,
        )?;

        self.stationary_mean_reversion_level_for_event_rates(stationary_event_rates)
            .map_err(|source| {
                LongVarianceStationaryLevelForHawkesKernelError::InvalidStationaryLevel { source }
            })
    }

    fn stationary_feedback_loading_drift(
        self,
        stationary_buy_event_rate_per_hour: f64,
        stationary_sell_event_rate_per_hour: f64,
    ) -> f64 {
        self.buy_event_loading * stationary_buy_event_rate_per_hour
            + self.sell_event_loading * stationary_sell_event_rate_per_hour
            + self.return_energy_loading * self.stationary_return_energy_rate_per_hour
    }
}

/// Boundary state for the physical long-variance factor.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LongVarianceState {
    pub level: f64,
}

/// Convert a realized log-return increment into return-energy feedback input.
pub fn return_energy_increment_from_log_return(
    log_return: f64,
) -> Result<f64, LongVarianceStateTransitionError> {
    let return_energy_increment = log_return * log_return;

    validate_non_negative_number(return_energy_increment, "return_energy_increment").map_err(
        |source| LongVarianceStateTransitionError::InvalidReturnEnergyIncrement { source },
    )?;

    Ok(return_energy_increment)
}

/// Compute the stationary generated return-energy rate for the fitted return generator.
pub fn stationary_return_energy_rate_per_hour_from_return_generator(
    kernel_parameters: HawkesKernelParameters,
    exogenous_source_rates: HawkesStationaryTargetSourceRates,
    return_clock_exposure_parameters: ReturnClockExposureParameters,
    driver_parameters: NigBuySellDriverParameters,
) -> Option<f64> {
    let stationary_event_rates = stationary_intensity_rates(
        kernel_parameters.alpha,
        kernel_parameters.beta,
        kernel_parameters.lambda_bar,
        exogenous_source_rates,
    )?;

    let buy_endogenous_activity =
        stationary_event_rates.buy_event_rate_per_hour - kernel_parameters.lambda_bar;
    let sell_endogenous_activity =
        stationary_event_rates.sell_event_rate_per_hour - kernel_parameters.lambda_bar;

    let buy_activity = IntegratedSideActivityDecomposition::try_new(
        kernel_parameters.lambda_bar,
        buy_endogenous_activity,
    )
    .ok()?;

    let sell_activity = IntegratedSideActivityDecomposition::try_new(
        kernel_parameters.lambda_bar,
        sell_endogenous_activity,
    )
    .ok()?;

    let return_clocks = compute_effective_return_clocks(
        return_clock_exposure_parameters,
        buy_activity,
        sell_activity,
    )
    .ok()?;

    let buy_variance = driver_parameters
        .buy_side_parameters
        .variance_per_unit_time()?;
    let sell_variance = driver_parameters
        .sell_side_parameters
        .variance_per_unit_time()?;

    let return_energy_rate = return_clocks.buy_return_clock.mul_add(
        buy_variance,
        return_clocks.sell_return_clock * sell_variance,
    );

    (return_energy_rate.is_finite() && return_energy_rate >= 0.0).then_some(return_energy_rate)
}

impl LongVarianceState {
    /// Return the unit stationary state.
    pub const fn unit() -> Self {
        Self { level: 1.0 }
    }

    /// Construct a state without validation.
    pub const fn new(level: f64) -> Self {
        Self { level }
    }

    /// Construct a validated state.
    pub fn try_new(level: f64) -> Result<Self, LongVarianceStateError> {
        let state = Self::new(level);
        state.validate()?;

        Ok(state)
    }

    /// Validate state positivity.
    pub fn validate(self) -> Result<(), LongVarianceStateError> {
        validate_positive_number(self.level, "level")
            .map_err(|source| LongVarianceStateError::InvalidLevel { source })
    }

    /// Advance the conditional mean state across elapsed clock time.
    pub fn apply_mean_reversion_drift(
        self,
        parameters: LongVarianceParameters,
        stationary_mean_reversion_level: f64,
        delta_hours: f64,
    ) -> Result<Self, LongVarianceStateTransitionError> {
        self.validate()
            .map_err(|source| LongVarianceStateTransitionError::InvalidInitialState { source })?;

        parameters.validate().map_err(|source| {
            LongVarianceStateTransitionError::InvalidLongVarianceParameters { source }
        })?;

        validate_positive_number(
            stationary_mean_reversion_level,
            "stationary_mean_reversion_level",
        )
        .map_err(|source| {
            LongVarianceStateTransitionError::InvalidStationaryMeanReversionLevel { source }
        })?;

        validate_finite_number(delta_hours, "delta_hours")
            .map_err(|source| LongVarianceStateTransitionError::InvalidElapsedHours { source })?;

        if delta_hours < 0.0 {
            return Err(LongVarianceStateTransitionError::NegativeElapsedHours {
                value: delta_hours,
            });
        }

        let decay = (-parameters.mean_reversion_rate * delta_hours).exp();

        let next_level = stationary_mean_reversion_level
            + (self.level - stationary_mean_reversion_level) * decay;

        Self::try_new(next_level)
            .map_err(|source| LongVarianceStateTransitionError::InvalidTransitionState { source })
    }

    /// Return the average conditional-mean long-variance level over elapsed open-market time.
    pub fn baseline_clock_scale(
        self,
        parameters: LongVarianceParameters,
        stationary_mean_reversion_level: f64,
        delta_hours: f64,
    ) -> Result<f64, LongVarianceStateTransitionError> {
        self.validate()
            .map_err(|source| LongVarianceStateTransitionError::InvalidInitialState { source })?;

        parameters.validate().map_err(|source| {
            LongVarianceStateTransitionError::InvalidLongVarianceParameters { source }
        })?;

        validate_positive_number(
            stationary_mean_reversion_level,
            "stationary_mean_reversion_level",
        )
        .map_err(|source| {
            LongVarianceStateTransitionError::InvalidStationaryMeanReversionLevel { source }
        })?;

        validate_finite_number(delta_hours, "delta_hours")
            .map_err(|source| LongVarianceStateTransitionError::InvalidElapsedHours { source })?;

        if delta_hours < 0.0 {
            return Err(LongVarianceStateTransitionError::NegativeElapsedHours {
                value: delta_hours,
            });
        }

        if delta_hours == 0.0 {
            return Ok(self.level);
        }

        let decay = (-parameters.mean_reversion_rate * delta_hours).exp();

        let integrated_conditional_mean = stationary_mean_reversion_level * delta_hours
            + (self.level - stationary_mean_reversion_level) * (1.0 - decay)
                / parameters.mean_reversion_rate;

        let clock_scale = integrated_conditional_mean / delta_hours;

        validate_positive_number(clock_scale, "baseline_clock_scale").map_err(|source| {
            LongVarianceStateTransitionError::InvalidBaselineClockScale { source }
        })?;

        Ok(clock_scale)
    }

    /// Apply the uncompensated long-variance jump from one observed event.
    pub fn apply_event_jump(
        self,
        parameters: LongVarianceParameters,
        event_side: HawkesEventSide,
    ) -> Result<Self, LongVarianceStateTransitionError> {
        self.validate()
            .map_err(|source| LongVarianceStateTransitionError::InvalidInitialState { source })?;

        parameters.validate().map_err(|source| {
            LongVarianceStateTransitionError::InvalidLongVarianceParameters { source }
        })?;

        let loading = match event_side {
            HawkesEventSide::Buy => parameters.buy_event_loading,
            HawkesEventSide::Sell => parameters.sell_event_loading,
        };

        Self::try_new(self.level + loading)
            .map_err(|source| LongVarianceStateTransitionError::InvalidTransitionState { source })
    }

    /// Apply the uncompensated long-variance jump from realized return-trading-time energy.
    pub fn apply_return_energy_jump(
        self,
        parameters: LongVarianceParameters,
        return_energy_increment: f64,
    ) -> Result<Self, LongVarianceStateTransitionError> {
        self.validate()
            .map_err(|source| LongVarianceStateTransitionError::InvalidInitialState { source })?;

        parameters.validate().map_err(|source| {
            LongVarianceStateTransitionError::InvalidLongVarianceParameters { source }
        })?;

        validate_non_negative_number(return_energy_increment, "return_energy_increment").map_err(
            |source| LongVarianceStateTransitionError::InvalidReturnEnergyIncrement { source },
        )?;

        let transition_level =
            self.level + parameters.return_energy_loading * return_energy_increment;

        Self::try_new(transition_level)
            .map_err(|source| LongVarianceStateTransitionError::InvalidTransitionState { source })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LongVarianceParametersError {
    InvalidPositiveParameter {
        field: &'static str,
        source: janiform_foundations::numeric::PositiveNumberValidationError,
    },
    InvalidNonNegativeParameter {
        field: &'static str,
        source: janiform_foundations::numeric::NonNegativeNumberValidationError,
    },
}

impl Display for LongVarianceParametersError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPositiveParameter { field, source } => {
                write!(formatter, "{field} is invalid: {source}")
            }
            Self::InvalidNonNegativeParameter { field, source } => {
                write!(formatter, "{field} is invalid: {source}")
            }
        }
    }
}

impl Error for LongVarianceParametersError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPositiveParameter { source, .. } => Some(source),
            Self::InvalidNonNegativeParameter { source, .. } => Some(source),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LongVarianceStateError {
    InvalidLevel {
        source: janiform_foundations::numeric::PositiveNumberValidationError,
    },
}

impl Display for LongVarianceStateError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLevel { source } => write!(formatter, "level is invalid: {source}"),
        }
    }
}

impl Error for LongVarianceStateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidLevel { source } => Some(source),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LongVarianceStateTransitionError {
    InvalidInitialState {
        source: LongVarianceStateError,
    },
    InvalidLongVarianceParameters {
        source: LongVarianceParametersError,
    },
    InvalidStationaryMeanReversionLevel {
        source: janiform_foundations::numeric::PositiveNumberValidationError,
    },
    InvalidElapsedHours {
        source: janiform_foundations::numeric::FiniteNumberValidationError,
    },
    NegativeElapsedHours {
        value: f64,
    },
    InvalidReturnEnergyIncrement {
        source: janiform_foundations::numeric::NonNegativeNumberValidationError,
    },
    InvalidBaselineClockScale {
        source: janiform_foundations::numeric::PositiveNumberValidationError,
    },
    InvalidTransitionState {
        source: LongVarianceStateError,
    },
}

impl Display for LongVarianceStateTransitionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInitialState { source } => {
                write!(
                    formatter,
                    "initial long-variance state is invalid: {source}"
                )
            }
            Self::InvalidLongVarianceParameters { source } => {
                write!(formatter, "long-variance parameters are invalid: {source}")
            }
            Self::InvalidStationaryMeanReversionLevel { source } => write!(
                formatter,
                "stationary_mean_reversion_level is invalid: {source}"
            ),
            Self::InvalidElapsedHours { source } => {
                write!(formatter, "delta_hours is invalid: {source}")
            }
            Self::NegativeElapsedHours { value } => {
                write!(formatter, "delta_hours must be non-negative, found {value}")
            }
            Self::InvalidReturnEnergyIncrement { source } => {
                write!(formatter, "return_energy_increment is invalid: {source}")
            }
            Self::InvalidBaselineClockScale { source } => {
                write!(formatter, "baseline_clock_scale is invalid: {source}")
            }
            Self::InvalidTransitionState { source } => {
                write!(
                    formatter,
                    "transitioned long-variance state is invalid: {source}"
                )
            }
        }
    }
}

impl Error for LongVarianceStateTransitionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidInitialState { source } => Some(source),
            Self::InvalidLongVarianceParameters { source } => Some(source),
            Self::InvalidStationaryMeanReversionLevel { source } => Some(source),
            Self::InvalidElapsedHours { source } => Some(source),
            Self::InvalidReturnEnergyIncrement { source } => Some(source),
            Self::InvalidBaselineClockScale { source } => Some(source),
            Self::InvalidTransitionState { source } => Some(source),
            Self::NegativeElapsedHours { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LongVarianceStationaryLevelError {
    InvalidLongVarianceParameters {
        source: LongVarianceParametersError,
    },
    InvalidBaselineIntensity {
        source: janiform_foundations::numeric::PositiveNumberValidationError,
    },
    InvalidStationaryEventRate {
        field: &'static str,
        source: janiform_foundations::numeric::NonNegativeNumberValidationError,
    },
    InvalidDerivedMeanReversionLevel {
        source: janiform_foundations::numeric::PositiveNumberValidationError,
    },
}

impl Display for LongVarianceStationaryLevelError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLongVarianceParameters { source } => {
                write!(formatter, "long-variance parameters are invalid: {source}")
            }
            Self::InvalidBaselineIntensity { source } => {
                write!(formatter, "baseline_intensity is invalid: {source}")
            }
            Self::InvalidStationaryEventRate { field, source } => {
                write!(formatter, "{field} is invalid: {source}")
            }
            Self::InvalidDerivedMeanReversionLevel { source } => write!(
                formatter,
                "derived stationary mean-reversion level is invalid: {source}"
            ),
        }
    }
}

impl Error for LongVarianceStationaryLevelError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidLongVarianceParameters { source } => Some(source),
            Self::InvalidBaselineIntensity { source } => Some(source),
            Self::InvalidStationaryEventRate { source, .. } => Some(source),
            Self::InvalidDerivedMeanReversionLevel { source } => Some(source),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LongVarianceStationaryLevelForHawkesKernelError {
    InvalidStationaryIntensityMultipliers,
    InvalidStationaryLevel {
        source: LongVarianceStationaryLevelError,
    },
}

impl Display for LongVarianceStationaryLevelForHawkesKernelError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidStationaryIntensityMultipliers => write!(
                formatter,
                "stationary Hawkes intensity multipliers could not be derived"
            ),
            Self::InvalidStationaryLevel { source } => write!(
                formatter,
                "long-variance stationary mean-reversion level is invalid: {source}"
            ),
        }
    }
}

impl Error for LongVarianceStationaryLevelForHawkesKernelError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidStationaryLevel { source } => Some(source),
            Self::InvalidStationaryIntensityMultipliers => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::hawkes::{
        HawkesEventSide, HawkesKernelLoadings, HawkesKernelParameters, HawkesSourceComponents,
        HawkesStationaryIntensityMultipliers, HawkesStationaryTargetSourceRates,
    };
    use crate::nig::NigSharedPlusShiftConfiguration;
    use crate::return_clock::ReturnClockExposureParameters;

    use super::{
        LongVarianceParameters, LongVarianceState, LongVarianceStateTransitionError,
        LongVarianceStationaryLevelError, return_energy_increment_from_log_return,
        stationary_return_energy_rate_per_hour_from_return_generator,
    };

    #[test]
    fn stationary_mean_reversion_level_offsets_event_feedback_drift() {
        let parameters = LongVarianceParameters::try_new(4.0, 0.5, 0.0, 0.0, 0.10, 0.20)
            .expect("parameters should build");

        let level = parameters
            .stationary_mean_reversion_level(
                2.0,
                HawkesStationaryIntensityMultipliers {
                    buy_intensity_multiplier: 0.5,
                    sell_intensity_multiplier: 0.75,
                },
            )
            .expect("stationary level should build");

        assert_eq!(level, 1.0 - (0.10 * 1.0 + 0.20 * 1.5) / 4.0);
    }

    #[test]
    fn stationary_mean_reversion_level_offsets_return_energy_feedback_drift() {
        let parameters = LongVarianceParameters::try_new(4.0, 0.5, 0.03, 2.0, 0.10, 0.20)
            .expect("parameters should build");

        let level = parameters
            .stationary_mean_reversion_level(
                2.0,
                HawkesStationaryIntensityMultipliers {
                    buy_intensity_multiplier: 0.5,
                    sell_intensity_multiplier: 0.75,
                },
            )
            .expect("stationary level should build");

        assert_eq!(level, 1.0 - (0.10 * 1.0 + 0.20 * 1.5 + 2.0 * 0.03) / 4.0);
    }

    #[test]
    fn return_energy_increment_squares_valid_log_return() {
        let increment =
            return_energy_increment_from_log_return(0.25).expect("increment should validate");

        assert_eq!(increment, 0.0625);
    }

    #[test]
    fn return_energy_increment_rejects_invalid_log_return() {
        let error = return_energy_increment_from_log_return(f64::NAN).expect_err("NaN is invalid");

        assert!(matches!(
            error,
            LongVarianceStateTransitionError::InvalidReturnEnergyIncrement { .. }
        ));
    }

    #[test]
    fn stationary_mean_reversion_level_includes_exogenous_hawkes_source_rates() {
        let parameters = LongVarianceParameters::try_new(10.0, 0.0, 0.0, 0.0, 0.10, 0.20)
            .expect("parameters should build");

        let zero_loadings = HawkesKernelLoadings::from_target_components(
            HawkesSourceComponents::new(0.0, 0.0),
            HawkesSourceComponents::new(0.0, 0.0),
        );

        let kernel_parameters =
            HawkesKernelParameters::try_new(3.0, 2.0, zero_loadings, zero_loadings)
                .expect("zero-loadings kernel should build");

        let level = parameters
            .stationary_mean_reversion_level_for_hawkes_kernel(
                kernel_parameters,
                HawkesStationaryTargetSourceRates {
                    target_plus_source_rate: 0.4,
                    target_minus_source_rate: 0.2,
                },
            )
            .expect("stationary level should build");

        let expected_buy_rate = 3.0 + 0.4 / 2.0;
        let expected_sell_rate = 3.0 + 0.2 / 2.0;

        assert_eq!(
            level,
            1.0 - (0.10 * expected_buy_rate + 0.20 * expected_sell_rate) / 10.0
        );
    }

    #[test]
    fn generated_stationary_return_energy_rate_uses_side_nig_variance_and_return_clocks() {
        let zero_loadings = HawkesKernelLoadings::from_target_components(
            HawkesSourceComponents::new(0.0, 0.0),
            HawkesSourceComponents::new(0.0, 0.0),
        );

        let kernel_parameters =
            HawkesKernelParameters::try_new(2.0, 1.0, zero_loadings, zero_loadings)
                .expect("zero-loadings kernel should build");

        let driver_parameters = NigSharedPlusShiftConfiguration::try_new(5.0, 0.0, 0.2, 0.0, -1.0)
            .expect("NIG fixture should build")
            .derive_driver_parameters()
            .expect("driver fixture should build");

        let energy_rate = stationary_return_energy_rate_per_hour_from_return_generator(
            kernel_parameters,
            HawkesStationaryTargetSourceRates::zero(),
            ReturnClockExposureParameters::neutral(),
            driver_parameters,
        )
        .expect("stationary energy rate should build");

        let expected_rate = 2.0
            * driver_parameters
                .buy_side_parameters
                .variance_per_unit_time()
                .expect("buy variance should build")
            + 2.0
                * driver_parameters
                    .sell_side_parameters
                    .variance_per_unit_time()
                    .expect("sell variance should build");

        assert_eq!(energy_rate, expected_rate);
    }

    #[test]
    fn stationary_mean_reversion_level_rejects_exhausted_event_feedback_budget() {
        let parameters = LongVarianceParameters::try_new(4.0, 0.0, 0.0, 0.0, 0.5, 0.5)
            .expect("parameters should build");

        let error = parameters
            .stationary_mean_reversion_level(
                2.0,
                HawkesStationaryIntensityMultipliers {
                    buy_intensity_multiplier: 2.0,
                    sell_intensity_multiplier: 2.0,
                },
            )
            .expect_err("event feedback drift at the mean-reversion rate must be rejected");

        assert!(matches!(
            error,
            LongVarianceStationaryLevelError::InvalidDerivedMeanReversionLevel { .. }
        ));
    }

    #[test]
    fn long_variance_state_replay_applies_drift_and_observed_event_jumps() {
        let parameters = LongVarianceParameters::try_new(2.0, 0.0, 0.0, 0.0, 0.25, 0.50)
            .expect("parameters should build");

        let initial_state = LongVarianceState::unit();

        let after_buy = initial_state
            .apply_event_jump(parameters, HawkesEventSide::Buy)
            .expect("buy jump should apply");

        let after_drift = after_buy
            .apply_mean_reversion_drift(parameters, 0.75, 1.0)
            .expect("drift should apply");

        let after_sell = after_drift
            .apply_event_jump(parameters, HawkesEventSide::Sell)
            .expect("sell jump should apply");

        let expected_after_drift = 0.75 + (1.25 - 0.75) * (-2.0_f64).exp();

        assert_eq!(after_drift.level, expected_after_drift);
        assert_eq!(after_sell.level, expected_after_drift + 0.50);
    }

    #[test]
    fn long_variance_state_replay_applies_observed_return_energy_jumps() {
        let parameters = LongVarianceParameters::try_new(2.0, 0.0, 0.01, 4.0, 0.25, 0.50)
            .expect("parameters should build");

        let state = LongVarianceState::unit()
            .apply_return_energy_jump(parameters, 0.125)
            .expect("return-energy jump should apply");

        assert_eq!(state.level, 1.0 + 4.0 * 0.125);
    }
}
