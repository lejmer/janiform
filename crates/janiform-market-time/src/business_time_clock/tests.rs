use chrono::NaiveDate;
use janiform_foundations::time::MINUTES_PER_DAY;

use super::{
    FOURIER_BUSINESS_TIME_CLOCK_BASIS, FOURIER_BUSINESS_TIME_CLOCK_COORDINATE_SYSTEM,
    FOURIER_BUSINESS_TIME_CLOCK_MODEL_SCHEMA_VERSION,
    FOURIER_BUSINESS_TIME_CLOCK_NORMALIZATION_POLICY, FOURIER_BUSINESS_TIME_CLOCK_SELECTION_POLICY,
    FourierBusinessTimeClockModel,
};

fn flat_clock_model() -> FourierBusinessTimeClockModel {
    FourierBusinessTimeClockModel {
        schema_version: FOURIER_BUSINESS_TIME_CLOCK_MODEL_SCHEMA_VERSION.to_string(),
        coordinate_system: FOURIER_BUSINESS_TIME_CLOCK_COORDINATE_SYSTEM.to_string(),
        coefficient_basis: FOURIER_BUSINESS_TIME_CLOCK_BASIS.to_string(),
        normalization_policy: FOURIER_BUSINESS_TIME_CLOCK_NORMALIZATION_POLICY.to_string(),
        selection_policy: FOURIER_BUSINESS_TIME_CLOCK_SELECTION_POLICY.to_string(),
        intraday_harmonic_order: 0,
        weekly_harmonic_order: 0,
        annual_harmonic_order: 0,
        intercept: 0.0,
        intraday_cosine_coefficients: Vec::new(),
        intraday_sine_coefficients: Vec::new(),
        weekly_cosine_coefficients: Vec::new(),
        weekly_sine_coefficients: Vec::new(),
        annual_cosine_coefficients: Vec::new(),
        annual_sine_coefficients: Vec::new(),
        selected_objective_value: 1.0,
        selected_negative_log_likelihood: 1.0,
        evaluated_candidate_count: 1,
        fit_open_minute_count: 1,
        fit_open_event_count: 1,
        fit_open_exposure_seconds: 60.0,
    }
}

#[test]
fn intraday_component_repeats_across_weekdays() {
    let mut model = flat_clock_model();

    model.intraday_harmonic_order = 1;
    model.intraday_cosine_coefficients = vec![1.0];
    model.intraday_sine_coefficients = vec![0.0];

    let monday = NaiveDate::from_ymd_opt(2026, 2, 2).expect("valid fixture date");
    let tuesday = NaiveDate::from_ymd_opt(2026, 2, 3).expect("valid fixture date");

    let monday_midnight = model
        .rate_at_local_minute(monday, 0)
        .expect("valid intraday rate");

    let tuesday_midnight = model
        .rate_at_local_minute(tuesday, 0)
        .expect("valid intraday rate");

    let monday_noon = model
        .rate_at_local_minute(monday, MINUTES_PER_DAY / 2)
        .expect("valid intraday rate");

    assert_eq!(monday_midnight, tuesday_midnight);
    assert!(monday_midnight > monday_noon);
}

#[test]
fn rate_upper_bound_includes_annual_harmonics() {
    let flat_bound = flat_clock_model()
        .rate_upper_bound()
        .expect("flat model should admit a rate bound");

    let mut annual_model = flat_clock_model();

    annual_model.annual_harmonic_order = 1;
    annual_model.annual_cosine_coefficients = vec![1.0];
    annual_model.annual_sine_coefficients = vec![0.0];

    let annual_bound = annual_model
        .rate_upper_bound()
        .expect("annual model should admit a rate bound");

    assert!(annual_bound > flat_bound);
}

#[test]
fn rate_upper_bound_rejects_mismatched_coefficient_vectors() {
    let mut model = flat_clock_model();

    model.intraday_harmonic_order = 1;
    model.intraday_cosine_coefficients = vec![0.0];

    assert_eq!(model.rate_upper_bound(), None);
}

#[test]
fn rate_upper_bound_saturates_overflowing_log_bound_to_finite_maximum() {
    let mut model = flat_clock_model();

    model.intraday_harmonic_order = 1;
    model.intraday_cosine_coefficients = vec![f64::MAX];
    model.intraday_sine_coefficients = vec![0.0];

    assert_eq!(model.rate_upper_bound(), Some(f64::MAX));
}
