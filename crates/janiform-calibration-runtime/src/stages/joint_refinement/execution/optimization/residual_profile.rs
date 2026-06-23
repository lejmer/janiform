use std::cmp::Ordering;
use std::collections::BTreeMap;

use janiform_calibration_artifacts::CalibrationOptimizerSettings;

use crate::stages::adaptive_direct_search::objective_improves;

use super::workflow::RestartObjectiveResidual;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ResidualTailProfile {
    ordered_tail_prefix_losses: Vec<f64>,
}

impl ResidualTailProfile {
    pub(super) fn from_residuals(residuals: &[RestartObjectiveResidual]) -> Option<Self> {
        let mut energies = Vec::with_capacity(residuals.len());
        let mut family_energies = BTreeMap::<&str, f64>::new();

        for residual in residuals {
            let energy = residual.objective_residual * residual.objective_residual;
            if !energy.is_finite() {
                return None;
            }

            energies.push(energy);

            let family_energy = family_energies
                .entry(residual.family_id.as_str())
                .or_insert(0.0);

            *family_energy += energy;
        }

        energies.extend(family_energies.into_values());

        if energies.is_empty() {
            return None;
        }

        energies.sort_by(|left, right| right.total_cmp(left));

        let mut running_loss = 0.0;

        let ordered_tail_prefix_losses = energies
            .into_iter()
            .map(|energy| {
                running_loss += energy;
                running_loss
            })
            .collect::<Vec<_>>();

        Some(Self {
            ordered_tail_prefix_losses,
        })
    }

    #[allow(dead_code)]
    pub(super) fn dominates(
        &self,
        incumbent: &Self,
        solver_settings: &CalibrationOptimizerSettings,
    ) -> bool {
        if self.ordered_tail_prefix_losses.len() != incumbent.ordered_tail_prefix_losses.len() {
            return false;
        }

        let mut has_improved_prefix = false;

        for (candidate_loss, incumbent_loss) in self
            .ordered_tail_prefix_losses
            .iter()
            .zip(incumbent.ordered_tail_prefix_losses.iter())
        {
            if objective_improves(
                *incumbent_loss,
                *candidate_loss,
                solver_settings.absolute_tolerance,
                solver_settings.relative_tolerance,
            ) {
                return false;
            }

            if objective_improves(
                *candidate_loss,
                *incumbent_loss,
                solver_settings.absolute_tolerance,
                solver_settings.relative_tolerance,
            ) {
                has_improved_prefix = true;
            }
        }

        has_improved_prefix
    }

    pub(super) fn compare(&self, other: &Self) -> Ordering {
        self.ordered_tail_prefix_losses
            .iter()
            .zip(other.ordered_tail_prefix_losses.iter())
            .map(|(left_loss, right_loss)| left_loss.total_cmp(right_loss))
            .find(|ordering| *ordering != Ordering::Equal)
            .unwrap_or_else(|| {
                self.ordered_tail_prefix_losses
                    .len()
                    .cmp(&other.ordered_tail_prefix_losses.len())
            })
    }

    #[allow(dead_code)]
    pub(super) fn total_loss(&self) -> f64 {
        self.ordered_tail_prefix_losses
            .last()
            .copied()
            .unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use janiform_calibration_artifacts::CalibrationOptimizerSettings;

    use super::{ResidualTailProfile, RestartObjectiveResidual};

    fn solver_settings() -> CalibrationOptimizerSettings {
        CalibrationOptimizerSettings::try_new(
            "test",
            64,
            1,
            f64::MIN_POSITIVE,
            f64::MIN_POSITIVE,
            1.0,
        )
        .expect("test solver settings are valid")
    }

    fn residual(
        family_id: &str,
        residual_id: &str,
        objective_residual: f64,
    ) -> RestartObjectiveResidual {
        RestartObjectiveResidual {
            family_id: family_id.to_string(),
            residual_id: residual_id.to_string(),
            objective_residual,
        }
    }

    #[test]
    fn residual_tail_profile_includes_coordinate_and_family_energy() {
        let profile = ResidualTailProfile::from_residuals(&[
            residual("shape", "a", 2.0),
            residual("shape", "b", 1.0),
            residual("clock", "a", 3.0),
        ])
        .expect("profile can be built from finite residuals");

        assert_eq!(
            profile.ordered_tail_prefix_losses,
            vec![9.0, 18.0, 23.0, 27.0, 28.0]
        );
    }

    #[test]
    fn residual_tail_dominance_rejects_worse_prefix_loss() {
        let settings = solver_settings();

        let candidate = ResidualTailProfile {
            ordered_tail_prefix_losses: vec![8.0, 16.0],
        };

        let incumbent = ResidualTailProfile {
            ordered_tail_prefix_losses: vec![9.0, 15.0],
        };

        assert!(!candidate.dominates(&incumbent, &settings));
    }

    #[test]
    fn residual_tail_dominance_accepts_better_tail_profile() {
        let settings = solver_settings();

        let candidate = ResidualTailProfile {
            ordered_tail_prefix_losses: vec![8.0, 14.0],
        };

        let incumbent = ResidualTailProfile {
            ordered_tail_prefix_losses: vec![9.0, 15.0],
        };

        assert!(candidate.dominates(&incumbent, &settings));
    }
}
