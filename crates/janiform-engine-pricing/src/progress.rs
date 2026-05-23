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
    pub(crate) fn from_risk_neutral_anchor_materialization_progress(
        event: PricingNodeProgress,
    ) -> Option<Self> {
        if event.phase != PricingNodeProgressPhase::MaterializingRiskNeutralAnchors {
            return None;
        }

        let total_intervals = event.total_intervals?;
        let completed_interval_count = event
            .current_interval_index
            .unwrap_or(total_intervals)
            .min(total_intervals);

        Some(Self::interval_progress(
            completed_interval_count,
            total_intervals,
            Some(completed_interval_count),
            total_intervals,
        ))
    }

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

pub(crate) fn emit_node_progress(
    observer: &mut Option<&mut dyn FnMut(PricingNodeProgress)>,
    event: PricingNodeProgress,
) {
    if let Some(observer) = observer.as_deref_mut() {
        observer(event);
    }
}

#[cfg(test)]
mod tests {
    use super::{PricingNodeProgress, PricingNodeProgressPhase, PricingProgressEvent};

    #[test]
    fn anchor_materialization_progress_forwards_interval_counts() {
        let progress_events = [
            PricingNodeProgress::for_interval(
                PricingNodeProgressPhase::MaterializingRiskNeutralAnchors,
                0,
                47,
            ),
            PricingNodeProgress::for_interval(
                PricingNodeProgressPhase::MaterializingRiskNeutralAnchors,
                47,
                47,
            ),
        ]
        .into_iter()
        .map(PricingProgressEvent::from_risk_neutral_anchor_materialization_progress)
        .collect::<Vec<_>>();

        assert_eq!(
            progress_events,
            vec![
                Some(PricingProgressEvent::interval_progress(0, 47, Some(0), 47)),
                Some(PricingProgressEvent::interval_progress(
                    47,
                    47,
                    Some(47),
                    47
                )),
            ],
        );
    }

    #[test]
    fn non_materialization_node_progress_does_not_map_to_interval_progress() {
        assert_eq!(
            PricingProgressEvent::from_risk_neutral_anchor_materialization_progress(
                PricingNodeProgress::phase_only(
                    PricingNodeProgressPhase::EvaluatingFourierInversion
                ),
            ),
            None,
        );
    }
}
