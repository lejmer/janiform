/// Golden ratio rounded to the nearest `f64`.
pub const GOLDEN_RATIO: f64 = 1.618_033_988_749_895;

/// Reciprocal of the golden ratio rounded to the nearest `f64`.
pub const GOLDEN_RATIO_RECIPROCAL: f64 = 0.618_033_988_749_894_9;

/// Square root of `f64` machine epsilon.
///
/// This is the scale where first-order floating-point cancellation error is
/// expected after subtracting two well-conditioned `f64` values.
pub fn square_root_machine_epsilon() -> f64 {
    f64::EPSILON.sqrt()
}

#[cfg(test)]
mod tests {
    use super::{GOLDEN_RATIO, GOLDEN_RATIO_RECIPROCAL, square_root_machine_epsilon};

    #[test]
    fn golden_ratio_constants_match_defining_identities() {
        assert_eq!(GOLDEN_RATIO_RECIPROCAL, GOLDEN_RATIO - 1.0);
        assert_eq!(GOLDEN_RATIO * GOLDEN_RATIO, GOLDEN_RATIO + 1.0);
    }

    #[test]
    fn square_root_machine_epsilon_matches_defining_identity() {
        let observed = square_root_machine_epsilon();

        assert_eq!(observed * observed, f64::EPSILON);
    }
}
