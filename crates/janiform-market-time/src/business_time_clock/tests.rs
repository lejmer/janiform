use std::collections::BTreeMap;

use chrono::NaiveDate;
use janiform_foundations::time::MINUTES_PER_DAY;

use super::{
    FOURIER_BUSINESS_TIME_CLOCK_AXES, FOURIER_BUSINESS_TIME_CLOCK_BASIS,
    FOURIER_BUSINESS_TIME_CLOCK_COORDINATE_SYSTEM,
    FOURIER_BUSINESS_TIME_CLOCK_MODEL_SCHEMA_VERSION,
    FOURIER_BUSINESS_TIME_CLOCK_NORMALIZATION_POLICY, FOURIER_BUSINESS_TIME_CLOCK_SELECTION_POLICY,
    FourierBusinessTimeClockAxis, FourierBusinessTimeClockHarmonicSeries,
    FourierBusinessTimeClockModel, FourierBusinessTimeClockPhases,
    zero_order_harmonic_series_by_axis,
};

fn flat_clock_model() -> FourierBusinessTimeClockModel {
    FourierBusinessTimeClockModel {
        schema_version: FOURIER_BUSINESS_TIME_CLOCK_MODEL_SCHEMA_VERSION.to_string(),
        coordinate_system: FOURIER_BUSINESS_TIME_CLOCK_COORDINATE_SYSTEM.to_string(),
        coefficient_basis: FOURIER_BUSINESS_TIME_CLOCK_BASIS.to_string(),
        normalization_policy: FOURIER_BUSINESS_TIME_CLOCK_NORMALIZATION_POLICY.to_string(),
        selection_policy: FOURIER_BUSINESS_TIME_CLOCK_SELECTION_POLICY.to_string(),
        intercept: 0.0,
        harmonic_series: zero_order_harmonic_series_by_axis(),
        selected_objective_value: 1.0,
        selected_negative_log_likelihood: 1.0,
        evaluated_candidate_count: 1,
        fit_open_minute_count: 1,
        fit_open_event_count: 1,
        fit_open_exposure_seconds: 60.0,
    }
}

fn set_harmonic_series(
    model: &mut FourierBusinessTimeClockModel,
    axis: FourierBusinessTimeClockAxis,
    harmonic_order: usize,
    cosine_coefficients: Vec<f64>,
    sine_coefficients: Vec<f64>,
) {
    model.harmonic_series.insert(
        axis,
        FourierBusinessTimeClockHarmonicSeries {
            harmonic_order,
            cosine_coefficients,
            sine_coefficients,
        },
    );
}

#[test]
fn rate_at_phases_accepts_axis_keyed_phase_map() {
    let mut phase_by_axis = BTreeMap::new();

    for axis in FOURIER_BUSINESS_TIME_CLOCK_AXES {
        phase_by_axis.insert(axis, 0.0);
    }

    let phases = FourierBusinessTimeClockPhases::try_from_axis_phase_map(&phase_by_axis)
        .expect("complete finite phase map");

    assert_eq!(flat_clock_model().rate_at_phases(phases), Some(1.0));
}

#[test]
fn axis_keyed_phase_map_requires_every_clock_axis() {
    let mut phase_by_axis = BTreeMap::new();
    phase_by_axis.insert(FourierBusinessTimeClockAxis::Intraday, 0.0);

    assert_eq!(
        FourierBusinessTimeClockPhases::try_from_axis_phase_map(&phase_by_axis),
        None
    );
}

#[test]
fn intraday_component_repeats_across_weekdays() {
    let mut model = flat_clock_model();

    set_harmonic_series(
        &mut model,
        FourierBusinessTimeClockAxis::Intraday,
        1,
        vec![1.0],
        vec![0.0],
    );

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

    set_harmonic_series(
        &mut annual_model,
        FourierBusinessTimeClockAxis::Annual,
        1,
        vec![1.0],
        vec![0.0],
    );

    let annual_bound = annual_model
        .rate_upper_bound()
        .expect("annual model should admit a rate bound");

    assert!(annual_bound > flat_bound);
}

#[test]
fn rate_upper_bound_rejects_mismatched_coefficient_vectors() {
    let mut model = flat_clock_model();

    set_harmonic_series(
        &mut model,
        FourierBusinessTimeClockAxis::Intraday,
        1,
        vec![0.0],
        Vec::new(),
    );

    assert_eq!(model.rate_upper_bound(), None);
}

#[test]
fn rate_upper_bound_saturates_overflowing_log_bound_to_finite_maximum() {
    let mut model = flat_clock_model();

    set_harmonic_series(
        &mut model,
        FourierBusinessTimeClockAxis::Intraday,
        1,
        vec![f64::MAX],
        vec![0.0],
    );

    assert_eq!(model.rate_upper_bound(), Some(f64::MAX));
}
