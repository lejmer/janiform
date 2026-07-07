use std::cmp::Ordering;
use std::collections::BTreeMap;

use super::workflow::CandidateObjectiveResidual;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ResidualTailProfile {
    ordered_tail_prefix_losses: Vec<f64>,
}

impl ResidualTailProfile {
    pub(super) fn from_residuals(residuals: &[CandidateObjectiveResidual]) -> Option<Self> {
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
}

#[cfg(test)]
mod tests {
    use super::{CandidateObjectiveResidual, ResidualTailProfile};

    fn residual(
        family_id: &str,
        residual_id: &str,
        objective_residual: f64,
    ) -> CandidateObjectiveResidual {
        CandidateObjectiveResidual {
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
}
