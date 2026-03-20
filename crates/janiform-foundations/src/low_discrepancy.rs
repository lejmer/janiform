/// Deterministic Van der Corput radical-inverse sequence on the open unit interval.
pub fn van_der_corput(mut index: u64, base: u64) -> f64 {
    let mut fraction = 1.0 / base as f64;
    let mut value = 0.0;

    while index > 0 {
        value += fraction * (index % base) as f64;
        index /= base;
        fraction /= base as f64;
    }

    value
}

#[cfg(test)]
mod tests {
    use super::van_der_corput;

    #[test]
    fn van_der_corput_matches_base_two_reference_sequence() {
        let values: Vec<f64> = (1..=5).map(|index| van_der_corput(index, 2)).collect();
        assert_eq!(values, vec![0.5, 0.25, 0.75, 0.125, 0.625]);
    }

    #[test]
    fn van_der_corput_is_deterministic_for_fixed_index_and_base() {
        assert_eq!(van_der_corput(17, 3), van_der_corput(17, 3));
    }
}
