use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use janiform_model_kernels::nig::NigParameters;

use super::types::ContinuousReturnProxyEmDiagnostic;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProxyEmFitStopReason {
    Converged,
    NonImproving,
    NumericalFailure,
    IterationBudgetExhausted,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ProxyEmFitOutcome {
    pub(super) parameters: NigParameters,
    pub(super) stop_reason: ProxyEmFitStopReason,
}

pub(super) struct ProxyEmSeedCounters {
    pub(super) completed_seed_count: AtomicUsize,
    pub(super) refined_seed_count: AtomicUsize,
    pub(super) invalid_initial_seed_count: AtomicUsize,
    converged_seed_count: AtomicUsize,
    non_improving_seed_count: AtomicUsize,
    numerical_failure_seed_count: AtomicUsize,
    iteration_budget_exhausted_seed_count: AtomicUsize,
    pub(super) projection_failure_seed_count: AtomicUsize,
}

impl ProxyEmSeedCounters {
    pub(super) fn new() -> Self {
        Self {
            completed_seed_count: AtomicUsize::new(0),
            refined_seed_count: AtomicUsize::new(0),
            invalid_initial_seed_count: AtomicUsize::new(0),
            converged_seed_count: AtomicUsize::new(0),
            non_improving_seed_count: AtomicUsize::new(0),
            numerical_failure_seed_count: AtomicUsize::new(0),
            iteration_budget_exhausted_seed_count: AtomicUsize::new(0),
            projection_failure_seed_count: AtomicUsize::new(0),
        }
    }

    pub(super) fn record_fit_stop_reason(&self, stop_reason: ProxyEmFitStopReason) {
        match stop_reason {
            ProxyEmFitStopReason::Converged => {
                self.converged_seed_count.fetch_add(1, Ordering::Relaxed);
            }
            ProxyEmFitStopReason::NonImproving => {
                self.non_improving_seed_count
                    .fetch_add(1, Ordering::Relaxed);
            }
            ProxyEmFitStopReason::NumericalFailure => {
                self.numerical_failure_seed_count
                    .fetch_add(1, Ordering::Relaxed);
            }
            ProxyEmFitStopReason::IterationBudgetExhausted => {
                self.iteration_budget_exhausted_seed_count
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub(super) fn record_initial_numerical_failure(&self) {
        self.numerical_failure_seed_count
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn diagnostic(
        &self,
        best_objective: &Mutex<Option<f64>>,
        final_convergence_ratio: &Mutex<Option<f64>>,
        evaluation_count: &AtomicUsize,
        requested_seed_count: usize,
        iteration_budget_per_seed: usize,
        fitting_sample_count: usize,
        fitting_sample_weight_sum: f64,
    ) -> ContinuousReturnProxyEmDiagnostic {
        ContinuousReturnProxyEmDiagnostic {
            best_objective: best_objective.lock().ok().and_then(|guard| *guard),
            final_convergence_ratio: final_convergence_ratio.lock().ok().and_then(|guard| *guard),
            evaluation_count: evaluation_count.load(Ordering::Relaxed),
            completed_seed_count: self.completed_seed_count.load(Ordering::Relaxed),
            refined_seed_count: self.refined_seed_count.load(Ordering::Relaxed),
            requested_seed_count,
            iteration_budget_per_seed,
            fitting_sample_count,
            fitting_sample_weight_sum,
            converged_seed_count: self.converged_seed_count.load(Ordering::Relaxed),
            non_improving_seed_count: self.non_improving_seed_count.load(Ordering::Relaxed),
            numerical_failure_seed_count: self.numerical_failure_seed_count.load(Ordering::Relaxed),
            iteration_budget_exhausted_seed_count: self
                .iteration_budget_exhausted_seed_count
                .load(Ordering::Relaxed),
            projection_failure_seed_count: self
                .projection_failure_seed_count
                .load(Ordering::Relaxed),
            invalid_initial_seed_count: self.invalid_initial_seed_count.load(Ordering::Relaxed),
        }
    }
}
