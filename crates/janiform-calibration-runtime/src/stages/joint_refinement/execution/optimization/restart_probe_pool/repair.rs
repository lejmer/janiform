use std::collections::BTreeMap;

use super::RestartObjectiveResidual;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct RestartRepairScore {
    pub(super) worst_family_loss: f64,
    pub(super) top_residual_energy: f64,
}

impl RestartRepairScore {
    pub(super) fn loss(self) -> f64 {
        self.worst_family_loss.max(self.top_residual_energy)
    }
}

pub(super) fn restart_repair_loss(residuals: &[RestartObjectiveResidual]) -> Option<f64> {
    restart_repair_score(residuals).map(RestartRepairScore::loss)
}

pub(super) fn restart_repair_score(
    residuals: &[RestartObjectiveResidual],
) -> Option<RestartRepairScore> {
    restart_repair_score_from_components(
        residuals
            .iter()
            .map(|residual| (residual.family_id.as_str(), residual.objective_residual)),
    )
}

pub(super) fn restart_repair_score_from_components<'a>(
    residuals: impl IntoIterator<Item = (&'a str, f64)>,
) -> Option<RestartRepairScore> {
    let mut family_loss_by_id = BTreeMap::new();
    let mut top_residual_energy = None::<f64>;

    for (family_id, objective_residual) in residuals {
        if family_id.is_empty() || !objective_residual.is_finite() {
            return None;
        }

        let residual_energy = objective_residual * objective_residual;
        if !residual_energy.is_finite() {
            return None;
        }

        family_loss_by_id
            .entry(family_id)
            .and_modify(|family_loss| *family_loss += residual_energy)
            .or_insert(residual_energy);

        top_residual_energy = Some(
            top_residual_energy
                .map(|current_energy| current_energy.max(residual_energy))
                .unwrap_or(residual_energy),
        );
    }

    let worst_family_loss = family_loss_by_id.values().copied().fold(
        None,
        |current_loss: Option<f64>, family_loss| {
            Some(
                current_loss
                    .map(|current_loss| current_loss.max(family_loss))
                    .unwrap_or(family_loss),
            )
        },
    )?;

    Some(RestartRepairScore {
        worst_family_loss,
        top_residual_energy: top_residual_energy?,
    })
}

#[cfg(test)]
mod tests {
    use super::{RestartObjectiveResidual, RestartRepairScore, restart_repair_score};

    #[test]
    fn repair_score_uses_worst_family_and_top_residual_energy() {
        let residuals = vec![
            RestartObjectiveResidual {
                family_id: "shape".to_string(),
                residual_id: "shape.a".to_string(),
                objective_residual: 2.0,
            },
            RestartObjectiveResidual {
                family_id: "shape".to_string(),
                residual_id: "shape.b".to_string(),
                objective_residual: 1.0,
            },
            RestartObjectiveResidual {
                family_id: "activity".to_string(),
                residual_id: "activity.a".to_string(),
                objective_residual: 3.0,
            },
        ];

        assert_eq!(
            restart_repair_score(residuals.as_slice()),
            Some(RestartRepairScore {
                worst_family_loss: 9.0,
                top_residual_energy: 9.0,
            })
        );
    }
}
