use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::sleep;
use std::time::Duration;

use super::progress::{PricingProgressEvent, PricingProgressPhase};

const PRICING_PROGRESS_NONE: usize = usize::MAX;
const PRICING_PROGRESS_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone)]
pub(crate) struct PricingProgressReporter {
    state: Option<Arc<PricingProgressState>>,
}

impl PricingProgressReporter {
    pub(crate) fn disabled() -> Self {
        Self { state: None }
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.state.is_some()
    }

    pub(crate) fn shared() -> Self {
        Self {
            state: Some(Arc::new(PricingProgressState::new())),
        }
    }

    pub(crate) fn emit(&self, event: PricingProgressEvent) {
        let Some(state) = &self.state else {
            return;
        };

        state.store_event(&event);
    }

    pub(crate) fn start_transition_rows(
        &self,
        interval_index: usize,
        total_intervals: usize,
        total_rows: usize,
    ) {
        self.emit(PricingProgressEvent::transition_row_progress(
            0,
            total_rows,
            Some(interval_index),
            Some(total_intervals),
            None,
            total_rows,
        ));
    }

    pub(crate) fn advance_transition_rows(&self, delta: usize) {
        let Some(state) = &self.state else {
            return;
        };

        if state.phase() != Some(PricingProgressPhase::TransitionRowProgress) {
            return;
        }

        let completed_units = state.completed_units.fetch_add(delta, Ordering::SeqCst) + delta;
        let total_units = state.total_units.load(Ordering::SeqCst);
        state.current_transition_row_index.store(
            if completed_units < total_units {
                completed_units.saturating_sub(1)
            } else {
                PRICING_PROGRESS_NONE
            },
            Ordering::SeqCst,
        );
    }

    pub(crate) fn finish_transition_rows(&self) {
        let Some(state) = &self.state else {
            return;
        };

        if state.phase() != Some(PricingProgressPhase::TransitionRowProgress) {
            return;
        }

        let total_units = state.total_units.load(Ordering::SeqCst);

        state.completed_units.store(total_units, Ordering::SeqCst);
        state
            .current_transition_row_index
            .store(PRICING_PROGRESS_NONE, Ordering::SeqCst);
    }

    pub(crate) fn start_strike_pricing(
        &self,
        maturity_index: usize,
        total_maturities: usize,
        total_strikes: usize,
    ) {
        self.emit(PricingProgressEvent::strike_pricing_progress(
            0,
            total_strikes,
            Some(maturity_index),
            Some(total_maturities),
            None,
            total_strikes,
        ));
    }

    pub(crate) fn advance_strike_pricing(&self, delta: usize) {
        let Some(state) = &self.state else {
            return;
        };

        if state.phase() != Some(PricingProgressPhase::StrikePricingProgress) {
            return;
        }

        let completed_units = state.completed_units.fetch_add(delta, Ordering::SeqCst) + delta;
        let total_units = state.total_units.load(Ordering::SeqCst);

        state.current_strike_index.store(
            if completed_units < total_units {
                completed_units.saturating_sub(1)
            } else {
                PRICING_PROGRESS_NONE
            },
            Ordering::SeqCst,
        );
    }

    pub(crate) fn finish_strike_pricing(&self) {
        let Some(state) = &self.state else {
            return;
        };

        if state.phase() != Some(PricingProgressPhase::StrikePricingProgress) {
            return;
        }

        let total_units = state.total_units.load(Ordering::SeqCst);

        state.completed_units.store(total_units, Ordering::SeqCst);
        state
            .current_strike_index
            .store(PRICING_PROGRESS_NONE, Ordering::SeqCst);
    }

    pub(crate) fn finish(&self) {
        let Some(state) = &self.state else {
            return;
        };

        state.finished.store(true, Ordering::SeqCst);
    }

    pub(crate) fn poll_until_finished(&self, observer: &mut dyn FnMut(PricingProgressEvent)) {
        let Some(state) = &self.state else {
            return;
        };

        let mut last_snapshot = None::<PricingProgressEvent>;

        loop {
            if let Some(snapshot) = state.snapshot() {
                if last_snapshot.as_ref() != Some(&snapshot) {
                    observer(snapshot.clone());
                    last_snapshot = Some(snapshot);
                }
            }

            if state.finished.load(Ordering::SeqCst) {
                break;
            }

            sleep(PRICING_PROGRESS_POLL_INTERVAL);
        }

        if let Some(snapshot) = state.snapshot() {
            if last_snapshot.as_ref() != Some(&snapshot) {
                observer(snapshot);
            }
        }
    }
}

struct PricingProgressState {
    phase: AtomicUsize,
    completed_units: AtomicUsize,
    total_units: AtomicUsize,
    current_group_index: AtomicUsize,
    current_maturity_index: AtomicUsize,
    total_maturities: AtomicUsize,
    current_interval_index: AtomicUsize,
    total_intervals: AtomicUsize,
    current_transition_row_index: AtomicUsize,
    total_transition_rows: AtomicUsize,
    current_strike_index: AtomicUsize,
    total_strikes: AtomicUsize,
    finished: AtomicBool,
}

