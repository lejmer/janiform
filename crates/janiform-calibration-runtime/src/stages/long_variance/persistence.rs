use janiform_calibration_artifacts::{VariancePersistenceFit, VariancePersistenceIdentification};
use janiform_foundations::numeric::{GOLDEN_RATIO_RECIPROCAL, square_root_machine_epsilon};
use janiform_foundations::time::duration_nanos_to_hours;
use janiform_model_kernels::long_variance::integrated_cir_covariance_factor;

use super::covariance::{VarianceCovariance, supports_persistence_fit};
use super::observations::VarianceObservation;
use super::policy::{
    LONG_VARIANCE_STANDARD_CIR_VARIANCE_PROXY_BOUNDARY,
    long_variance_mean_reversion_rate_calibration_upper_bound,
};

// A fixed 64-cell log-rate scan bounds initializer work independently of tick count.
// One weighted squared-error unit defines descriptive profile support.
const PROFILE_CELL_COUNT: usize = 64;
const PROFILE_SUPPORT_LOSS_INCREMENT: f64 = 1.0;

#[derive(Debug, Clone, Copy)]
struct ProfilePoint {
    rate: f64,
    variance: f64,
    noise: f64,
    loss: f64,
}

pub(crate) fn fit_variance_persistence(
    observations: &[VarianceObservation],
    covariances: &[VarianceCovariance],
) -> VariancePersistenceFit {
    let mut result = VariancePersistenceFit {
        identification: VariancePersistenceIdentification::InsufficientObservations,
        observation_count: observations.len(),
        single_return_observation_count: observations
            .iter()
            .filter(|row| row.sampling_count == 1)
            .count(),
        mean_reversion_rate: None,
        latent_variance: None,
        observation_noise_variance: None,
        supported_rate_lower: None,
        supported_rate_upper: None,
    };

    let Some((lower, upper)) = persistence_rate_domain(observations) else {
        return result;
    };

    result.supported_rate_lower = Some(lower);
    result.supported_rate_upper = Some(upper);

    let moments = covariances
        .iter()
        .copied()
        .filter(|moment| {
            moment.value.is_some_and(f64::is_finite)
                && moment
                    .standard_error
                    .is_some_and(|error| error.is_finite() && error > 0.0)
        })
        .collect::<Vec<_>>();

    if !supports_persistence_fit(&moments) {
        return result;
    }

    let log_lower = lower.ln();
    let step = (upper.ln() - log_lower) / PROFILE_CELL_COUNT as f64;

    let profile = (0..=PROFILE_CELL_COUNT)
        .filter_map(|position| profile_point((log_lower + position as f64 * step).exp(), &moments))
        .collect::<Vec<_>>();

    let Some(best) = profile
        .iter()
        .min_by(|left, right| left.loss.total_cmp(&right.loss))
        .copied()
    else {
        return result;
    };

    let best = refine_profile(best, lower, upper, step, &moments);

    populate_profile_diagnostics(&mut result, best, &profile, (lower, upper), step);

    let noise_only_loss = moments
        .iter()
        .filter(|moment| moment.lag_hours > 0.0)
        .filter_map(|moment| Some((moment.value? / moment.standard_error?).powi(2)))
        .sum::<f64>();

    if noise_only_loss <= best.loss + PROFILE_SUPPORT_LOSS_INCREMENT {
        result.identification = VariancePersistenceIdentification::NoPersistentSignal;
        result.mean_reversion_rate = None;
    }

    result
}

fn populate_profile_diagnostics(
    result: &mut VariancePersistenceFit,
    best: ProfilePoint,
    profile: &[ProfilePoint],
    domain: (f64, f64),
    step: f64,
) {
    let (lower, upper) = domain;

    result.mean_reversion_rate = Some(best.rate);
    result.latent_variance = Some(best.variance);
    result.observation_noise_variance = Some(best.noise);

    if best.variance == 0.0 {
        result.identification = VariancePersistenceIdentification::NoPersistentSignal;
        result.mean_reversion_rate = None;
        result.supported_rate_lower = Some(lower);
        result.supported_rate_upper = Some(upper);

        return;
    }

    let supported = profile
        .iter()
        .filter(|point| point.loss <= best.loss + PROFILE_SUPPORT_LOSS_INCREMENT);

    // Include one adjacent scan cell on each side so grid resolution does not narrow support.
    let (mut support_lower, mut support_upper) = (best.rate, best.rate);

    for point in supported {
        support_lower = support_lower.min(point.rate);
        support_upper = support_upper.max(point.rate);
    }

    support_lower = (support_lower.ln() - step).exp().max(lower);
    support_upper = (support_upper.ln() + step).exp().min(upper);

    result.supported_rate_lower = Some(support_lower);
    result.supported_rate_upper = Some(support_upper);

    result.identification = if support_lower == lower || support_upper == upper {
        VariancePersistenceIdentification::BoundaryLimited
    } else {
        VariancePersistenceIdentification::Identified
    };
}

