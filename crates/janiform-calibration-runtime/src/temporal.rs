use chrono::{DateTime, Utc};

pub(crate) fn datetime_from_nanos<E>(
    timestamp_utc_nanos: i64,
    error: E,
) -> Result<DateTime<Utc>, E> {
    let seconds = timestamp_utc_nanos.div_euclid(1_000_000_000);
    let nanoseconds = timestamp_utc_nanos.rem_euclid(1_000_000_000) as u32;

    DateTime::from_timestamp(seconds, nanoseconds).ok_or(error)
}

pub(crate) fn count_timestamps_in_half_open_interval(
    sorted_timestamps: &[i64],
    start_inclusive: i64,
    end_exclusive: i64,
) -> usize {
    let start_index = sorted_timestamps.partition_point(|timestamp| *timestamp < start_inclusive);
    let end_index = sorted_timestamps.partition_point(|timestamp| *timestamp < end_exclusive);

    end_index.saturating_sub(start_index)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::{count_timestamps_in_half_open_interval, datetime_from_nanos};

    #[test]
    fn datetime_from_nanos_reconstructs_utc_timestamp() {
        let timestamp_utc_nanos = chrono_tz::UTC
            .with_ymd_and_hms(2026, 2, 3, 12, 34, 56)
            .single()
            .expect("fixture timestamp should be valid")
            .timestamp_nanos_opt()
            .expect("fixture timestamp should fit in nanos");

        let reconstructed = datetime_from_nanos(timestamp_utc_nanos, ())
            .expect("timestamp nanos should reconstruct");

        assert_eq!(
            reconstructed.timestamp_nanos_opt(),
            Some(timestamp_utc_nanos)
        );
    }

    #[test]
    fn count_timestamps_in_half_open_interval_counts_expected_members() {
        let sorted_timestamps = [10_i64, 20, 20, 30, 40];

        assert_eq!(
            count_timestamps_in_half_open_interval(&sorted_timestamps, 20, 40),
            3
        );
    }
}
