use super::constants::INTRADAY_PRICING_HOURS_PER_YEAR;

pub(crate) fn intraday_year_fraction(interval_hours: f64) -> f64 {
    interval_hours / INTRADAY_PRICING_HOURS_PER_YEAR
}
