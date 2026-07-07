use std::collections::BTreeMap;

use chrono::{NaiveDate, TimeZone};
use chrono_tz::America::New_York;
use janiform_foundations::time::MINUTES_PER_DAY;

use super::{
    FOURIER_BUSINESS_TIME_CLOCK_AXES, FOURIER_BUSINESS_TIME_CLOCK_BASIS,
    FOURIER_BUSINESS_TIME_CLOCK_COORDINATE_SYSTEM,
    FOURIER_BUSINESS_TIME_CLOCK_MODEL_SCHEMA_VERSION,
    FOURIER_BUSINESS_TIME_CLOCK_NORMALIZATION_POLICY, FOURIER_BUSINESS_TIME_CLOCK_SELECTION_POLICY,
    FourierBusinessTimeClockAxis, FourierBusinessTimeClockHarmonicSeries,
    FourierBusinessTimeClockModel, FourierBusinessTimeClockOrderedIntervalExposureInput,
    FourierBusinessTimeClockPhases,
    integrate_fourier_business_time_clock_ordered_interval_exposures,
    zero_order_harmonic_series_by_axis,
};
use crate::calendar::{
    CalendarRuleMetadataArtifact, ExchangeCalendarArtifact, ExchangeCalendarDefinition,
    RegularSessionArtifact, SessionWindowArtifact, TradingWeekday,
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

fn xnys_monday_calendar_definition() -> ExchangeCalendarDefinition {
    ExchangeCalendarDefinition::from_artifact(ExchangeCalendarArtifact {
        schema_version: "calendar.v1".to_string(),
        exchange_code: "XNYS".to_string(),
        exchange_name: "New York Stock Exchange".to_string(),
        timezone_name: Some("America/New_York".to_string()),
        rule_metadata: CalendarRuleMetadataArtifact {
            holiday_calendar_code: "US_EQUITIES".to_string(),
            early_close_rule_set: Some("US_EQUITIES_EARLY_CLOSE".to_string()),
        },
        regular_sessions: vec![RegularSessionArtifact {
            weekday: TradingWeekday::Monday,
            windows: vec![SessionWindowArtifact {
                label: "regular".to_string(),
                open_time: "09:30".to_string(),
                close_time: "16:00".to_string(),
            }],
        }],
        holiday_dates: Vec::new(),
        early_close_sessions: Vec::new(),
    })
    .expect("calendar fixture should validate")
}

fn new_york_local_timestamp_nanos(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> i64 {
    New_York
        .with_ymd_and_hms(year, month, day, hour, minute, second)
        .single()
        .expect("fixture local timestamp should resolve uniquely")
        .timestamp_nanos_opt()
        .expect("fixture timestamp should be inside nanosecond range")
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
fn ordered_interval_exposures_integrate_flat_clock_over_open_segments() {
    let calendar_definition = xnys_monday_calendar_definition();
    let model = flat_clock_model();

    let interval_start_utc_nanos = vec![
        new_york_local_timestamp_nanos(2026, 2, 2, 9, 30, 0),
        new_york_local_timestamp_nanos(2026, 2, 2, 9, 30, 30),
        new_york_local_timestamp_nanos(2026, 2, 2, 16, 0, 0),
    ];

    let interval_end_utc_nanos = vec![
        new_york_local_timestamp_nanos(2026, 2, 2, 9, 30, 30),
        new_york_local_timestamp_nanos(2026, 2, 2, 9, 31, 30),
        new_york_local_timestamp_nanos(2026, 2, 2, 16, 30, 0),
    ];

    let output = integrate_fourier_business_time_clock_ordered_interval_exposures(
        FourierBusinessTimeClockOrderedIntervalExposureInput {
            calendar_definition: &calendar_definition,
            timezone: New_York,
            model: &model,
            interval_start_utc_nanos: interval_start_utc_nanos.as_slice(),
            interval_end_utc_nanos: interval_end_utc_nanos.as_slice(),
        },
    )
    .expect("flat-clock ordered interval exposure should integrate");

    assert_eq!(output.business_time_exposure_seconds, vec![30.0, 60.0, 0.0]);
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

#[test]
fn validate_rejects_log_rate_envelope_outside_representable_positive_domain() {
    let mut model = flat_clock_model();

    model.intercept = f64::MAX.ln();

    set_harmonic_series(
        &mut model,
        FourierBusinessTimeClockAxis::Intraday,
        1,
        vec![f64::MAX.ln()],
        vec![0.0],
    );

    assert_eq!(
        model.validate(),
        Err(
            "business-time clock normalized log-rate envelope exceeds the representable positive f64 domain"
                .to_string()
        )
    );
}
