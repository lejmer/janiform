use janiform_calibration_artifacts::{VariancePersistenceFit, VariancePersistenceIdentification};
use janiform_foundations::numeric::coverage_ratio;
use janiform_model_kernels::long_variance::LongVarianceParameters;

use super::covariance::{
    VarianceCovariance, covariances_from_normalized_variance, normalized_variance,
};
use super::feedback::estimate_variance_feedback;
use super::observations::VarianceObservation;
use super::persistence::fit_variance_persistence;
use super::policy::{
    LongVarianceStationaryEventRates, project_long_variance_feedback_loadings_to_stationary_budget,
};
use crate::stages::joint_metrics::JointRefinementAggregatedIntervalObservation;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LongVarianceParameterEstimation {
    pub(crate) parameters: LongVarianceParameters,
    pub(crate) diagnostics: LongVarianceParameterEstimationDiagnostics,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LongVarianceParameterEstimationDiagnostics {
    pub(crate) persistence_lag_diagnostics: Vec<VarianceCovariance>,
    pub(crate) selected_mean_reversion_rate: f64,
    pub(crate) persistence_fit: VariancePersistenceFit,
    pub(crate) event_loading_fit_candidate_pair_count: usize,
    pub(crate) event_loading_fit_pair_count: usize,
    pub(crate) event_loading_fit_coverage_ratio: f64,
    pub(crate) stationary_return_energy_rate_per_hour: f64,
    pub(crate) return_energy_fit_candidate_pair_count: usize,
    pub(crate) return_energy_fit_pair_count: usize,
    pub(crate) return_energy_fit_coverage_ratio: f64,
}

pub(crate) fn estimate_long_variance_parameters(
    stationary_event_rates: LongVarianceStationaryEventRates,
    activity_intervals: &[JointRefinementAggregatedIntervalObservation],
    variance_observations: &[VarianceObservation],
) -> LongVarianceParameterEstimation {
    let normalized_observations = normalized_variance(variance_observations);

    let persistence_lag_diagnostics =
        covariances_from_normalized_variance(&normalized_observations, true);

    let mut persistence_fit =
        fit_variance_persistence(variance_observations, &persistence_lag_diagnostics);

    let identified = persistence_fit.identified_dynamics();

    let (parameters, feedback) = match identified {
        Some((mean_reversion_rate, variance)) => {
            let feedback = estimate_variance_feedback(
                variance_observations,
                &normalized_observations,
                activity_intervals,
                mean_reversion_rate,
            );

            let loadings = project_long_variance_feedback_loadings_to_stationary_budget(
                feedback.loadings,
                stationary_event_rates,
                mean_reversion_rate,
            );

            let parameters = LongVarianceParameters::try_new(
                mean_reversion_rate,
                (2.0 * mean_reversion_rate * variance).sqrt(),
                loadings.stationary_return_energy_rate_per_hour,
                loadings.return_energy_loading,
                loadings.buy_event_loading,
                loadings.sell_event_loading,
            );

            match parameters {
                Ok(parameters) => (parameters, Some(feedback)),
                Err(_) => {
                    persistence_fit.identification =
                        VariancePersistenceIdentification::InvalidEstimate;
                    (LongVarianceParameters::neutral(), None)
                }
            }
        }
        None => (LongVarianceParameters::neutral(), None),
    };

    let candidate_pair_count = feedback.map_or(0, |fit| fit.candidate_pair_count);
    let activity_pair_count = feedback.map_or(0, |fit| fit.activity_pair_count);
    let energy_pair_count = feedback.map_or(0, |fit| fit.energy_pair_count);

    LongVarianceParameterEstimation {
        parameters,
        diagnostics: LongVarianceParameterEstimationDiagnostics {
            persistence_lag_diagnostics,
            selected_mean_reversion_rate: parameters.mean_reversion_rate,
            persistence_fit,
            event_loading_fit_candidate_pair_count: candidate_pair_count,
            event_loading_fit_pair_count: activity_pair_count,
            event_loading_fit_coverage_ratio: coverage_ratio(
                activity_pair_count,
                candidate_pair_count,
            ),
            stationary_return_energy_rate_per_hour: parameters
                .stationary_return_energy_rate_per_hour,
            return_energy_fit_candidate_pair_count: candidate_pair_count,
            return_energy_fit_pair_count: energy_pair_count,
            return_energy_fit_coverage_ratio: coverage_ratio(
                energy_pair_count,
                candidate_pair_count,
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use janiform_foundations::time::NANOS_PER_HOUR;
    use janiform_model_kernels::long_variance::LongVarianceParameters;

    use super::estimate_long_variance_parameters;
    use crate::stages::long_variance::observations::VarianceObservation;
    use crate::stages::long_variance::policy::LongVarianceStationaryEventRates;

    #[test]
    fn insufficient_history_disables_diffusion_and_all_feedback_at_any_return_scale() {
        for scale in [1.0e-12, 1.0, 1.0e12] {
            for hours in [0, 24, 72] {
                let observations = (0..hours)
                    .map(|hour| VarianceObservation {
                        start: hour * NANOS_PER_HOUR,
                        end: (hour + 1) * NANOS_PER_HOUR,
                        energy: scale * (1.0 + (hour as f64 / 12.0).sin()).powi(2),
                        sampling_count: 12,
                    })
                    .collect::<Vec<_>>();

                let result = estimate_long_variance_parameters(
                    LongVarianceStationaryEventRates {
                        buy_event_rate_per_hour: 100.0,
                        sell_event_rate_per_hour: 200.0,
                    },
                    &[],
                    &observations,
                );

                assert_eq!(result.parameters, LongVarianceParameters::neutral());
                assert!(result.diagnostics.persistence_fit.warning().is_some());
                assert_eq!(result.diagnostics.event_loading_fit_pair_count, 0);
                assert_eq!(result.diagnostics.return_energy_fit_pair_count, 0);
            }
        }
    }
}