/// The longest resolved half-life is the observed time span. The shortest is the existing
/// five-minute identification policy. Rates outside this interval are boundary limited.
pub(crate) fn persistence_rate_domain(observations: &[VarianceObservation]) -> Option<(f64, f64)> {
    let span = observations
        .last()?
        .start
        .checked_sub(observations.first()?.start)?;

    let hours = duration_nanos_to_hours(span);

    (hours > 0.0).then(|| {
        (
            std::f64::consts::LN_2 / hours,
            long_variance_mean_reversion_rate_calibration_upper_bound(),
        )
    })
}

fn profile_point(rate: f64, moments: &[VarianceCovariance]) -> Option<ProfilePoint> {
    let rows = moments
        .iter()
        .map(|moment| {
            let factor =
                integrated_cir_covariance_factor(rate, moment.window_hours, moment.lag_hours)?;
            Some((
                factor / moment.standard_error?,
                moment.value? / moment.standard_error?,
                moment,
            ))
        })
        .collect::<Option<Vec<_>>>()?;

    let mut numerator = 0.0;
    let mut denominator = 0.0;
    let mut zero_rows = Vec::new();

    for &(factor, target, moment) in &rows {
        if moment.lag_hours == 0.0 {
            zero_rows.push((factor, target));
        } else {
            numerator += factor * target;
            denominator += factor * factor;
        }
    }

    if !denominator.is_finite() || denominator <= 0.0 {
        return None;
    }

    let maximum = LONG_VARIANCE_STANDARD_CIR_VARIANCE_PROXY_BOUNDARY;

    let mut variance = (numerator / denominator).clamp(0.0, maximum);

    // Each sampling group has its own nonnegative observation-noise term. Eliminating
    // these terms gives zero-lag hinges max(factor * variance - target, 0)^2.
    // Sorted hinge thresholds permit an exact scalar active-set solve.
    zero_rows.sort_by(|(a, y), (b, z)| (y / a).total_cmp(&(z / b)));

    for (factor, target) in zero_rows {
        if factor * variance <= target {
            break;
        }

        numerator += factor * target;
        denominator += factor * factor;

        variance = (numerator / denominator).clamp(0.0, maximum);
    }

    let mut loss = 0.0;
    let mut noise_sum = 0.0;
    let mut noise_count = 0;

    for (factor, target, moment) in rows {
        let residual = factor * variance - target;

        if moment.lag_hours == 0.0 {
            loss += residual.max(0.0).powi(2);
            noise_sum += (-residual).max(0.0) * moment.standard_error?;
            noise_count += 1;
        } else {
            loss += residual.powi(2);
        }
    }

    let noise = noise_sum / noise_count as f64;

    (loss.is_finite() && variance.is_finite() && noise.is_finite()).then_some(ProfilePoint {
        rate,
        variance,
        noise,
        loss,
    })
}

