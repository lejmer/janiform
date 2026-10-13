use super::{minimum_norm_least_squares, non_negative_least_squares_2};

/// Solve three-coordinate nonnegative least squares by enumerating its faces.
/// Column normalization keeps predictors in different physical units comparable.
pub fn non_negative_least_squares_3(rows: &[[f64; 3]], response: &[f64]) -> Option<[f64; 3]> {
    if rows.is_empty()
        || rows.len() != response.len()
        || response.iter().any(|value| !value.is_finite())
        || rows.iter().flatten().any(|value| !value.is_finite())
    {
        return None;
    }

    let scales = std::array::from_fn::<_, 3, _>(|column| {
        rows.iter().map(|row| row[column].abs()).fold(0.0, f64::max)
    });

    let normalized = rows
        .iter()
        .map(|row| {
            std::array::from_fn::<_, 3, _>(|column| {
                if scales[column] > 0.0 {
                    row[column] / scales[column]
                } else {
                    0.0
                }
            })
        })
        .collect::<Vec<_>>();

    let mut candidates = vec![[0.0; 3]];

    let design = normalized
        .iter()
        .map(|row| row.to_vec())
        .collect::<Vec<_>>();

    if let Some(solution) = minimum_norm_least_squares(&design, response)
        && solution.iter().all(|coefficient| *coefficient >= 0.0)
        && let Ok(coefficients) = <[f64; 3]>::try_from(solution)
    {
        candidates.push(coefficients);
    }

    for omitted in 0..3 {
        let columns = (0..3)
            .filter(|column| *column != omitted)
            .collect::<Vec<_>>();

        let projected = normalized
            .iter()
            .map(|row| [row[columns[0]], row[columns[1]]])
            .collect::<Vec<_>>();

        if let Some(solution) = non_negative_least_squares_2(&projected, response) {
            let mut coefficients = [0.0; 3];

            for (column, coefficient) in columns.iter().zip(solution) {
                coefficients[*column] = coefficient;
            }

            candidates.push(coefficients);
        }
    }

    let loss = |coefficients: &[f64; 3]| {
        normalized
            .iter()
            .zip(response)
            .map(|(row, target)| {
                (row.iter()
                    .zip(coefficients)
                    .map(|(value, coefficient)| value * coefficient)
                    .sum::<f64>()
                    - target)
                    .powi(2)
            })
            .sum::<f64>()
    };

    let selected = candidates
        .into_iter()
        .min_by(|left, right| loss(left).total_cmp(&loss(right)))?;

    let coefficients = std::array::from_fn(|column| {
        if scales[column] > 0.0 {
            selected[column] / scales[column]
        } else {
            0.0
        }
    });

    coefficients
        .iter()
        .all(|coefficient: &f64| coefficient.is_finite())
        .then_some(coefficients)
}

#[cfg(test)]
mod tests {
    use super::non_negative_least_squares_3;

    #[test]
    fn nonnegative_fit_handles_physical_units_and_boundary_coefficients() {
        assert_eq!(
            non_negative_least_squares_3(
                &[[1e8, 0.0, 0.0], [0.0, 1e-8, 0.0], [0.0, 0.0, 1.0]],
                &[2.0, 3.0, -1.0],
            ),
            Some([2e-8, 3e8, 0.0])
        );

        assert_eq!(
            non_negative_least_squares_3(&[[0.0; 3]], &[1.0]),
            Some([0.0; 3])
        );

        assert_eq!(non_negative_least_squares_3(&[[f64::NAN; 3]], &[1.0]), None);
    }
}
