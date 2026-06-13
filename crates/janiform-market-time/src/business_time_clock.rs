//! Fourier business-time clock over recurring exchange-local calendar phases.

use std::collections::BTreeMap;

mod coordinates;
mod exposure;
mod fit;
mod materialization;

#[cfg(test)]
mod tests;

use chrono::NaiveDate;
pub use coordinates::{
    FOURIER_BUSINESS_TIME_CLOCK_AXES, FourierBusinessTimeClockAxis,
    FourierBusinessTimeClockHarmonicSeries, FourierBusinessTimeClockHarmonicSeriesByAxis,
    FourierBusinessTimeClockPhases, zero_order_harmonic_series_by_axis,
};
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
    "log_rate_intercept_then_axis_keyed_cos_sin_harmonic_series.v1";

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
    pub intercept: f64,
    pub harmonic_series: FourierBusinessTimeClockHarmonicSeriesByAxis,
    pub selected_objective_value: f64,
    pub selected_negative_log_likelihood: f64,
    pub evaluated_candidate_count: usize,
    pub fit_open_minute_count: usize,
    pub fit_open_event_count: usize,
    pub fit_open_exposure_seconds: f64,
}

impl FourierBusinessTimeClockModel {
    /// Evaluate the normalized business-time rate at recurring local phases.
    pub fn rate_at_phases(&self, phases: FourierBusinessTimeClockPhases) -> Option<f64> {
        if !phases.is_finite() {
            return None;
        }

        let log_rate = coordinates::fourier_log_rate(self.coefficient_slices(), phases)?;
        let rate = log_rate.exp();

        (rate.is_finite() && rate > 0.0).then_some(rate)
    }

    /// Return a finite upper bound for the normalized rate over every phase pair.
    pub fn rate_upper_bound(&self) -> Option<f64> {
        let log_rate_bound = harmonic_log_rate_upper_bound(self.coefficient_slices())?;

        saturating_positive_exp(log_rate_bound)
    }

    /// Evaluate the normalized business-time rate at an exchange-local minute.
    pub fn rate_at_local_minute(&self, local_date: NaiveDate, minute_of_day: usize) -> Option<f64> {
        let phases = coordinates::phases_for_local_minute(local_date, minute_of_day)?;

        self.rate_at_phases(phases)
    }

    /// Return fitted harmonic orders keyed by recurring axis.
    pub fn harmonic_orders(&self) -> BTreeMap<FourierBusinessTimeClockAxis, usize> {
        self.harmonic_series
            .iter()
            .map(|(axis, series)| (*axis, series.harmonic_order))
            .collect()
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

        validate_harmonic_series(self.coefficient_slices()).map_err(ToString::to_string)?;

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

    fn coefficient_slices(&self) -> coordinates::FourierCoefficientSlices<'_> {
        coordinates::FourierCoefficientSlices {
            intercept: self.intercept,
            harmonic_series: &self.harmonic_series,
        }
    }
}

fn validate_harmonic_series(
    coefficients: coordinates::FourierCoefficientSlices<'_>,
) -> Result<(), &'static str> {
    if !coefficients.intercept.is_finite() {
        return Err("business-time clock coefficients must be finite");
    }

    if coefficients.harmonic_series.len() != FOURIER_BUSINESS_TIME_CLOCK_AXES.len() {
        return Err("business-time clock harmonic series must contain exactly the canonical axes");
    }

    for axis in FOURIER_BUSINESS_TIME_CLOCK_AXES {
        let Some(series) = coefficients.series_for_axis(axis) else {
            return Err(
                "business-time clock harmonic series must contain exactly the canonical axes",
            );
        };

        if series.cosine.len() != series.harmonic_order
            || series.sine.len() != series.harmonic_order
        {
            return Err("business-time clock coefficient lengths must match harmonic orders");
        }

        if !series
            .cosine
            .iter()
            .chain(series.sine.iter())
            .all(|coefficient| coefficient.is_finite())
        {
            return Err("business-time clock coefficients must be finite");
        }
    }

    Ok(())
}

fn harmonic_log_rate_upper_bound(
    coefficients: coordinates::FourierCoefficientSlices<'_>,
) -> Option<f64> {
    if !coefficients.intercept.is_finite() {
        return None;
    }
    if coefficients.harmonic_series.len() != FOURIER_BUSINESS_TIME_CLOCK_AXES.len() {
        return None;
    }

    let mut log_rate_bound = coefficients.intercept;

    for axis in FOURIER_BUSINESS_TIME_CLOCK_AXES {
        let series = coefficients.series_for_axis(axis)?;
        if series.cosine.len() != series.harmonic_order
            || series.sine.len() != series.harmonic_order
        {
            return None;
        }

        for (cosine_coefficient, sine_coefficient) in series.cosine.iter().zip(series.sine.iter()) {
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
    }

    Some(log_rate_bound)
}
