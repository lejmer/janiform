use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate};
use janiform_foundations::time::{
    DAYS_PER_COMMON_YEAR, DAYS_PER_LEAP_YEAR, DAYS_PER_WEEK, MINUTES_PER_DAY,
};
use serde::{Deserialize, Serialize};

const FOURIER_BUSINESS_TIME_CLOCK_AXIS_COUNT: usize = 3;

/// Canonical recurring axes used by the Fourier business-time clock.
pub const FOURIER_BUSINESS_TIME_CLOCK_AXES: [FourierBusinessTimeClockAxis; 3] = [
    FourierBusinessTimeClockAxis::Intraday,
    FourierBusinessTimeClockAxis::Weekly,
    FourierBusinessTimeClockAxis::Annual,
];

/// Recurring coordinate axis used by the Fourier business-time clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FourierBusinessTimeClockAxis {
    /// Exchange-local minute-of-day phase.
    Intraday,
    /// Exchange-local weekday phase anchored to Monday.
    Weekly,
    /// Civil year minute phase.
    Annual,
}

impl FourierBusinessTimeClockAxis {
    /// Stable lowercase axis label used in diagnostics and serialized maps.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Intraday => "intraday",
            Self::Weekly => "weekly",
            Self::Annual => "annual",
        }
    }

    /// Human-readable coordinate support name used in validation errors.
    pub const fn coordinate_name(self) -> &'static str {
        match self {
            Self::Intraday => "minute-of-day",
            Self::Weekly => "weekday",
            Self::Annual => "civil-year minute",
        }
    }

    /// Settings path used when reporting invalid harmonic-order settings.
    pub const fn maximum_harmonic_order_setting_name(self) -> &'static str {
        match self {
            Self::Intraday => "maximum_harmonic_orders.intraday",
            Self::Weekly => "maximum_harmonic_orders.weekly",
            Self::Annual => "maximum_harmonic_orders.annual",
        }
    }

    /// Nyquist harmonic limit for axes with discrete support in the fit design.
    pub const fn nyquist_harmonic_limit(self) -> Option<usize> {
        match self {
            Self::Intraday => Some(MINUTES_PER_DAY / 2),
            Self::Weekly => Some(DAYS_PER_WEEK / 2),
            Self::Annual => None,
        }
    }

    const fn position(self) -> usize {
        match self {
            Self::Intraday => 0,
            Self::Weekly => 1,
            Self::Annual => 2,
        }
    }

    fn phase_for_local_minute(self, local_date: NaiveDate, minute_of_day: usize) -> Option<f64> {
        match self {
            Self::Intraday => intraday_phase(minute_of_day),
            Self::Weekly => weekly_phase(local_date),
            Self::Annual => annual_phase(local_date, minute_of_day),
        }
    }
}

/// Fourier harmonic series for one recurring business-time clock axis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FourierBusinessTimeClockHarmonicSeries {
    /// Number of cosine/sine harmonic pairs in this axis series.
    pub harmonic_order: usize,
    /// Cosine coefficients ordered by harmonic number.
    pub cosine_coefficients: Vec<f64>,
    /// Sine coefficients ordered by harmonic number.
    pub sine_coefficients: Vec<f64>,
}

impl FourierBusinessTimeClockHarmonicSeries {
    /// Construct a harmonic series with no cosine or sine terms.
    pub fn zero_order() -> Self {
        Self {
            harmonic_order: 0,
            cosine_coefficients: Vec::new(),
            sine_coefficients: Vec::new(),
        }
    }

    pub(super) fn slices(&self) -> HarmonicSeriesSlices<'_> {
        HarmonicSeriesSlices {
            harmonic_order: self.harmonic_order,
            cosine: self.cosine_coefficients.as_slice(),
            sine: self.sine_coefficients.as_slice(),
        }
    }
}

/// Fourier harmonic series keyed by recurring clock axis.
pub type FourierBusinessTimeClockHarmonicSeriesByAxis =
    BTreeMap<FourierBusinessTimeClockAxis, FourierBusinessTimeClockHarmonicSeries>;

/// Construct zero-order harmonic series for every canonical recurring axis.
pub fn zero_order_harmonic_series_by_axis() -> FourierBusinessTimeClockHarmonicSeriesByAxis {
    FOURIER_BUSINESS_TIME_CLOCK_AXES
        .into_iter()
        .map(|axis| (axis, FourierBusinessTimeClockHarmonicSeries::zero_order()))
        .collect()
}

/// Recurring calendar phases used by the Fourier business-time clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FourierBusinessTimeClockPhases {
    phases: [f64; FOURIER_BUSINESS_TIME_CLOCK_AXIS_COUNT],
}

