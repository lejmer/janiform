pub fn interpolate_range(min_value: f64, max_value: f64, noise: f64) -> f64 {
    min_value + (max_value - min_value) * noise
}

pub fn splitmix64(value: u64) -> u64 {
    let mut mixed = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    mixed ^ (mixed >> 31)
}

pub fn derive_invocation_seed(
    component_seed: u64,
    invocation_count_including_warmup: usize,
) -> u64 {
    splitmix64(component_seed ^ invocation_count_including_warmup as u64)
}

pub fn unit_interval_from_seed(seed: u64) -> f64 {
    let sampled_integer = splitmix64(seed) >> 11;
    sampled_integer as f64 * (1.0 / ((1_u64 << 53) as f64))
}

pub fn fnv1a_64(payload: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for value in payload {
        hash ^= *value as u64;
        hash = hash.wrapping_mul(0x1000_0000_01B3);
    }

    hash
}
