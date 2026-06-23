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

pub(super) fn normalized_parameter_step(
    accepted_vector: &[f64],
    candidate_vector: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Option<Vec<f64>> {
    if accepted_vector.len() != candidate_vector.len()
        || accepted_vector.len() != lower_bounds.len()
        || accepted_vector.len() != upper_bounds.len()
    {
        return None;
    }

    accepted_vector
        .iter()
        .zip(candidate_vector.iter())
        .zip(lower_bounds.iter().zip(upper_bounds.iter()))
        .map(
            |((accepted_value, candidate_value), (lower_bound, upper_bound))| {
                let width = upper_bound - lower_bound;
                if !accepted_value.is_finite()
                    || !candidate_value.is_finite()
                    || !lower_bound.is_finite()
                    || !upper_bound.is_finite()
                    || width < 0.0
                {
                    return None;
                }

                if width == 0.0 {
                    return (*accepted_value == *candidate_value
                        && *accepted_value == *lower_bound)
                        .then_some(0.0);
                }

                Some((candidate_value - accepted_value) / width)
            },
        )
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

pub(super) fn movable_parameter_positions(
    parameter_vector: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Vec<usize> {
    parameter_vector
        .iter()
        .zip(lower_bounds.iter().zip(upper_bounds.iter()))
        .enumerate()
        .filter_map(
            |(parameter_position, (parameter_value, (lower_bound, upper_bound)))| {
                let width = upper_bound - lower_bound;
                (parameter_value.is_finite()
                    && lower_bound.is_finite()
                    && upper_bound.is_finite()
                    && width > 0.0)
                    .then_some(parameter_position)
            },
        )
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

pub(super) fn parameter_step_is_zero(step: &[f64]) -> bool {
    step.iter().all(|component| *component == 0.0)
}

#[cfg(test)]
mod tests {
    use super::{
        denormalize_parameter_vector, movable_parameter_count, movable_parameter_positions,
        normalized_parameter_step, normalized_parameter_vector,
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
            movable_parameter_positions(
                &[0.0, 0.5, 2.0],
                lower_bounds.as_slice(),
                upper_bounds.as_slice()
            ),
            vec![1]
        );
        assert_eq!(
            movable_parameter_count(lower_bounds.as_slice(), upper_bounds.as_slice()),
            1
        );
    }

    #[test]
    fn fixed_coordinate_steps_are_zero_width_safe() {
        assert_eq!(
            normalized_parameter_step(&[0.0, 0.0], &[0.0, 0.5], &[0.0, -1.0], &[0.0, 1.0],),
            Some(vec![0.0, 0.25])
        );
    }
}
