use janiform_foundations::linear_algebra::symmetric_psd_whitened_quadratic_form;

use super::contracts::{ObjectiveFamilyEvaluation, ObjectiveFamilyInput};

pub(crate) fn evaluate_objective_family(
    input: ObjectiveFamilyInput,
) -> Option<ObjectiveFamilyEvaluation> {
    let ObjectiveFamilyInput {
        family_id,
        coordinates,
        covariance_rows,
    } = input;

    let coordinate_count = coordinates.len();
    if coordinate_count == 0
        || covariance_rows.len() != coordinate_count
        || covariance_rows
            .iter()
            .any(|row| row.len() != coordinate_count)
    {
        return None;
    }

    let mut coordinate_ids = Vec::with_capacity(coordinate_count);
    let mut observed_values = Vec::with_capacity(coordinate_count);
    let mut model_values = Vec::with_capacity(coordinate_count);
    let mut residual_vector = Vec::with_capacity(coordinate_count);
    let mut marginal_standardized_residuals = Vec::with_capacity(coordinate_count);

    for (coordinate_position, coordinate) in coordinates.into_iter().enumerate() {
        if !coordinate.observed_value.is_finite() || !coordinate.model_value.is_finite() {
            return None;
        }

        let residual = coordinate.model_value - coordinate.observed_value;
        let covariance_diagonal = covariance_rows[coordinate_position][coordinate_position];

        let marginal_standardized_residual = (covariance_diagonal.is_finite()
            && covariance_diagonal > 0.0)
            .then_some(residual / covariance_diagonal.sqrt());

        coordinate_ids.push(coordinate.coordinate_id);
        observed_values.push(coordinate.observed_value);
        model_values.push(coordinate.model_value);
        residual_vector.push(residual);
        marginal_standardized_residuals.push(marginal_standardized_residual);
    }

    let whitened_form =
        symmetric_psd_whitened_quadratic_form(covariance_rows.as_slice(), &residual_vector)?;

    if whitened_form.rank == 0 {
        return Some(ObjectiveFamilyEvaluation {
            family_id,
            coordinate_ids,
            observed_values,
            model_values,
            covariance_rows,
            marginal_standardized_residuals,
            whitened_coordinates: whitened_form.whitened_coordinates,
            loss: 0.0,
            rank: 0,
        });
    }

    Some(ObjectiveFamilyEvaluation {
        family_id,
        coordinate_ids,
        observed_values,
        model_values,
        covariance_rows,
        marginal_standardized_residuals,
        whitened_coordinates: whitened_form.whitened_coordinates,
        loss: whitened_form.quadratic_form / whitened_form.rank as f64,
        rank: whitened_form.rank,
    })
}

#[cfg(test)]
mod tests {
    use super::evaluate_objective_family;
    use crate::objective_families::{ObjectiveCoordinateInput, ObjectiveFamilyInput};

    #[test]
    fn objective_family_loss_is_rank_normalized() {
        let evaluation = evaluate_objective_family(ObjectiveFamilyInput {
            family_id: "shape".to_string(),
            coordinates: vec![
                ObjectiveCoordinateInput {
                    coordinate_id: "a".to_string(),
                    observed_value: 0.0,
                    model_value: 1.0,
                },
                ObjectiveCoordinateInput {
                    coordinate_id: "b".to_string(),
                    observed_value: 0.0,
                    model_value: 1.0,
                },
            ],
            covariance_rows: vec![vec![1.0, 0.0], vec![0.0, 1.0]],
        })
        .expect("identified family should evaluate");

        assert_eq!(evaluation.loss, 1.0);
        assert_eq!(evaluation.rank, 2);
    }
}
