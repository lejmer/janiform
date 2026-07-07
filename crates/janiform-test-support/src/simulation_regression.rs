use chrono::{DateTime, Utc};
pub use janiform_market_time::time::NANOS_PER_HOUR;

pub fn count_timestamps_in_half_open_interval(
    sorted_timestamps: &[i64],
    start_inclusive: i64,
    end_exclusive: i64,
) -> usize {
    sorted_timestamps
        .iter()
        .filter(|timestamp_utc_nanos| {
            **timestamp_utc_nanos >= start_inclusive && **timestamp_utc_nanos < end_exclusive
        })
        .count()
}

pub fn interval_delta_hours_to_nanos(interval_delta_hours: f64) -> i64 {
    (interval_delta_hours * NANOS_PER_HOUR as f64).round() as i64
}

pub fn population_standard_deviation(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }

    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance = values
        .iter()
        .map(|value| {
            let centered = value - mean;
            centered * centered
        })
        .sum::<f64>()
        / values.len() as f64;

    variance.sqrt()
}

pub fn timestamp_utc_nanos(raw_timestamp_utc: &str) -> i64 {
    DateTime::parse_from_rfc3339(raw_timestamp_utc)
        .expect("fixture timestamp must be RFC3339")
        .with_timezone(&Utc)
        .timestamp_nanos_opt()
        .expect("fixture timestamp must be in nanosecond range")
}

#[cfg(test)]
mod tests {
    use super::{
        NANOS_PER_HOUR, count_timestamps_in_half_open_interval, interval_delta_hours_to_nanos,
        population_standard_deviation, timestamp_utc_nanos,
    };

    #[test]
    fn interval_delta_hours_to_nanos_matches_one_hour_constant() {
        assert_eq!(interval_delta_hours_to_nanos(1.0), NANOS_PER_HOUR);
        assert_eq!(interval_delta_hours_to_nanos(0.25), NANOS_PER_HOUR / 4);
    }

    #[test]
    fn population_standard_deviation_matches_expected_value() {
        assert!(
            (population_standard_deviation(&[1.0, 2.0, 3.0, 4.0]) - 1.118_033_988_749_895).abs()
                < 1.0e-12
        );
    }

    #[test]
    fn timestamp_window_count_matches_half_open_membership() {
        assert_eq!(
            count_timestamps_in_half_open_interval(&[1, 2, 2, 3, 4], 2, 4),
            3
        );
    }

    #[test]
    fn timestamp_utc_nanos_parses_rfc3339_values() {
        assert_eq!(
            timestamp_utc_nanos("2024-01-22T20:00:00Z"),
            1_705_953_600_000_000_000,
        );
    }
}
