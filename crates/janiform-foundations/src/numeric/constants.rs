/// Golden ratio rounded to the nearest `f64`.
pub const GOLDEN_RATIO: f64 = 1.618_033_988_749_895;

/// Reciprocal of the golden ratio rounded to the nearest `f64`.
pub const GOLDEN_RATIO_RECIPROCAL: f64 = 0.618_033_988_749_894_9;

#[cfg(test)]
mod tests {
    use super::{GOLDEN_RATIO, GOLDEN_RATIO_RECIPROCAL};

    #[test]
    fn golden_ratio_constants_match_defining_identities() {
        assert_eq!(GOLDEN_RATIO_RECIPROCAL, GOLDEN_RATIO - 1.0);
        assert_eq!(GOLDEN_RATIO * GOLDEN_RATIO, GOLDEN_RATIO + 1.0);
    }
}
