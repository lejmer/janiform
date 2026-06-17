use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use janiform_foundations::numeric::validate_non_negative_number;

use crate::hawkes::HawkesEventSide;
use crate::return_clock::{
    IntegratedSideActivityDecomposition, IntegratedSideActivityDecompositionError,
};

/// Economic no-move mark probabilities split by event side and activity source.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoMoveEventModel {
    pub buy_baseline_no_move_probability: f64,
    pub buy_endogenous_no_move_probability: f64,
    pub sell_baseline_no_move_probability: f64,
    pub sell_endogenous_no_move_probability: f64,
}

impl NoMoveEventModel {
    /// Return the neutral model where every market event is a moving-price event.
    pub const fn neutral() -> Self {
        Self {
            buy_baseline_no_move_probability: 0.0,
            buy_endogenous_no_move_probability: 0.0,
            sell_baseline_no_move_probability: 0.0,
            sell_endogenous_no_move_probability: 0.0,
        }
    }

    /// Construct a no-move model without validation.
    pub const fn new(
        buy_baseline_no_move_probability: f64,
        buy_endogenous_no_move_probability: f64,
        sell_baseline_no_move_probability: f64,
        sell_endogenous_no_move_probability: f64,
    ) -> Self {
        Self {
            buy_baseline_no_move_probability,
            buy_endogenous_no_move_probability,
            sell_baseline_no_move_probability,
            sell_endogenous_no_move_probability,
        }
    }

    /// Construct a validated no-move model.
    pub fn try_new(
        buy_baseline_no_move_probability: f64,
        buy_endogenous_no_move_probability: f64,
        sell_baseline_no_move_probability: f64,
        sell_endogenous_no_move_probability: f64,
    ) -> Result<Self, NoMoveEventModelError> {
        let model = Self::new(
            buy_baseline_no_move_probability,
            buy_endogenous_no_move_probability,
            sell_baseline_no_move_probability,
            sell_endogenous_no_move_probability,
        );

        model.validate()?;

        Ok(model)
    }

    /// Validate that every no-move probability is in the closed unit interval.
    pub fn validate(self) -> Result<(), NoMoveEventModelError> {
        validate_probability(
            "buy_baseline_no_move_probability",
            self.buy_baseline_no_move_probability,
        )?;

        validate_probability(
            "buy_endogenous_no_move_probability",
            self.buy_endogenous_no_move_probability,
        )?;

        validate_probability(
            "sell_baseline_no_move_probability",
            self.sell_baseline_no_move_probability,
        )?;

        validate_probability(
            "sell_endogenous_no_move_probability",
            self.sell_endogenous_no_move_probability,
        )?;

        Ok(())
    }

    /// Return the moving-event probability for buy baseline activity.
    pub fn buy_baseline_move_probability(self) -> f64 {
        1.0 - self.buy_baseline_no_move_probability
    }

    /// Return the moving-event probability for buy endogenous activity.
    pub fn buy_endogenous_move_probability(self) -> f64 {
        1.0 - self.buy_endogenous_no_move_probability
    }

    /// Return the moving-event probability for sell baseline activity.
    pub fn sell_baseline_move_probability(self) -> f64 {
        1.0 - self.sell_baseline_no_move_probability
    }

    /// Return the moving-event probability for sell endogenous activity.
    pub fn sell_endogenous_move_probability(self) -> f64 {
        1.0 - self.sell_endogenous_no_move_probability
    }

    /// Thin buy-side integrated activity to the moving-clock contribution.
    pub fn moving_buy_activity(
        self,
        activity: IntegratedSideActivityDecomposition,
    ) -> Result<IntegratedSideActivityDecomposition, NoMoveEventModelError> {
        self.validate()?;

        activity
            .validate()
            .map_err(|source| NoMoveEventModelError::InvalidActivity { source })?;

        IntegratedSideActivityDecomposition::try_new(
            activity.baseline_activity * self.buy_baseline_move_probability(),
            activity.endogenous_activity * self.buy_endogenous_move_probability(),
        )
        .map_err(|source| NoMoveEventModelError::InvalidActivity { source })
    }

    /// Thin sell-side integrated activity to the moving-clock contribution.
    pub fn moving_sell_activity(
        self,
        activity: IntegratedSideActivityDecomposition,
    ) -> Result<IntegratedSideActivityDecomposition, NoMoveEventModelError> {
        self.validate()?;

        activity
            .validate()
            .map_err(|source| NoMoveEventModelError::InvalidActivity { source })?;

        IntegratedSideActivityDecomposition::try_new(
            activity.baseline_activity * self.sell_baseline_move_probability(),
            activity.endogenous_activity * self.sell_endogenous_move_probability(),
        )
        .map_err(|source| NoMoveEventModelError::InvalidActivity { source })
    }

