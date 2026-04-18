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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PricingProgressEvent {
    pub phase: PricingProgressPhase,
    pub completed_units: usize,
    pub total_units: usize,
    pub current_group_index: Option<usize>,
    pub current_maturity_index: Option<usize>,
    pub total_maturities: Option<usize>,
    pub current_interval_index: Option<usize>,
    pub total_intervals: Option<usize>,
    pub current_transition_row_index: Option<usize>,
    pub total_transition_rows: Option<usize>,
    pub current_strike_index: Option<usize>,
    pub total_strikes: Option<usize>,
    pub seed_exact_transition_row_count: Option<usize>,
    pub shared_theta_transition_row_count: Option<usize>,
    pub certified_transition_row_count: Option<usize>,
    pub fallback_exact_transition_row_count: Option<usize>,
    pub theta_search_elapsed_millis: Option<u64>,
    pub theta_certification_elapsed_millis: Option<u64>,
    pub row_batch_elapsed_millis: Option<u64>,
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
            current_transition_row_index: None,
            total_transition_rows: None,
            current_strike_index: None,
            total_strikes: None,
            seed_exact_transition_row_count: None,
            shared_theta_transition_row_count: None,
            certified_transition_row_count: None,
            fallback_exact_transition_row_count: None,
            theta_search_elapsed_millis: None,
            theta_certification_elapsed_millis: None,
            row_batch_elapsed_millis: None,
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
            current_transition_row_index: None,
            total_transition_rows: None,
            current_strike_index: None,
            total_strikes: None,
            seed_exact_transition_row_count: None,
            shared_theta_transition_row_count: None,
            certified_transition_row_count: None,
            fallback_exact_transition_row_count: None,
            theta_search_elapsed_millis: None,
            theta_certification_elapsed_millis: None,
            row_batch_elapsed_millis: None,
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
            current_transition_row_index: None,
            total_transition_rows: None,
            current_strike_index: None,
            total_strikes: None,
            seed_exact_transition_row_count: None,
            shared_theta_transition_row_count: None,
            certified_transition_row_count: None,
            fallback_exact_transition_row_count: None,
            theta_search_elapsed_millis: None,
            theta_certification_elapsed_millis: None,
            row_batch_elapsed_millis: None,
        }
    }

    pub fn transition_row_progress(
        completed_units: usize,
        total_units: usize,
        current_interval_index: Option<usize>,
        total_intervals: Option<usize>,
        current_transition_row_index: Option<usize>,
        total_transition_rows: usize,
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
            current_transition_row_index,
            total_transition_rows: Some(total_transition_rows),
            current_strike_index: None,
            total_strikes: None,
            seed_exact_transition_row_count: None,
            shared_theta_transition_row_count: None,
            certified_transition_row_count: None,
            fallback_exact_transition_row_count: None,
            theta_search_elapsed_millis: None,
            theta_certification_elapsed_millis: None,
            row_batch_elapsed_millis: None,
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
            current_transition_row_index: None,
            total_transition_rows: None,
            current_strike_index,
            total_strikes: Some(total_strikes),
            seed_exact_transition_row_count: None,
            shared_theta_transition_row_count: None,
            certified_transition_row_count: None,
            fallback_exact_transition_row_count: None,
            theta_search_elapsed_millis: None,
            theta_certification_elapsed_millis: None,
            row_batch_elapsed_millis: None,
        }
    }

    pub fn with_transition_row_solve_diagnostics(
        mut self,
        seed_exact_transition_row_count: usize,
        shared_theta_transition_row_count: usize,
        certified_transition_row_count: usize,
        fallback_exact_transition_row_count: usize,
        theta_search_elapsed_millis: u64,
        theta_certification_elapsed_millis: u64,
        row_batch_elapsed_millis: u64,
    ) -> Self {
        self.seed_exact_transition_row_count = Some(seed_exact_transition_row_count);
        self.shared_theta_transition_row_count = Some(shared_theta_transition_row_count);
        self.certified_transition_row_count = Some(certified_transition_row_count);
        self.fallback_exact_transition_row_count = Some(fallback_exact_transition_row_count);
        self.theta_search_elapsed_millis = Some(theta_search_elapsed_millis);
        self.theta_certification_elapsed_millis = Some(theta_certification_elapsed_millis);
        self.row_batch_elapsed_millis = Some(row_batch_elapsed_millis);
        self
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
