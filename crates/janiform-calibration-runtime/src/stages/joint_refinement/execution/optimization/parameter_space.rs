pub(super) fn normalized_parameter_vector(
    parameter_vector: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Option<Vec<f64>> {
    if parameter_vector.len() != lower_bounds.len() || parameter_vector.len() != upper_bounds.len()
    {
        return None;
    }

    parameter_vector
        .iter()
        .zip(lower_bounds.iter().zip(upper_bounds.iter()))
        .map(|(parameter_value, (lower_bound, upper_bound))| {
            let width = upper_bound - lower_bound;
            if !parameter_value.is_finite()
                || !lower_bound.is_finite()
                || !upper_bound.is_finite()
                || width < 0.0
            {
                return None;
            }

            if width == 0.0 {
                return (*parameter_value == *lower_bound).then_some(0.0);
            }

            Some(((parameter_value - lower_bound) / width).clamp(0.0, 1.0))
        })
        .collect()
}

pub(super) fn denormalize_parameter_vector(
    normalized_vector: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Option<Vec<f64>> {
    if normalized_vector.len() != lower_bounds.len()
        || normalized_vector.len() != upper_bounds.len()
    {
        return None;
    }

    normalized_vector
        .iter()
        .zip(lower_bounds.iter().zip(upper_bounds.iter()))
        .map(|(normalized_value, (lower_bound, upper_bound))| {
            let width = upper_bound - lower_bound;
            if !normalized_value.is_finite()
                || !lower_bound.is_finite()
                || !upper_bound.is_finite()
                || width < 0.0
            {
                return None;
            }

            if width == 0.0 {
                return Some(*lower_bound);
            }

            Some(lower_bound + normalized_value.clamp(0.0, 1.0) * width)
        })
        .collect()
}

pub(crate) fn movable_parameter_count(lower_bounds: &[f64], upper_bounds: &[f64]) -> usize {
    lower_bounds
        .iter()
        .zip(upper_bounds.iter())
        .filter(|(lower_bound, upper_bound)| {
            let width = *upper_bound - *lower_bound;
            lower_bound.is_finite() && upper_bound.is_finite() && width > 0.0
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::{
        denormalize_parameter_vector, movable_parameter_count, normalized_parameter_vector,
    };

    #[test]
    fn fixed_coordinates_normalize_as_zero_and_remain_fixed() {
        let lower_bounds = vec![0.0, -1.0, 2.0];
        let upper_bounds = vec![0.0, 1.0, 2.0];
        let parameter_vector = vec![0.0, 0.5, 2.0];

        assert_eq!(
            normalized_parameter_vector(
                parameter_vector.as_slice(),
                lower_bounds.as_slice(),
                upper_bounds.as_slice(),
            ),
            Some(vec![0.0, 0.75, 0.0])
        );
        assert_eq!(
            denormalize_parameter_vector(
                &[0.5, 0.75, 1.0],
                lower_bounds.as_slice(),
                upper_bounds.as_slice(),
            ),
            Some(parameter_vector)
        );
        assert_eq!(
            movable_parameter_count(lower_bounds.as_slice(), upper_bounds.as_slice()),
            1
        );
    }
}
