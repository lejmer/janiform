//! Fourier business-time clock over recurring exchange-local calendar phases.

mod coordinates;
mod exposure;
mod fit;
mod materialization;

#[cfg(test)]
mod tests;

use chrono::NaiveDate;
pub use exposure::{BusinessTimeClockExposureRow, build_business_time_clock_exposure_rows};
pub use fit::{
    FourierBusinessTimeClockFitError, FourierBusinessTimeClockFitInput,
    FourierBusinessTimeClockFitProgress, FourierBusinessTimeClockFitProgressPhase,
    FourierBusinessTimeClockFitSettings, FourierBusinessTimeClockFitSummary,
    fit_fourier_business_time_clock_model,
};
use janiform_foundations::numeric::saturating_positive_exp;
pub use materialization::{
    FourierBusinessTimeClockMaterializationError, FourierBusinessTimeClockMaterializationInput,
    FourierBusinessTimeClockMaterializationOutput, materialize_fourier_business_time_clock,
};
use serde::{Deserialize, Serialize};

/// Schema for fitted recurring Fourier business-time clock models.
pub const FOURIER_BUSINESS_TIME_CLOCK_MODEL_SCHEMA_VERSION: &str =
    "business_time_clock.fourier_model.v1";

/// Coordinate system used by the Fourier clock.
pub const FOURIER_BUSINESS_TIME_CLOCK_COORDINATE_SYSTEM: &str =
    "exchange_local_minute_of_day_weekday_plus_civil_year_phase.v1";

/// Basis contract for the stored Fourier coefficients.
pub const FOURIER_BUSINESS_TIME_CLOCK_BASIS: &str =
    "log_rate_intercept_then_intraday_cos_sin_then_weekly_cos_sin_then_annual_cos_sin.v1";

/// Normalization policy for fitted clock rates.
pub const FOURIER_BUSINESS_TIME_CLOCK_NORMALIZATION_POLICY: &str =
    "fit_open_exposure_weighted_mean_rate_equals_one.v1";

/// Candidate-selection policy for fitted clock rates.
pub const FOURIER_BUSINESS_TIME_CLOCK_SELECTION_POLICY: &str =
    "aicc_over_open_minute_poisson_exposure_likelihood.v1";

/// Fitted recurring business-time clock.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FourierBusinessTimeClockModel {
    pub schema_version: String,
    pub coordinate_system: String,
    pub coefficient_basis: String,
    pub normalization_policy: String,
    pub selection_policy: String,
    pub intraday_harmonic_order: usize,
    pub weekly_harmonic_order: usize,
    pub annual_harmonic_order: usize,
    pub intercept: f64,
    pub intraday_cosine_coefficients: Vec<f64>,
    pub intraday_sine_coefficients: Vec<f64>,
    pub weekly_cosine_coefficients: Vec<f64>,
    pub weekly_sine_coefficients: Vec<f64>,
    pub annual_cosine_coefficients: Vec<f64>,
    pub annual_sine_coefficients: Vec<f64>,
    pub selected_objective_value: f64,
    pub selected_negative_log_likelihood: f64,
    pub evaluated_candidate_count: usize,
    pub fit_open_minute_count: usize,
    pub fit_open_event_count: usize,
    pub fit_open_exposure_seconds: f64,
}

impl FourierBusinessTimeClockModel {
    /// Evaluate the normalized business-time rate at recurring local phases.
    pub fn rate_at_phases(
        &self,
        intraday_phase: f64,
        weekly_phase: f64,
        annual_phase: f64,
    ) -> Option<f64> {
        if !intraday_phase.is_finite() || !weekly_phase.is_finite() || !annual_phase.is_finite() {
            return None;
        }

        let log_rate = coordinates::fourier_log_rate(
            coordinates::FourierCoefficientSlices {
                intercept: self.intercept,
                intraday_cosine: self.intraday_cosine_coefficients.as_slice(),
                intraday_sine: self.intraday_sine_coefficients.as_slice(),
                weekly_cosine: self.weekly_cosine_coefficients.as_slice(),
                weekly_sine: self.weekly_sine_coefficients.as_slice(),
                annual_cosine: self.annual_cosine_coefficients.as_slice(),
                annual_sine: self.annual_sine_coefficients.as_slice(),
            },
            coordinates::FourierPhases {
                intraday: intraday_phase,
                weekly: weekly_phase,
                annual: annual_phase,
            },
        )?;

        let rate = log_rate.exp();
        (rate.is_finite() && rate > 0.0).then_some(rate)
    }

    /// Return a finite upper bound for the normalized rate over every phase pair.
    pub fn rate_upper_bound(&self) -> Option<f64> {
        let log_rate_bound = harmonic_log_rate_upper_bound(
            self.intercept,
            self.intraday_cosine_coefficients.as_slice(),
            self.intraday_sine_coefficients.as_slice(),
            self.weekly_cosine_coefficients.as_slice(),
            self.weekly_sine_coefficients.as_slice(),
            self.annual_cosine_coefficients.as_slice(),
            self.annual_sine_coefficients.as_slice(),
        )?;

        saturating_positive_exp(log_rate_bound)
    }

