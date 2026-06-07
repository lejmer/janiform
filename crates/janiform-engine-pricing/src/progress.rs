use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PricingProgressPhase {
    GroupProgress,
    MaturityProgress,
    IntervalProgress,
    TransitionRowProgress,
    StrikePricingProgress,
}

/// Render a pricing progress phase for user-facing status output.
pub const fn pricing_progress_phase_label(phase: PricingProgressPhase) -> &'static str {
    match phase {
        PricingProgressPhase::GroupProgress => "Pricing groups",
        PricingProgressPhase::MaturityProgress => "Pricing maturities",
        PricingProgressPhase::IntervalProgress => "Pricing intervals",
        PricingProgressPhase::TransitionRowProgress => "Transition rows",
        PricingProgressPhase::StrikePricingProgress => "Pricing strikes",
    }
}

#[cfg(test)]
mod tests {
    use super::{PricingProgressPhase, pricing_progress_phase_label};

    #[test]
    fn pricing_progress_phase_labels_are_sentence_case() {
        assert_eq!(
            [
                pricing_progress_phase_label(PricingProgressPhase::GroupProgress),
                pricing_progress_phase_label(PricingProgressPhase::MaturityProgress),
                pricing_progress_phase_label(PricingProgressPhase::IntervalProgress),
                pricing_progress_phase_label(PricingProgressPhase::TransitionRowProgress),
                pricing_progress_phase_label(PricingProgressPhase::StrikePricingProgress),
            ],
            [
                "Pricing groups",
                "Pricing maturities",
                "Pricing intervals",
                "Transition rows",
                "Pricing strikes",
            ],
        );
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PricingProgressEvent {
    pub phase: PricingProgressPhase,
    pub completed_units: usize,
    pub total_units: usize,
    pub current_group_index: Option<usize>,
    pub current_maturity_index: Option<usize>,
    pub total_maturities: Option<usize>,
    pub current_interval_index: Option<usize>,
    pub total_intervals: Option<usize>,
    pub current_strike_index: Option<usize>,
    pub total_strikes: Option<usize>,
}

impl PricingProgressEvent {
    pub fn group_progress(
        completed_units: usize,
        total_units: usize,
        current_group_index: Option<usize>,
    ) -> Self {
        Self {
            phase: PricingProgressPhase::GroupProgress,
            completed_units,
            total_units,
            current_group_index,
            current_maturity_index: None,
            total_maturities: None,
            current_interval_index: None,
            total_intervals: None,
            current_strike_index: None,
            total_strikes: None,
        }
    }

    pub fn maturity_progress(
        completed_units: usize,
        total_units: usize,
        current_maturity_index: Option<usize>,
        total_maturities: usize,
    ) -> Self {
        Self {
            phase: PricingProgressPhase::MaturityProgress,
            completed_units,
            total_units,
            current_group_index: None,
            current_maturity_index,
            total_maturities: Some(total_maturities),
            current_interval_index: None,
            total_intervals: None,
            current_strike_index: None,
            total_strikes: None,
        }
    }

    pub fn interval_progress(
        completed_units: usize,
        total_units: usize,
        current_interval_index: Option<usize>,
        total_intervals: usize,
    ) -> Self {
        Self {
            phase: PricingProgressPhase::IntervalProgress,
            completed_units,
            total_units,
            current_group_index: None,
            current_maturity_index: None,
            total_maturities: None,
            current_interval_index,
            total_intervals: Some(total_intervals),
            current_strike_index: None,
            total_strikes: None,
        }
    }

    pub fn transition_row_progress(
        completed_units: usize,
        total_units: usize,
        current_interval_index: Option<usize>,
        total_intervals: Option<usize>,
    ) -> Self {
        Self {
            phase: PricingProgressPhase::TransitionRowProgress,
            completed_units,
            total_units,
            current_group_index: None,
            current_maturity_index: None,
            total_maturities: None,
            current_interval_index,
            total_intervals,
            current_strike_index: None,
            total_strikes: None,
        }
    }

    pub fn strike_pricing_progress(
        completed_units: usize,
        total_units: usize,
        current_maturity_index: Option<usize>,
        total_maturities: Option<usize>,
        current_strike_index: Option<usize>,
        total_strikes: usize,
    ) -> Self {
        Self {
            phase: PricingProgressPhase::StrikePricingProgress,
            completed_units,
            total_units,
            current_group_index: None,
            current_maturity_index,
            total_maturities,
            current_interval_index: None,
            total_intervals: None,
            current_strike_index,
            total_strikes: Some(total_strikes),
        }
    }
}
