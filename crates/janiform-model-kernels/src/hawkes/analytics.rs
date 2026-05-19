use super::HawkesKernelLoadings;

/// Machine-readable stability output for the de-seasonalized `Alpha / beta` kernel.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct HawkesSpectralRadiusStabilityCheck {
    pub spectral_radius_alpha_over_beta: f64,
    pub strict_stationarity_threshold: f64,
    pub numeric_tolerance: f64,
    pub effective_stationarity_threshold: f64,
    pub is_stable: bool,
}

/// Stationary buy and sell intensity multipliers implied by `Alpha / beta`.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct HawkesStationaryIntensityMultipliers {
    pub buy_intensity_multiplier: f64,
    pub sell_intensity_multiplier: f64,
}

impl HawkesStationaryIntensityMultipliers {
    pub const fn total_intensity_multiplier(self) -> f64 {
        self.buy_intensity_multiplier + self.sell_intensity_multiplier
    }
}

/// Compute the shared baseline intensity that matches a stationary total event rate.
///
/// The Hawkes kernel stores the de-seasonalized buy and sell baseline as the
/// shared `lambda_bar`. The total stationary event rate is `lambda_bar` times
/// the sum of the stationary buy and sell intensity multipliers, so this is the
/// inverse map used when optimizing event-rate level separately from excitation
/// shape.
pub fn baseline_intensity_for_stationary_total_intensity(
    stationary_total_intensity: f64,
    alpha: HawkesKernelLoadings,
    beta: f64,
) -> Option<f64> {
    if !stationary_total_intensity.is_finite() || stationary_total_intensity <= 0.0 {
        return None;
    }

    let stationary_multipliers = stationary_intensity_multipliers(alpha, beta)?;

    let total_intensity_multiplier = stationary_multipliers.total_intensity_multiplier();
    if !total_intensity_multiplier.is_finite() || total_intensity_multiplier <= 0.0 {
        return None;
    }

    let baseline_intensity = stationary_total_intensity / total_intensity_multiplier;
    (baseline_intensity.is_finite() && baseline_intensity > 0.0).then_some(baseline_intensity)
}

/// Compute the spectral radius of the scaled Hawkes excitation matrix `Alpha / beta`.
///
/// Callers are expected to pass a finite, strictly positive `beta`.
pub fn spectral_radius_alpha_over_beta(alpha: HawkesKernelLoadings, beta: f64) -> f64 {
    let scaled_plus_plus = alpha.plus_plus / beta;
    let scaled_plus_minus = alpha.plus_minus / beta;
    let scaled_minus_plus = alpha.minus_plus / beta;
    let scaled_minus_minus = alpha.minus_minus / beta;

    let trace = scaled_plus_plus + scaled_minus_minus;

    let discriminant = ((scaled_plus_plus - scaled_minus_minus).powi(2)
        + 4.0 * scaled_plus_minus * scaled_minus_plus)
        .max(0.0);
    let square_root_discriminant = discriminant.sqrt();

    let first_eigenvalue = 0.5 * (trace + square_root_discriminant);
    let second_eigenvalue = 0.5 * (trace - square_root_discriminant);

    first_eigenvalue.abs().max(second_eigenvalue.abs())
}

/// Evaluate stationarity of the scaled Hawkes excitation matrix `Alpha / beta`
/// against a configurable strict threshold and one-sided numeric tolerance.
pub fn spectral_radius_stability_check(
    alpha: HawkesKernelLoadings,
    beta: f64,
    strict_stationarity_threshold: f64,
    numeric_tolerance: f64,
) -> HawkesSpectralRadiusStabilityCheck {
    let spectral_radius_alpha_over_beta = spectral_radius_alpha_over_beta(alpha, beta);
    let effective_stationarity_threshold = strict_stationarity_threshold - numeric_tolerance;

    HawkesSpectralRadiusStabilityCheck {
        spectral_radius_alpha_over_beta,
        strict_stationarity_threshold,
        numeric_tolerance,
        effective_stationarity_threshold,
        is_stable: spectral_radius_alpha_over_beta < effective_stationarity_threshold,
    }
}

/// Compute the stationary buy and sell intensity multipliers implied by the
/// scaled Hawkes kernel `Alpha / beta`.
///
/// Returns `None` when the implied stationary linear system is singular or when
/// `beta` is not finite and strictly positive.
pub fn stationary_intensity_multipliers(
    alpha: HawkesKernelLoadings,
    beta: f64,
) -> Option<HawkesStationaryIntensityMultipliers> {
    if !beta.is_finite() || beta <= 0.0 {
        return None;
    }

    let scaled_plus_plus = alpha.plus_plus / beta;
    let scaled_plus_minus = alpha.plus_minus / beta;
    let scaled_minus_plus = alpha.minus_plus / beta;
    let scaled_minus_minus = alpha.minus_minus / beta;

    let determinant = (1.0 - scaled_plus_plus) * (1.0 - scaled_minus_minus)
        - scaled_plus_minus * scaled_minus_plus;
    if !determinant.is_finite() || determinant == 0.0 {
        return None;
    }

    Some(HawkesStationaryIntensityMultipliers {
        buy_intensity_multiplier: ((1.0 - scaled_minus_minus) + scaled_plus_minus) / determinant,
        sell_intensity_multiplier: (scaled_minus_plus + (1.0 - scaled_plus_plus)) / determinant,
    })
}