impl PricingProgressState {
    const fn new() -> Self {
        Self {
            phase: AtomicUsize::new(PRICING_PROGRESS_NONE),
            completed_units: AtomicUsize::new(0),
            total_units: AtomicUsize::new(0),
            current_group_index: AtomicUsize::new(PRICING_PROGRESS_NONE),
            current_maturity_index: AtomicUsize::new(PRICING_PROGRESS_NONE),
            total_maturities: AtomicUsize::new(PRICING_PROGRESS_NONE),
            current_interval_index: AtomicUsize::new(PRICING_PROGRESS_NONE),
            total_intervals: AtomicUsize::new(PRICING_PROGRESS_NONE),
            current_transition_row_index: AtomicUsize::new(PRICING_PROGRESS_NONE),
            total_transition_rows: AtomicUsize::new(PRICING_PROGRESS_NONE),
            current_strike_index: AtomicUsize::new(PRICING_PROGRESS_NONE),
            total_strikes: AtomicUsize::new(PRICING_PROGRESS_NONE),
            finished: AtomicBool::new(false),
        }
    }

    fn phase(&self) -> Option<PricingProgressPhase> {
        match self.phase.load(Ordering::SeqCst) {
            0 => Some(PricingProgressPhase::GroupProgress),
            1 => Some(PricingProgressPhase::MaturityProgress),
            2 => Some(PricingProgressPhase::IntervalProgress),
            3 => Some(PricingProgressPhase::TransitionRowProgress),
            4 => Some(PricingProgressPhase::StrikePricingProgress),
            _ => None,
        }
    }

    fn store_event(&self, event: &PricingProgressEvent) {
        self.phase.store(
            match event.phase {
                PricingProgressPhase::GroupProgress => 0,
                PricingProgressPhase::MaturityProgress => 1,
                PricingProgressPhase::IntervalProgress => 2,
                PricingProgressPhase::TransitionRowProgress => 3,
                PricingProgressPhase::StrikePricingProgress => 4,
            },
            Ordering::SeqCst,
        );

        self.completed_units
            .store(event.completed_units, Ordering::SeqCst);
        self.total_units.store(event.total_units, Ordering::SeqCst);

        self.current_group_index.store(
            event.current_group_index.unwrap_or(PRICING_PROGRESS_NONE),
            Ordering::SeqCst,
        );
        self.current_maturity_index.store(
            event
                .current_maturity_index
                .unwrap_or(PRICING_PROGRESS_NONE),
            Ordering::SeqCst,
        );

        self.total_maturities.store(
            event.total_maturities.unwrap_or(PRICING_PROGRESS_NONE),
            Ordering::SeqCst,
        );

        self.current_interval_index.store(
            event
                .current_interval_index
                .unwrap_or(PRICING_PROGRESS_NONE),
            Ordering::SeqCst,
        );
        self.total_intervals.store(
            event.total_intervals.unwrap_or(PRICING_PROGRESS_NONE),
            Ordering::SeqCst,
        );

        self.current_transition_row_index.store(
            event
                .current_transition_row_index
                .unwrap_or(PRICING_PROGRESS_NONE),
            Ordering::SeqCst,
        );
        self.total_transition_rows.store(
            event.total_transition_rows.unwrap_or(PRICING_PROGRESS_NONE),
            Ordering::SeqCst,
        );

        self.current_strike_index.store(
            event.current_strike_index.unwrap_or(PRICING_PROGRESS_NONE),
            Ordering::SeqCst,
        );
        self.total_strikes.store(
            event.total_strikes.unwrap_or(PRICING_PROGRESS_NONE),
            Ordering::SeqCst,
        );
    }

    fn snapshot(&self) -> Option<PricingProgressEvent> {
        Some(PricingProgressEvent {
            phase: self.phase()?,
            completed_units: self.completed_units.load(Ordering::SeqCst),
            total_units: self.total_units.load(Ordering::SeqCst),
            current_group_index: decode_optional_usize(
                self.current_group_index.load(Ordering::SeqCst),
            ),
            current_maturity_index: decode_optional_usize(
                self.current_maturity_index.load(Ordering::SeqCst),
            ),
            total_maturities: decode_optional_usize(self.total_maturities.load(Ordering::SeqCst)),
            current_interval_index: decode_optional_usize(
                self.current_interval_index.load(Ordering::SeqCst),
            ),
            total_intervals: decode_optional_usize(self.total_intervals.load(Ordering::SeqCst)),
            current_transition_row_index: decode_optional_usize(
                self.current_transition_row_index.load(Ordering::SeqCst),
            ),
            total_transition_rows: decode_optional_usize(
                self.total_transition_rows.load(Ordering::SeqCst),
            ),
            current_strike_index: decode_optional_usize(
                self.current_strike_index.load(Ordering::SeqCst),
            ),
            total_strikes: decode_optional_usize(self.total_strikes.load(Ordering::SeqCst)),
        })
    }
}

const fn decode_optional_usize(value: usize) -> Option<usize> {
    if value == PRICING_PROGRESS_NONE {
        None
    } else {
        Some(value)
    }
}
