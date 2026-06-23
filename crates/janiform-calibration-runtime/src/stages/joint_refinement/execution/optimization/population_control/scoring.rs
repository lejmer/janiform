use std::cmp::Ordering;

use super::super::residual_profile::ResidualTailProfile;
use super::super::workflow::RestartObjectiveEvaluation;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct PopulationControlScore {
    profile: Option<ResidualTailProfile>,
    admissibility_warning_count: usize,
    max_standardized_residual: Option<f64>,
    objective: f64,
}

impl PopulationControlScore {
    pub(super) fn from_evaluation(evaluation: &RestartObjectiveEvaluation) -> Self {
        Self {
            profile: ResidualTailProfile::from_residuals(evaluation.residuals.as_slice()),
            admissibility_warning_count: evaluation.admissibility_warning_count,
            max_standardized_residual: evaluation.max_standardized_residual,
            objective: evaluation.objective,
        }
    }

    pub(super) fn objective(&self) -> f64 {
        self.objective
    }

    pub(super) fn compare(&self, other: &Self) -> Ordering {
        compare_profiles(self.profile.as_ref(), other.profile.as_ref())
            .then_with(|| {
                self.admissibility_warning_count
                    .cmp(&other.admissibility_warning_count)
            })
            .then_with(|| {
                compare_optional_loss(
                    self.max_standardized_residual,
                    other.max_standardized_residual,
                )
            })
            .then_with(|| self.objective.total_cmp(&other.objective))
    }
}

fn compare_profiles(
    left_profile: Option<&ResidualTailProfile>,
    right_profile: Option<&ResidualTailProfile>,
) -> Ordering {
    match (left_profile, right_profile) {
        (Some(left_profile), Some(right_profile)) => left_profile.compare(right_profile),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare_optional_loss(left_loss: Option<f64>, right_loss: Option<f64>) -> Ordering {
    match (left_loss, right_loss) {
        (Some(left_loss), Some(right_loss)) => left_loss.total_cmp(&right_loss),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::workflow::{RestartObjectiveEvaluation, RestartObjectiveResidual};
    use super::PopulationControlScore;

    fn evaluation(objective: f64, residual: f64) -> RestartObjectiveEvaluation {
        RestartObjectiveEvaluation {
            objective,
            admissibility_warning_count: 0,
            max_standardized_residual: Some(residual.abs()),
            exact_likelihood_evaluator_counts: None,
            residuals: vec![RestartObjectiveResidual {
                family_id: "shape".to_string(),
                residual_id: "shape.tail".to_string(),
                objective_residual: residual,
            }],
        }
    }

    #[test]
    fn residual_tail_score_precedes_global_loss() {
        let lower_global_loss = PopulationControlScore::from_evaluation(&evaluation(1.0, 4.0));
        let lower_tail_loss = PopulationControlScore::from_evaluation(&evaluation(2.0, 2.0));

        assert!(lower_tail_loss.compare(&lower_global_loss).is_lt());
    }
}