    /// Evaluate the normalized business-time rate at an exchange-local minute.
    pub fn rate_at_local_minute(&self, local_date: NaiveDate, minute_of_day: usize) -> Option<f64> {
        let intraday_phase = coordinates::intraday_phase(minute_of_day)?;
        let weekly_phase = coordinates::weekly_phase(local_date)?;
        let annual_phase = coordinates::annual_phase(local_date, minute_of_day)?;

        self.rate_at_phases(intraday_phase, weekly_phase, annual_phase)
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.schema_version != FOURIER_BUSINESS_TIME_CLOCK_MODEL_SCHEMA_VERSION {
            return Err(format!(
                "business-time clock schema_version must be `{}`, found `{}`",
                FOURIER_BUSINESS_TIME_CLOCK_MODEL_SCHEMA_VERSION, self.schema_version
            ));
        }

        if self.coordinate_system != FOURIER_BUSINESS_TIME_CLOCK_COORDINATE_SYSTEM {
            return Err(format!(
                "business-time clock coordinate_system must be `{}`, found `{}`",
                FOURIER_BUSINESS_TIME_CLOCK_COORDINATE_SYSTEM, self.coordinate_system
            ));
        }

        if self.coefficient_basis != FOURIER_BUSINESS_TIME_CLOCK_BASIS {
            return Err(format!(
                "business-time clock coefficient_basis must be `{}`, found `{}`",
                FOURIER_BUSINESS_TIME_CLOCK_BASIS, self.coefficient_basis
            ));
        }

        if self.intraday_cosine_coefficients.len() != self.intraday_harmonic_order
            || self.intraday_sine_coefficients.len() != self.intraday_harmonic_order
            || self.weekly_cosine_coefficients.len() != self.weekly_harmonic_order
            || self.weekly_sine_coefficients.len() != self.weekly_harmonic_order
            || self.annual_cosine_coefficients.len() != self.annual_harmonic_order
            || self.annual_sine_coefficients.len() != self.annual_harmonic_order
        {
            return Err(
                "business-time clock coefficient lengths must match harmonic orders".to_string(),
            );
        }

        let all_coefficients_are_finite = std::iter::once(&self.intercept)
            .chain(self.intraday_cosine_coefficients.iter())
            .chain(self.intraday_sine_coefficients.iter())
            .chain(self.weekly_cosine_coefficients.iter())
            .chain(self.weekly_sine_coefficients.iter())
            .chain(self.annual_cosine_coefficients.iter())
            .chain(self.annual_sine_coefficients.iter())
            .all(|value| value.is_finite());
        if !all_coefficients_are_finite {
            return Err("business-time clock coefficients must be finite".to_string());
        }

        if !self.selected_objective_value.is_finite()
            || !self.selected_negative_log_likelihood.is_finite()
            || !self.fit_open_exposure_seconds.is_finite()
            || self.fit_open_exposure_seconds <= 0.0
            || self.fit_open_minute_count == 0
            || self.fit_open_event_count == 0
            || self.evaluated_candidate_count == 0
        {
            return Err("business-time clock fit metadata is invalid".to_string());
        }

        Ok(())
    }
}

fn harmonic_log_rate_upper_bound(
    intercept: f64,
    intraday_cosine_coefficients: &[f64],
    intraday_sine_coefficients: &[f64],
    weekly_cosine_coefficients: &[f64],
    weekly_sine_coefficients: &[f64],
    annual_cosine_coefficients: &[f64],
    annual_sine_coefficients: &[f64],
) -> Option<f64> {
    if intraday_cosine_coefficients.len() != intraday_sine_coefficients.len()
        || weekly_cosine_coefficients.len() != weekly_sine_coefficients.len()
        || annual_cosine_coefficients.len() != annual_sine_coefficients.len()
        || !intercept.is_finite()
    {
        return None;
    }

    let mut log_rate_bound = intercept;

    for (cosine_coefficient, sine_coefficient) in intraday_cosine_coefficients
        .iter()
        .zip(intraday_sine_coefficients.iter())
        .chain(
            weekly_cosine_coefficients
                .iter()
                .zip(weekly_sine_coefficients.iter()),
        )
        .chain(
            annual_cosine_coefficients
                .iter()
                .zip(annual_sine_coefficients.iter()),
        )
    {
        if !cosine_coefficient.is_finite() || !sine_coefficient.is_finite() {
            return None;
        }

        let amplitude = cosine_coefficient.hypot(*sine_coefficient);
        if amplitude.is_nan() {
            return None;
        }
        if amplitude.is_infinite() && amplitude.is_sign_positive() {
            return Some(f64::INFINITY);
        }

        log_rate_bound += amplitude;

        if log_rate_bound.is_infinite() && log_rate_bound.is_sign_positive() {
            return Some(f64::INFINITY);
        }
        if !log_rate_bound.is_finite() {
            return None;
        }
    }

    Some(log_rate_bound)
}