impl FourierBusinessTimeClockPhases {
    /// Construct phases from a map containing every canonical recurring axis.
    pub fn try_from_axis_phase_map(
        phase_by_axis: &BTreeMap<FourierBusinessTimeClockAxis, f64>,
    ) -> Option<Self> {
        if phase_by_axis.len() != FOURIER_BUSINESS_TIME_CLOCK_AXES.len() {
            return None;
        }

        let mut phases = [0.0; FOURIER_BUSINESS_TIME_CLOCK_AXIS_COUNT];

        for axis in FOURIER_BUSINESS_TIME_CLOCK_AXES {
            let phase = *phase_by_axis.get(&axis)?;
            if !phase.is_finite() {
                return None;
            }

            phases[axis.position()] = phase;
        }

        Some(Self { phases })
    }

    /// Return the phase for a recurring clock axis.
    pub fn phase_for_axis(self, axis: FourierBusinessTimeClockAxis) -> f64 {
        self.phases[axis.position()]
    }

    pub(super) fn is_finite(self) -> bool {
        FOURIER_BUSINESS_TIME_CLOCK_AXES
            .into_iter()
            .all(|axis| self.phase_for_axis(axis).is_finite())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FourierHarmonicOrders {
    orders: [usize; FOURIER_BUSINESS_TIME_CLOCK_AXIS_COUNT],
}

impl FourierHarmonicOrders {
    pub(super) fn zeros() -> Self {
        Self {
            orders: [0; FOURIER_BUSINESS_TIME_CLOCK_AXIS_COUNT],
        }
    }

    pub(super) fn from_order_map(
        order_by_axis: &BTreeMap<FourierBusinessTimeClockAxis, usize>,
    ) -> Option<Self> {
        let mut orders = [0; FOURIER_BUSINESS_TIME_CLOCK_AXIS_COUNT];

        for axis in FOURIER_BUSINESS_TIME_CLOCK_AXES {
            orders[axis.position()] = *order_by_axis.get(&axis)?;
        }

        Some(Self { orders })
    }

    pub(super) fn order_for_axis(self, axis: FourierBusinessTimeClockAxis) -> usize {
        self.orders[axis.position()]
    }

    pub(super) fn set_order_for_axis(
        &mut self,
        axis: FourierBusinessTimeClockAxis,
        harmonic_order: usize,
    ) {
        self.orders[axis.position()] = harmonic_order;
    }

    pub(super) fn axis_orders(self) -> impl Iterator<Item = (FourierBusinessTimeClockAxis, usize)> {
        FOURIER_BUSINESS_TIME_CLOCK_AXES
            .into_iter()
            .map(move |axis| (axis, self.order_for_axis(axis)))
    }

    pub(super) fn parameter_count(self) -> Option<usize> {
        let mut harmonic_count = 0_usize;

        for (_, harmonic_order) in self.axis_orders() {
            harmonic_count = harmonic_count.checked_add(harmonic_order)?;
        }

        1_usize.checked_add(2_usize.checked_mul(harmonic_count)?)
    }
}

pub(super) struct FourierCoefficientSlices<'a> {
    pub(super) intercept: f64,
    pub(super) harmonic_series: &'a FourierBusinessTimeClockHarmonicSeriesByAxis,
}

impl FourierCoefficientSlices<'_> {
    pub(super) fn series_for_axis(
        &self,
        axis: FourierBusinessTimeClockAxis,
    ) -> Option<HarmonicSeriesSlices<'_>> {
        Some(self.harmonic_series.get(&axis)?.slices())
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct HarmonicSeriesSlices<'a> {
    pub(super) harmonic_order: usize,
    pub(super) cosine: &'a [f64],
    pub(super) sine: &'a [f64],
}

pub(super) fn phases_for_local_minute(
    local_date: NaiveDate,
    minute_of_day: usize,
) -> Option<FourierBusinessTimeClockPhases> {
    let mut phases = [0.0; FOURIER_BUSINESS_TIME_CLOCK_AXIS_COUNT];

    for axis in FOURIER_BUSINESS_TIME_CLOCK_AXES {
        phases[axis.position()] = axis.phase_for_local_minute(local_date, minute_of_day)?;
    }

    Some(FourierBusinessTimeClockPhases { phases })
}

pub(super) fn fourier_log_rate(
    coefficients: FourierCoefficientSlices<'_>,
    phases: FourierBusinessTimeClockPhases,
) -> Option<f64> {
    if !coefficients.intercept.is_finite() || !phases.is_finite() {
        return None;
    }

    if coefficients.harmonic_series.len() != FOURIER_BUSINESS_TIME_CLOCK_AXES.len() {
        return None;
    }

    let mut log_rate = coefficients.intercept;

    for axis in FOURIER_BUSINESS_TIME_CLOCK_AXES {
        let series = coefficients.series_for_axis(axis)?;
        if series.cosine.len() != series.harmonic_order
            || series.sine.len() != series.harmonic_order
        {
            return None;
        }

        log_rate = add_harmonic_log_component(log_rate, phases.phase_for_axis(axis), series)?;
    }

    log_rate.is_finite().then_some(log_rate)
}

