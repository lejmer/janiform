pub(crate) fn asset_reference_log_price_from_observed_spot(
    spot: f64,
    observed_log_price: f64,
) -> Option<f64> {
    if !spot.is_finite() || spot <= 0.0 || !observed_log_price.is_finite() {
        return None;
    }

    let asset_reference_log_price = spot.ln() - observed_log_price;
    asset_reference_log_price
        .is_finite()
        .then_some(asset_reference_log_price)
}