fn refine_profile(
    mut best: ProfilePoint,
    lower: f64,
    upper: f64,
    step: f64,
    moments: &[VarianceCovariance],
) -> ProfilePoint {
    let mut left = (best.rate.ln() - step).max(lower.ln());
    let mut right = (best.rate.ln() + step).min(upper.ln());

    // Rate precision beyond sqrt(machine epsilon) is immaterial to the moment fit.
    while right - left > square_root_machine_epsilon() {
        let first = right - GOLDEN_RATIO_RECIPROCAL * (right - left);
        let second = left + GOLDEN_RATIO_RECIPROCAL * (right - left);

        let (Some(a), Some(b)) = (
            profile_point(first.exp(), moments),
            profile_point(second.exp(), moments),
        ) else {
            break;
        };

        for point in [a, b] {
            if point.loss < best.loss {
                best = point;
            }
        }

        if a.loss <= b.loss {
            right = second;
        } else {
            left = first;
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use janiform_calibration_artifacts::VariancePersistenceIdentification;
    use janiform_foundations::time::NANOS_PER_HOUR;
    use janiform_model_kernels::long_variance::integrated_cir_covariance_factor;

    use super::fit_variance_persistence;
    use crate::stages::long_variance::covariance::VarianceCovariance;
    use crate::stages::long_variance::observations::VarianceObservation;

    fn observations() -> Vec<VarianceObservation> {
        (0..24 * 90)
            .map(|hour| VarianceObservation {
                start: hour * NANOS_PER_HOUR,
                end: (hour + 1) * NANOS_PER_HOUR,
                energy: 1.0,
                sampling_count: 12,
            })
            .collect()
    }

    fn moments(rate: f64, variance: f64, noise: f64) -> Vec<VarianceCovariance> {
        [0, 1, 2, 4, 8, 24, 48, 168]
            .into_iter()
            .map(|lag| {
                let signal =
                    variance * integrated_cir_covariance_factor(rate, 1.0, lag as f64).unwrap();

                let value = if lag == 0 { signal + noise } else { signal };

                VarianceCovariance {
                    lag_hours: lag as f64,
                    window_hours: 1.0,
                    sampling_count: 12,
                    pair_count: 2000,
                    value: Some(value),
                    standard_error: Some(0.001),
                }
            })
            .collect()
    }

    #[test]
    fn mixed_interval_widths_share_decay_with_separate_observation_noise() {
        let rate = std::f64::consts::LN_2 / 72.0;
        let variance = 0.15;

        let mut targets = Vec::new();

        for (window_hours, noise) in [(0.25, 2.0), (0.5, 0.7), (24.0, 5.0)] {
            for step in [0, 1, 2, 4, 8, 16] {
                let lag_hours = window_hours * f64::from(step);

                let signal = variance
                    * integrated_cir_covariance_factor(rate, window_hours, lag_hours).unwrap();

                let value = if step == 0 { signal + noise } else { signal };

                targets.push(VarianceCovariance {
                    lag_hours,
                    window_hours,
                    sampling_count: 1,
                    pair_count: 1000,
                    value: Some(value),
                    standard_error: Some(0.001),
                });
            }
        }

        let fit = fit_variance_persistence(&observations(), &targets);

        let (fitted_rate, fitted_variance) = fit.identified_dynamics().unwrap();

        let tolerance = f64::EPSILON.sqrt().sqrt();

        assert!((fitted_rate / rate - 1.0).abs() < tolerance);
        assert!((fitted_variance / variance - 1.0).abs() < tolerance);
    }

    #[test]
    fn variance_persistence_recovers_decay_independently_of_observation_noise() {
        let rate = std::f64::consts::LN_2 / 24.0;
        let variance = 0.2;

        // The tolerance exceeds scalar-search roundoff, while resolving a day-long half-life.
        let tolerance = f64::EPSILON.sqrt().sqrt();

        for noise in [0.0, 0.1, 2.0, 10.0] {
            let fit = fit_variance_persistence(&observations(), &moments(rate, variance, noise));

            assert_eq!(
                fit.identification,
                VariancePersistenceIdentification::Identified
            );

            assert!((fit.mean_reversion_rate.unwrap() / rate - 1.0).abs() < tolerance);
            assert!((fit.latent_variance.unwrap() - variance).abs() < tolerance);
            assert!((fit.observation_noise_variance.unwrap() - noise).abs() < tolerance);
            assert!(fit.supported_rate_lower.unwrap() < rate);
            assert!(fit.supported_rate_upper.unwrap() > rate);
        }
    }

    #[test]
    fn variance_persistence_separates_constant_volatility_noise_and_unidentified_rate() {
        let fit = fit_variance_persistence(&observations(), &moments(0.2, 0.0, 2.0));

        assert_eq!(
            (
                fit.identification,
                fit.mean_reversion_rate,
                fit.latent_variance,
                fit.observation_noise_variance
            ),
            (
                VariancePersistenceIdentification::NoPersistentSignal,
                None,
                Some(0.0),
                Some(2.0)
            ),
        );

        let fit = fit_variance_persistence(&[], &[]);

        assert_eq!(
            fit.identification,
            VariancePersistenceIdentification::InsufficientObservations
        );

        assert_eq!(fit.mean_reversion_rate, None);
    }

    #[test]
    fn variance_persistence_reports_boundary_support_and_uses_negative_covariances() {
        let mut measured = moments(0.001, 0.1, 0.4);

        for moment in &mut measured {
            moment.standard_error = Some(1.0);
        }

        let fit = fit_variance_persistence(&observations(), &measured);

        assert_eq!(
            fit.identification,
            VariancePersistenceIdentification::NoPersistentSignal
        );

        for moment in measured.iter_mut().filter(|moment| moment.lag_hours > 0.0) {
            moment.value = Some(-0.1);
        }

        let fit = fit_variance_persistence(&observations(), &measured);

        assert_eq!(
            fit.identification,
            VariancePersistenceIdentification::NoPersistentSignal
        );
    }
}
