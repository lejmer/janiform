use chrono::{DateTime, Utc};

use crate::constants::{NANOS_PER_HOUR, ONE_SECOND_NANOS};

pub(crate) fn derive_elapsed_hours(
    previous_timestamp_utc_nanos: i64,
    timestamp_utc_nanos: i64,
) -> f64 {
    (timestamp_utc_nanos - previous_timestamp_utc_nanos) as f64 / NANOS_PER_HOUR as f64
}

pub(crate) fn timestamp_utc_from_nanos(timestamp_utc_nanos: i64) -> Option<DateTime<Utc>> {
    let seconds = timestamp_utc_nanos.div_euclid(ONE_SECOND_NANOS);
    let nanoseconds = timestamp_utc_nanos.rem_euclid(ONE_SECOND_NANOS) as u32;

    DateTime::<Utc>::from_timestamp(seconds, nanoseconds)
}
