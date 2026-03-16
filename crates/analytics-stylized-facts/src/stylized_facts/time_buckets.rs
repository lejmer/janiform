//! Shared timestamp-window and interval-bucketing helpers for verification modules.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TimestampAnalysisWindow {
    pub(crate) start_utc_nanos: i64,
    pub(crate) end_utc_nanos: i64,
}

impl TimestampAnalysisWindow {
    pub(crate) const fn new(start_utc_nanos: i64, end_utc_nanos: i64) -> Self {
        Self {
            start_utc_nanos,
            end_utc_nanos,
        }
    }

    pub(crate) fn contains_timestamp(self, timestamp_utc_nanos: i64) -> bool {
        timestamp_utc_nanos >= self.start_utc_nanos && timestamp_utc_nanos < self.end_utc_nanos
    }
}

pub(crate) fn interval_count_for_window(
    analysis_window: TimestampAnalysisWindow,
    interval_duration_nanos: i64,
) -> Option<usize> {
    if interval_duration_nanos <= 0 {
        return None;
    }
    if analysis_window.start_utc_nanos >= analysis_window.end_utc_nanos {
        return None;
    }

    let window_span =
        i128::from(analysis_window.end_utc_nanos) - i128::from(analysis_window.start_utc_nanos);
    let interval_duration = i128::from(interval_duration_nanos);
    let interval_count = (window_span + interval_duration - 1) / interval_duration;

    usize::try_from(interval_count).ok()
}

pub(crate) fn interval_position_for_timestamp(
    timestamp_utc_nanos: i64,
    analysis_window: TimestampAnalysisWindow,
    interval_duration_nanos: i64,
    interval_count: usize,
) -> Option<usize> {
    if interval_duration_nanos <= 0 || interval_count == 0 {
        return None;
    }
    if !analysis_window.contains_timestamp(timestamp_utc_nanos) {
        return None;
    }

    let offset_nanos =
        i128::from(timestamp_utc_nanos).saturating_sub(i128::from(analysis_window.start_utc_nanos));
    let interval_duration = i128::from(interval_duration_nanos);
    let interval_position = usize::try_from(offset_nanos / interval_duration).ok()?;

    if interval_position < interval_count {
        Some(interval_position)
    } else {
        None
    }
}
