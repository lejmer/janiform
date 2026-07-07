use chrono::{DateTime, Utc};
use janiform_foundations::time::utc_datetime_from_nanos;

pub(crate) fn datetime_from_nanos<E>(
    timestamp_utc_nanos: i64,
    error: E,
) -> Result<DateTime<Utc>, E> {
    utc_datetime_from_nanos(timestamp_utc_nanos).ok_or(error)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::datetime_from_nanos;

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
}
