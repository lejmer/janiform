/// Clamp each coordinate of a point to its matching lower and upper bound.
///
/// Callers must pass equal-length slices from an already validated coordinate
/// contract.
pub fn clamp_point(point: &[f64], lower_bounds: &[f64], upper_bounds: &[f64]) -> Vec<f64> {
    let mut clamped_point = point.to_vec();

    clamp_point_in_place(clamped_point.as_mut_slice(), lower_bounds, upper_bounds);

    clamped_point
}

/// Clamp each coordinate of a point in place to its matching lower and upper bound.
///
/// Callers must pass equal-length slices from an already validated coordinate
/// contract.
pub fn clamp_point_in_place(point: &mut [f64], lower_bounds: &[f64], upper_bounds: &[f64]) {
    debug_assert_eq!(point.len(), lower_bounds.len());
    debug_assert_eq!(point.len(), upper_bounds.len());

    for (coordinate, (lower_bound, upper_bound)) in point
        .iter_mut()
        .zip(lower_bounds.iter().zip(upper_bounds.iter()))
    {
        *coordinate = coordinate.clamp(*lower_bound, *upper_bound);
    }
}

#[cfg(test)]
mod tests {
    use super::{clamp_point, clamp_point_in_place};

    #[test]
    fn clamp_point_applies_coordinate_bounds() {
        assert_eq!(
            clamp_point(&[-2.0, 0.5, 3.0], &[0.0, 0.0, 0.0], &[1.0, 1.0, 2.0]),
            vec![0.0, 0.5, 2.0]
        );
    }

    #[test]
    fn clamp_point_in_place_applies_coordinate_bounds() {
        let mut point = vec![-2.0, 0.5, 3.0];

        clamp_point_in_place(point.as_mut_slice(), &[0.0, 0.0, 0.0], &[1.0, 1.0, 2.0]);

        assert_eq!(point, vec![0.0, 0.5, 2.0]);
    }
}
