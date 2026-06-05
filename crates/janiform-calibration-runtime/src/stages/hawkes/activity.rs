pub(crate) fn short_gap_ratio_from_threshold(gap_seconds: &[f64], threshold_seconds: f64) -> f64 {
    if gap_seconds.is_empty() {
        return 0.0;
    }

    gap_seconds
        .iter()
        .filter(|duration_seconds| **duration_seconds <= threshold_seconds)
        .count() as f64
        / gap_seconds.len() as f64
}
