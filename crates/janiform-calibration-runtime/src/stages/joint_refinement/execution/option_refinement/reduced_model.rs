use nalgebra::{DMatrix, SymmetricEigen};
use serde::{Deserialize, Serialize};

use janiform_calibration_artifacts::JointRefinementStageOptionReducedModelDiagnostics;

use super::objective_gradient::JointRefinementOptionNodeResidualSensitivity;
use super::residual_jacobian::weighted_option_residual_jacobian_rows;

const OPTION_REDUCED_MODEL_ACTIVITY_PARAMETER_LABELS: [&str; 2] =
    ["log_buy_activity_premium", "log_sell_activity_premium"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct JointRefinementOptionReducedModelArtifact {
    pub(crate) local_parameter_labels: Vec<String>,
    pub(crate) weighted_residual_jacobian_row_count: usize,
    pub(crate) weighted_residual_jacobian_column_count: usize,
    pub(crate) weighted_residual_jacobian_singular_values: Vec<f64>,
    pub(crate) cumulative_rank_energy: Vec<f64>,
    pub(crate) effective_rank_at_90_percent_energy: usize,
    pub(crate) effective_rank_at_95_percent_energy: usize,
    pub(crate) effective_rank_at_99_percent_energy: usize,
}

pub(crate) fn build_option_reduced_model_artifact(
    node_sensitivities: &[JointRefinementOptionNodeResidualSensitivity],
) -> Result<JointRefinementOptionReducedModelArtifact, String> {
    let weighted_rows = weighted_option_residual_jacobian_rows(node_sensitivities)?;
    let gradient_dimension = weighted_rows
        .first()
        .map(|row| row.gradient.len())
        .ok_or_else(|| {
            "option reduced model requires at least one priced option node".to_string()
        })?;

    let mut weighted_gram = DMatrix::<f64>::zeros(gradient_dimension, gradient_dimension);

    for row in &weighted_rows {
        for left_index in 0..gradient_dimension {
            let left_value = row.gradient[left_index];

            for right_index in 0..gradient_dimension {
                weighted_gram[(left_index, right_index)] += left_value * row.gradient[right_index];
            }
        }
    }

    let symmetric_eigen = SymmetricEigen::new(weighted_gram);

    let mut singular_values = symmetric_eigen
        .eigenvalues
        .iter()
        .map(|eigenvalue| eigenvalue.max(0.0).sqrt())
        .collect::<Vec<_>>();

    singular_values.sort_by(f64::total_cmp);
    singular_values.reverse();

    let total_energy = singular_values
        .iter()
        .map(|value| value * value)
        .sum::<f64>();

    let mut cumulative_energy = 0.0;

    let cumulative_rank_energy = singular_values
        .iter()
        .map(|singular_value| {
            cumulative_energy += singular_value * singular_value;
            if total_energy > 0.0 {
                cumulative_energy / total_energy
            } else {
                0.0
            }
        })
        .collect::<Vec<_>>();

    Ok(JointRefinementOptionReducedModelArtifact {
        local_parameter_labels: reduced_model_parameter_labels(gradient_dimension),
        weighted_residual_jacobian_row_count: weighted_rows.len(),
        weighted_residual_jacobian_column_count: gradient_dimension,
        weighted_residual_jacobian_singular_values: singular_values,
        effective_rank_at_90_percent_energy: effective_rank_for_energy(
            cumulative_rank_energy.as_slice(),
            0.90,
        ),
        effective_rank_at_95_percent_energy: effective_rank_for_energy(
            cumulative_rank_energy.as_slice(),
            0.95,
        ),
        effective_rank_at_99_percent_energy: effective_rank_for_energy(
            cumulative_rank_energy.as_slice(),
            0.99,
        ),
        cumulative_rank_energy,
    })
}

pub(crate) fn reduced_model_diagnostics_from_artifact(
    artifact: &JointRefinementOptionReducedModelArtifact,
    current_search_evaluation_budget: usize,
) -> JointRefinementStageOptionReducedModelDiagnostics {
    JointRefinementStageOptionReducedModelDiagnostics {
        local_parameter_labels: artifact.local_parameter_labels.clone(),
        weighted_residual_jacobian_row_count: artifact.weighted_residual_jacobian_row_count,
        weighted_residual_jacobian_column_count: artifact.weighted_residual_jacobian_column_count,
        weighted_residual_jacobian_singular_values: artifact
            .weighted_residual_jacobian_singular_values
            .clone(),
        cumulative_rank_energy: artifact.cumulative_rank_energy.clone(),
        effective_rank_at_90_percent_energy: artifact.effective_rank_at_90_percent_energy,
        effective_rank_at_95_percent_energy: artifact.effective_rank_at_95_percent_energy,
        effective_rank_at_99_percent_energy: artifact.effective_rank_at_99_percent_energy,
        current_search_evaluation_budget,
        predicted_local_linear_oracle_call_count: 2,
        predicted_local_quadratic_oracle_call_count: 2 + artifact
            .effective_rank_at_95_percent_energy,
        reference_oracle_call_wall_clock_millis: None,
        predicted_local_linear_wall_clock_millis: None,
        predicted_local_quadratic_wall_clock_millis: None,
        predicted_local_linear_wall_clock_saved_millis: None,
        predicted_local_quadratic_wall_clock_saved_millis: None,
    }
}

pub(crate) fn populate_reduced_model_runtime_estimates(
    diagnostics: &mut JointRefinementStageOptionReducedModelDiagnostics,
    reference_oracle_call_wall_clock_millis: Option<f64>,
) {
    let Some(reference_oracle_call_wall_clock_millis) = reference_oracle_call_wall_clock_millis
    else {
        return;
    };

    diagnostics.reference_oracle_call_wall_clock_millis =
        Some(reference_oracle_call_wall_clock_millis);

    let linear_wall_clock = diagnostics.predicted_local_linear_oracle_call_count as f64
        * reference_oracle_call_wall_clock_millis;
    let quadratic_wall_clock = diagnostics.predicted_local_quadratic_oracle_call_count as f64
        * reference_oracle_call_wall_clock_millis;
    let baseline_wall_clock = diagnostics.current_search_evaluation_budget as f64
        * reference_oracle_call_wall_clock_millis;

    diagnostics.predicted_local_linear_wall_clock_millis = Some(linear_wall_clock);
    diagnostics.predicted_local_quadratic_wall_clock_millis = Some(quadratic_wall_clock);
    diagnostics.predicted_local_linear_wall_clock_saved_millis =
        Some((baseline_wall_clock - linear_wall_clock).max(0.0));
    diagnostics.predicted_local_quadratic_wall_clock_saved_millis =
        Some((baseline_wall_clock - quadratic_wall_clock).max(0.0));
}

fn reduced_model_parameter_labels(gradient_dimension: usize) -> Vec<String> {
    (0..gradient_dimension)
        .map(reduced_model_parameter_label)
        .collect()
}

fn reduced_model_parameter_label(parameter_position: usize) -> String {
    if let Some(label) = OPTION_REDUCED_MODEL_ACTIVITY_PARAMETER_LABELS.get(parameter_position) {
        return (*label).to_string();
    }

    if parameter_position == OPTION_REDUCED_MODEL_ACTIVITY_PARAMETER_LABELS.len() {
        return "pressure_signed_imbalance_exposure_risk_price".to_string();
    }

    format!("option_pricing_parameter_{parameter_position}")
}

fn effective_rank_for_energy(cumulative_rank_energy: &[f64], threshold: f64) -> usize {
    cumulative_rank_energy
        .iter()
        .position(|energy| *energy >= threshold)
        .map(|position| position + 1)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::build_option_reduced_model_artifact;
    use crate::stages::joint_refinement::execution::option_refinement::objective_gradient::JointRefinementOptionNodeResidualSensitivity;

    #[test]
    fn option_reduced_model_labels_activity_premium_coordinates() {
        let artifact = build_option_reduced_model_artifact(&[
            JointRefinementOptionNodeResidualSensitivity {
                expiry_timestamp_utc_nanos: 10,
                maturity_weight: 1,
                spread_normalized_residual: 1.0,
                spread_normalized_residual_gradient: vec![0.5, -0.25],
            },
            JointRefinementOptionNodeResidualSensitivity {
                expiry_timestamp_utc_nanos: 10,
                maturity_weight: 1,
                spread_normalized_residual: -1.0,
                spread_normalized_residual_gradient: vec![0.25, 0.5],
            },
        ])
        .expect("reduced model artifact should construct");

        assert_eq!(
            artifact.local_parameter_labels,
            vec![
                "log_buy_activity_premium".to_string(),
                "log_sell_activity_premium".to_string(),
            ]
        );
    }

    #[test]
    fn option_reduced_model_labels_pressure_risk_price_coordinate() {
        let artifact = build_option_reduced_model_artifact(&[
            JointRefinementOptionNodeResidualSensitivity {
                expiry_timestamp_utc_nanos: 10,
                maturity_weight: 1,
                spread_normalized_residual: 1.0,
                spread_normalized_residual_gradient: vec![0.5, -0.25, 0.0],
            },
            JointRefinementOptionNodeResidualSensitivity {
                expiry_timestamp_utc_nanos: 10,
                maturity_weight: 1,
                spread_normalized_residual: -1.0,
                spread_normalized_residual_gradient: vec![0.25, 0.5, 0.0],
            },
        ])
        .expect("reduced model artifact should construct");

        assert_eq!(
            artifact.local_parameter_labels,
            vec![
                "log_buy_activity_premium".to_string(),
                "log_sell_activity_premium".to_string(),
                "pressure_signed_imbalance_exposure_risk_price".to_string(),
            ]
        );
    }
}