    /// Return the activity-conditioned no-move probability for one accepted event.
    pub fn event_no_move_probability(
        self,
        side: HawkesEventSide,
        baseline_rate: f64,
        endogenous_rate: f64,
    ) -> Result<Option<f64>, NoMoveEventModelError> {
        self.validate()?;

        validate_non_negative_number(baseline_rate, "baseline_rate")
            .map_err(|source| NoMoveEventModelError::InvalidRate { source })?;

        validate_non_negative_number(endogenous_rate, "endogenous_rate")
            .map_err(|source| NoMoveEventModelError::InvalidRate { source })?;

        let total_rate = baseline_rate + endogenous_rate;

        validate_non_negative_number(total_rate, "total_rate")
            .map_err(|source| NoMoveEventModelError::InvalidRate { source })?;

        if total_rate == 0.0 {
            return Ok(None);
        }

        let numerator = match side {
            HawkesEventSide::Buy => {
                self.buy_baseline_no_move_probability * baseline_rate
                    + self.buy_endogenous_no_move_probability * endogenous_rate
            }
            HawkesEventSide::Sell => {
                self.sell_baseline_no_move_probability * baseline_rate
                    + self.sell_endogenous_no_move_probability * endogenous_rate
            }
        };

        Ok(Some(numerator / total_rate))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum NoMoveEventModelError {
    InvalidProbability {
        field: &'static str,
        value: f64,
    },
    InvalidRate {
        source: janiform_foundations::numeric::NonNegativeNumberValidationError,
    },
    InvalidActivity {
        source: IntegratedSideActivityDecompositionError,
    },
}

impl Display for NoMoveEventModelError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidProbability { field, value } => write!(
                formatter,
                "{field} must be finite and between 0.0 and 1.0 inclusive, found {value}"
            ),
            Self::InvalidRate { source } => write!(formatter, "{source}"),
            Self::InvalidActivity { source } => write!(formatter, "{source}"),
        }
    }
}

impl Error for NoMoveEventModelError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRate { source } => Some(source),
            Self::InvalidActivity { source } => Some(source),
            Self::InvalidProbability { .. } => None,
        }
    }
}

fn validate_probability(field: &'static str, value: f64) -> Result<(), NoMoveEventModelError> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(NoMoveEventModelError::InvalidProbability { field, value });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::NoMoveEventModel;
    use crate::hawkes::HawkesEventSide;
    use crate::return_clock::IntegratedSideActivityDecomposition;

    #[test]
    fn neutral_model_preserves_moving_activity() {
        let model = NoMoveEventModel::neutral();

        let buy_activity = IntegratedSideActivityDecomposition::try_new(2.0, 3.0)
            .expect("activity should construct");

        let sell_activity = IntegratedSideActivityDecomposition::try_new(5.0, 7.0)
            .expect("activity should construct");

        assert_eq!(
            model
                .moving_buy_activity(buy_activity)
                .expect("moving buy activity should construct"),
            buy_activity
        );
        assert_eq!(
            model
                .moving_sell_activity(sell_activity)
                .expect("moving sell activity should construct"),
            sell_activity
        );
    }

    #[test]
    fn moving_activity_scales_side_and_source_components() {
        let model =
            NoMoveEventModel::try_new(0.25, 0.5, 0.75, 1.0).expect("model should construct");

        assert_eq!(
            model
                .moving_buy_activity(IntegratedSideActivityDecomposition::new(4.0, 6.0))
                .expect("moving buy activity should construct"),
            IntegratedSideActivityDecomposition::new(3.0, 3.0)
        );
        assert_eq!(
            model
                .moving_sell_activity(IntegratedSideActivityDecomposition::new(8.0, 10.0))
                .expect("moving sell activity should construct"),
            IntegratedSideActivityDecomposition::new(2.0, 0.0)
        );
    }

    #[test]
    fn event_no_move_probability_uses_current_activity_mix() {
        let model = NoMoveEventModel::try_new(0.2, 0.8, 0.1, 0.7).expect("model should construct");

        let probability = model
            .event_no_move_probability(HawkesEventSide::Buy, 3.0, 1.0)
            .expect("probability should construct")
            .expect("positive rate should return a probability");

        let expected_probability = ((0.2 * 3.0) + (0.8 * 1.0)) / (3.0 + 1.0);

        assert_eq!(probability, expected_probability);
        assert_eq!(
            model
                .event_no_move_probability(HawkesEventSide::Sell, 0.0, 0.0)
                .expect("zero rate should be valid"),
            None
        );
    }
}
