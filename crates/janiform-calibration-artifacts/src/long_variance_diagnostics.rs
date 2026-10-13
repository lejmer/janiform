use serde::{Deserialize, Serialize};

/// Identification of the approximate CIR covariance initializer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariancePersistenceIdentification {
    /// The observations do not support the covariance fit and its temporal uncertainty.
    InsufficientObservations,
    /// Observation noise alone remains within the descriptive profile support.
    NoPersistentSignal,
    /// The one-unit profile-loss support reaches a search boundary.
    BoundaryLimited,
    /// Numerical estimation could not produce valid model parameters.
    InvalidEstimate,
    /// The one-unit profile-loss support is interior to the search domain.
    Identified,
}

/// Approximate moment-profile support, conditional on the estimated seasonal profile.
/// The support is a diagnostic, not a likelihood confidence interval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VariancePersistenceFit {
    /// Identification status of the approximate covariance fit.
    pub identification: VariancePersistenceIdentification,
    /// Number of measured return-energy observations.
    pub observation_count: usize,
    /// Observations represented by one measured return square.
    pub single_return_observation_count: usize,
    /// Fitted decay rate per hour, absent when persistence is unidentified.
    pub mean_reversion_rate: Option<f64>,
    /// Persistent stationary dispersion before full-model feedback refinement.
    pub latent_variance: Option<f64>,
    /// Mean uncorrelated dispersion across duration and sampling-count groups.
    pub observation_noise_variance: Option<f64>,
    /// Lower rate in the descriptive one-unit profile-loss support.
    pub supported_rate_lower: Option<f64>,
    /// Upper rate in the descriptive one-unit profile-loss support.
    pub supported_rate_upper: Option<f64>,
}

impl VariancePersistenceFit {
    /// Return estimated dynamics only when persistence and amplitude are resolved.
    pub fn identified_dynamics(&self) -> Option<(f64, f64)> {
        if self.identification != VariancePersistenceIdentification::Identified {
            return None;
        }

        let rate = self.mean_reversion_rate?;
        let variance = self.latent_variance?;

        (rate.is_finite() && rate > 0.0 && variance.is_finite() && variance > 0.0)
            .then_some((rate, variance))
    }

    /// Explain why long-variance dynamics are disabled while calibration continues.
    pub fn warning(&self) -> Option<&'static str> {
        match self.identification {
            VariancePersistenceIdentification::Identified => None,
            VariancePersistenceIdentification::InvalidEstimate => Some(
                "Long-variance effects are disabled because estimation did not produce valid finite parameters. Calibration continues with the other model factors.",
            ),
            VariancePersistenceIdentification::InsufficientObservations => Some(
                "Long-variance effects are disabled because the supplied history does not support their estimation. Calibration continues with the other model factors.",
            ),
            VariancePersistenceIdentification::NoPersistentSignal => Some(
                "Long-variance effects are disabled because persistent variance cannot be distinguished from observation noise. Calibration continues with the other model factors.",
            ),
            VariancePersistenceIdentification::BoundaryLimited => Some(
                "Long-variance effects are disabled because the supplied history does not resolve their timescale. Calibration continues with the other model factors.",
            ),
        }
    }
}
