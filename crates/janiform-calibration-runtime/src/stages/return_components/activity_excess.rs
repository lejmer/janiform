pub(crate) fn positive_activity_excess(post_shock_activity_lift: f64) -> f64 {
    if !post_shock_activity_lift.is_finite() {
        return 0.0;
    }

    (post_shock_activity_lift - 1.0).max(0.0)
}
