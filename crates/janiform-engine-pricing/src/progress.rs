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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PricingNodeProgressPhase {
    MaterializingRiskNeutralAnchors,
    EvaluatingAnchoredRecursion,
    EvaluatingFourierInversion,
    EvaluatingRefinedAnchoredRecursion,
    EvaluatingRefinedFourierInversion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PricingNodeProgress {
    pub phase: PricingNodeProgressPhase,
    pub current_interval_index: Option<usize>,
    pub total_intervals: Option<usize>,
    pub current_fourier_step_index: Option<usize>,
    pub total_fourier_steps: Option<usize>,
}

impl PricingNodeProgress {
    pub const fn new(
        phase: PricingNodeProgressPhase,
        current_interval_index: Option<usize>,
        total_intervals: Option<usize>,
        current_fourier_step_index: Option<usize>,
        total_fourier_steps: Option<usize>,
    ) -> Self {
        Self {
            phase,
            current_interval_index,
            total_intervals,
            current_fourier_step_index,
            total_fourier_steps,
        }
    }

    pub const fn for_interval(
        phase: PricingNodeProgressPhase,
        current_interval_index: usize,
        total_intervals: usize,
    ) -> Self {
        Self::new(
            phase,
            Some(current_interval_index),
            Some(total_intervals),
            None,
            None,
        )
    }

    pub const fn for_fourier_step(
        phase: PricingNodeProgressPhase,
        current_fourier_step_index: usize,
        total_fourier_steps: usize,
    ) -> Self {
        Self::new(
            phase,
            None,
            None,
            Some(current_fourier_step_index),
            Some(total_fourier_steps),
        )
    }

    pub const fn phase_only(phase: PricingNodeProgressPhase) -> Self {
        Self::new(phase, None, None, None, None)
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
