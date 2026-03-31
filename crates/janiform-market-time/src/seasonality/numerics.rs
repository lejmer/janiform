pub(super) fn stable_sum_f64(values: &[f64]) -> f64 {
    let mut running_sum = 0.0;
    let mut compensation = 0.0;

    for value in values.iter().copied() {
        let tentative_sum = running_sum + value;

        if running_sum.abs() >= value.abs() {
            compensation += (running_sum - tentative_sum) + value;
        } else {
            compensation += (value - tentative_sum) + running_sum;
        }

        running_sum = tentative_sum;
    }

    running_sum + compensation
}

pub(super) fn stable_mean_f64(values: &[f64]) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }

    stable_sum_f64(values) / values.len() as f64
}
