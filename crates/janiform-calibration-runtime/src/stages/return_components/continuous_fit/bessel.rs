use std::f64::consts::PI;

use puruspe::Inu_Knu;

// DLMF 10.40.2 large-argument K_nu expansion through O(x^-3). This is only
// used after the direct library value underflows, so x is already in the
// asymptotic regime for the NIG calibration cases it protects.
const MODIFIED_BESSEL_K_ASYMPTOTIC_CORRECTION_TERM_COUNT: usize = 3;

pub(super) fn log_modified_bessel_k1(argument: f64) -> Option<f64> {
    if !argument.is_finite() || argument <= 0.0 {
        return None;
    }

    let (_, k1) = Inu_Knu(1.0, argument);
    if k1.is_finite() && k1 > 0.0 {
        return Some(k1.ln());
    }

    log_modified_bessel_k_large_argument(1.0, argument)
}

pub(super) fn modified_bessel_k0_to_k1_ratio(argument: f64) -> Option<f64> {
    if !argument.is_finite() || argument <= 0.0 {
        return None;
    }

    let (_, k0) = Inu_Knu(0.0, argument);
    let (_, k1) = Inu_Knu(1.0, argument);
    if k0.is_finite() && k0 > 0.0 && k1.is_finite() && k1 > 0.0 {
        let ratio = k0 / k1;
        return ratio.is_finite().then_some(ratio);
    }

    let log_ratio = log_modified_bessel_k_large_argument(0.0, argument)?
        - log_modified_bessel_k_large_argument(1.0, argument)?;
    let ratio = log_ratio.exp();

    ratio.is_finite().then_some(ratio)
}

fn log_modified_bessel_k_large_argument(order: f64, argument: f64) -> Option<f64> {
    let correction = modified_bessel_k_large_argument_correction(
        order,
        argument,
        MODIFIED_BESSEL_K_ASYMPTOTIC_CORRECTION_TERM_COUNT,
    )?;

    Some(-argument + 0.5 * (PI / (2.0 * argument)).ln() + correction.ln())
}

fn modified_bessel_k_large_argument_correction(
    order: f64,
    argument: f64,
    correction_term_count: usize,
) -> Option<f64> {
    if !order.is_finite() || !argument.is_finite() || argument <= 0.0 || correction_term_count == 0
    {
        return None;
    }

    let order_term = 4.0 * order * order;
    let scaled_argument = 8.0 * argument;

    let mut correction = 1.0;
    let mut term = 1.0;

    for term_number in 1..=correction_term_count {
        let odd_order = (2 * term_number - 1) as f64;

        term *= (order_term - odd_order * odd_order) / (term_number as f64 * scaled_argument);
        correction += term;
    }

    (correction.is_finite() && correction > 0.0).then_some(correction)
}
