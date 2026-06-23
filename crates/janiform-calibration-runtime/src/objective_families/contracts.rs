#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ObjectiveCoordinateInput {
    pub(crate) coordinate_id: String,
    pub(crate) observed_value: f64,
    pub(crate) model_value: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ObjectiveFamilyInput {
    pub(crate) family_id: String,
    pub(crate) coordinates: Vec<ObjectiveCoordinateInput>,
    pub(crate) covariance_rows: Vec<Vec<f64>>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ObjectiveFamilyEvaluation {
    pub(crate) family_id: String,
    pub(crate) coordinate_ids: Vec<String>,
    pub(crate) observed_values: Vec<f64>,
    pub(crate) model_values: Vec<f64>,
    pub(crate) covariance_rows: Vec<Vec<f64>>,
    pub(crate) marginal_standardized_residuals: Vec<Option<f64>>,
    pub(crate) whitened_coordinates: Vec<f64>,
    pub(crate) loss: f64,
    pub(crate) rank: usize,
}

impl ObjectiveFamilyEvaluation {
    pub(crate) fn coordinate_count(&self) -> usize {
        self.coordinate_ids.len()
    }

    pub(crate) fn max_marginal_standardized_residual(&self) -> (f64, Option<&str>) {
        let mut max_residual = 0.0_f64;
        let mut coordinate_id = None;

        for (current_coordinate_id, residual) in self
            .coordinate_ids
            .iter()
            .zip(self.marginal_standardized_residuals.iter())
        {
            let Some(residual) = residual else {
                continue;
            };

            let absolute_residual = residual.abs();
            if absolute_residual > max_residual {
                max_residual = absolute_residual;
                coordinate_id = Some(current_coordinate_id.as_str());
            }
        }

        (max_residual, coordinate_id)
    }
}
