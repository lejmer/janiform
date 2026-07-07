use super::constants::INTRADAY_PRICING_HOURS_PER_YEAR;

/// Convert Janiform intraday pricing hours to continuous-rate year fraction.
pub fn intraday_year_fraction(interval_hours: f64) -> f64 {
    interval_hours / INTRADAY_PRICING_HOURS_PER_YEAR
}