pub(super) fn feature_vector(
    orders: FourierHarmonicOrders,
    phases: FourierBusinessTimeClockPhases,
) -> Option<Vec<f64>> {
    if !phases.is_finite() {
        return None;
    }

    let mut features = Vec::with_capacity(orders.parameter_count()?);

    features.push(1.0);

    for axis in FOURIER_BUSINESS_TIME_CLOCK_AXES {
        append_axis_features(
            &mut features,
            orders.order_for_axis(axis),
            phases.phase_for_axis(axis),
        );
    }

    Some(features)
}

pub(super) fn split_coefficients_by_axis(
    coefficients: &[f64],
    orders: FourierHarmonicOrders,
) -> Option<FourierBusinessTimeClockHarmonicSeriesByAxis> {
    if coefficients.len() != orders.parameter_count()? {
        return None;
    }

    let mut next_position = 1_usize;
    let mut harmonic_series = BTreeMap::new();

    for (axis, harmonic_order) in orders.axis_orders() {
        let (cosine_coefficients, sine_coefficients) =
            split_harmonic_coefficients(coefficients, &mut next_position, harmonic_order)?;

        harmonic_series.insert(
            axis,
            FourierBusinessTimeClockHarmonicSeries {
                harmonic_order,
                cosine_coefficients,
                sine_coefficients,
            },
        );
    }

    if next_position != coefficients.len() {
        return None;
    }

    Some(harmonic_series)
}

fn intraday_phase(minute_of_day: usize) -> Option<f64> {
    if minute_of_day >= MINUTES_PER_DAY {
        return None;
    }

    Some(minute_of_day as f64 / MINUTES_PER_DAY as f64)
}

fn weekly_phase(local_date: NaiveDate) -> Option<f64> {
    let weekday_position = usize::try_from(local_date.weekday().num_days_from_monday()).ok()?;

    Some(weekday_position as f64 / DAYS_PER_WEEK as f64)
}

fn annual_phase(local_date: NaiveDate, minute_of_day: usize) -> Option<f64> {
    if minute_of_day >= MINUTES_PER_DAY {
        return None;
    }

    let ordinal_position = usize::try_from(local_date.ordinal0()).ok()?;

    let minute_of_year = ordinal_position
        .checked_mul(MINUTES_PER_DAY)?
        .checked_add(minute_of_day)?;

    let year_minutes = civil_year_minute_count(local_date.year())?;

    Some(minute_of_year as f64 / year_minutes as f64)
}

fn civil_year_minute_count(year: i32) -> Option<usize> {
    let day_count = if is_gregorian_leap_year(year) {
        DAYS_PER_LEAP_YEAR
    } else {
        DAYS_PER_COMMON_YEAR
    };

    day_count.checked_mul(MINUTES_PER_DAY)
}

fn add_harmonic_log_component(
    mut log_rate: f64,
    phase: f64,
    series: HarmonicSeriesSlices<'_>,
) -> Option<f64> {
    if !phase.is_finite() {
        return None;
    }

    for (harmonic_position, (cosine_coefficient, sine_coefficient)) in
        series.cosine.iter().zip(series.sine.iter()).enumerate()
    {
        if !cosine_coefficient.is_finite() || !sine_coefficient.is_finite() {
            return None;
        }

        let harmonic_order = harmonic_position + 1;
        let angle = std::f64::consts::TAU * harmonic_order as f64 * phase;

        log_rate += cosine_coefficient * angle.cos() + sine_coefficient * angle.sin();
    }

    log_rate.is_finite().then_some(log_rate)
}

fn append_axis_features(features: &mut Vec<f64>, harmonic_order: usize, phase: f64) {
    for harmonic_order in 1..=harmonic_order {
        let angle = std::f64::consts::TAU * harmonic_order as f64 * phase;

        features.push(angle.cos());
        features.push(angle.sin());
    }
}

fn split_harmonic_coefficients(
    coefficients: &[f64],
    next_position: &mut usize,
    harmonic_order: usize,
) -> Option<(Vec<f64>, Vec<f64>)> {
    let mut cosine_coefficients = Vec::with_capacity(harmonic_order);
    let mut sine_coefficients = Vec::with_capacity(harmonic_order);

    for _ in 0..harmonic_order {
        cosine_coefficients.push(*coefficients.get(*next_position)?);
        *next_position = next_position.checked_add(1)?;

        sine_coefficients.push(*coefficients.get(*next_position)?);
        *next_position = next_position.checked_add(1)?;
    }

    Some((cosine_coefficients, sine_coefficients))
}

fn is_gregorian_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}
