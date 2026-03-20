//! Shared deterministic fixed-size vector helpers.

pub fn vector4_add(left: [f64; 4], right: [f64; 4]) -> [f64; 4] {
    [
        left[0] + right[0],
        left[1] + right[1],
        left[2] + right[2],
        left[3] + right[3],
    ]
}

pub fn vector4_subtract(left: [f64; 4], right: [f64; 4]) -> [f64; 4] {
    [
        left[0] - right[0],
        left[1] - right[1],
        left[2] - right[2],
        left[3] - right[3],
    ]
}

pub fn vector4_scale(vector: [f64; 4], scalar: f64) -> [f64; 4] {
    [
        vector[0] * scalar,
        vector[1] * scalar,
        vector[2] * scalar,
        vector[3] * scalar,
    ]
}

pub fn normalize_vector4(direction: [f64; 4], minimum_norm: f64) -> [f64; 4] {
    let norm = direction
        .iter()
        .map(|coordinate| coordinate.powi(2))
        .sum::<f64>()
        .sqrt();
    if !norm.is_finite() || norm <= minimum_norm {
        return [0.0, 0.0, 0.0, 0.0];
    }

    vector4_scale(direction, 1.0 / norm)
}

#[cfg(test)]
mod tests {
    use super::{normalize_vector4, vector4_add, vector4_scale, vector4_subtract};

    #[test]
    fn vector4_helpers_preserve_componentwise_arithmetic() {
        assert_eq!(
            vector4_add([1.0, 2.0, 3.0, 4.0], [0.5, -1.0, 2.0, -3.0]),
            [1.5, 1.0, 5.0, 1.0]
        );
        assert_eq!(
            vector4_subtract([1.0, 2.0, 3.0, 4.0], [0.5, -1.0, 2.0, -3.0]),
            [0.5, 3.0, 1.0, 7.0]
        );
        assert_eq!(
            vector4_scale([1.0, -2.0, 3.0, -4.0], 0.5),
            [0.5, -1.0, 1.5, -2.0]
        );
    }

    #[test]
    fn normalize_vector4_returns_zero_for_tiny_norms() {
        assert_eq!(normalize_vector4([0.0, 0.0, 0.0, 0.0], 1.0e-12), [0.0; 4]);
    }

    #[test]
    fn normalize_vector4_rescales_non_zero_direction() {
        let normalized = normalize_vector4([3.0, 4.0, 0.0, 0.0], 1.0e-12);
        let norm = normalized
            .iter()
            .map(|coordinate| coordinate.powi(2))
            .sum::<f64>()
            .sqrt();

        assert!((norm - 1.0).abs() <= 1.0e-12);
        assert!((normalized[0] - 0.6).abs() <= 1.0e-12);
        assert!((normalized[1] - 0.8).abs() <= 1.0e-12);
    }
}
