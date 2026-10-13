use crate::objective::CalibrationLossBuilder;
use crate::stages::joint_refinement::objective::{
    add_objective_family_from_seed_vectors, diagonal_covariance_from_standard_errors,
};
use crate::stages::long_variance::covariance::VarianceCovariance;

/// Consume cached covariance summaries without fitting rates or generating paths.
pub(super) fn add_variance_persistence_terms<'a>(
    builder: &mut CalibrationLossBuilder,
    observed: &[VarianceCovariance],
    models: impl Iterator<Item = &'a [VarianceCovariance]>,
) {
    let moments = observed
        .iter()
        .enumerate()
        .filter_map(|(position, moment)| {
            let value = moment.value?;
            let error = moment.standard_error?;

            (value.is_finite() && error.is_finite() && error > 0.0)
                .then_some((position, moment, value, error))
        })
        .collect::<Vec<_>>();

    let errors = moments
        .iter()
        .map(|(_, _, _, error)| *error)
        .collect::<Vec<_>>();

    let Some(covariance) = diagonal_covariance_from_standard_errors(&errors) else {
        return;
    };

    let vectors = models
        .map(|model| {
            moments
                .iter()
                .map(|(position, observed, _, _)| {
                    let moment = model.get(*position)?;
                    (moment.lag_hours == observed.lag_hours
                        && moment.window_hours == observed.window_hours
                        && moment.sampling_count == observed.sampling_count)
                        .then_some(moment.value)
                        .flatten()
                        .filter(|value| value.is_finite())
                })
                .collect::<Option<Vec<_>>>()
        })
        .collect::<Option<Vec<_>>>();

    let Some(vectors) = vectors.filter(|vectors| !vectors.is_empty()) else {
        builder.add_invalid_candidate_sentinel("long_variance.persistence.missing_model_moments");
        return;
    };

    add_objective_family_from_seed_vectors(
        builder,
        "long_variance.persistence".to_string(),
        moments
            .iter()
            .map(|(_, moment, _, _)| {
                format!(
                    "long_variance.persistence.covariance_{}h_window_{}h_samples_{}",
                    moment.lag_hours, moment.window_hours, moment.sampling_count
                )
            })
            .collect(),
        moments.iter().map(|(_, _, value, _)| *value).collect(),
        covariance,
        vectors,
    );
}

#[cfg(test)]
mod tests {
    use super::add_variance_persistence_terms;
    use crate::objective::CalibrationLossBuilder;
    use crate::stages::long_variance::covariance::VarianceCovariance;

    #[test]
    fn persistence_objective_scores_cached_decay_shape_and_rejects_missing_model_moments() {
        let observed = [VarianceCovariance {
            lag_hours: 24.0,
            window_hours: 1.0,
            sampling_count: 12,
            pair_count: 100,
            value: Some(0.2),
            standard_error: Some(0.05),
        }];

        let mut matching = CalibrationLossBuilder::new();

        add_variance_persistence_terms(&mut matching, &observed, [observed.as_slice()].into_iter());

        let matching = matching.build();

        assert_eq!(matching.global_loss, 0.0);

        assert_eq!(
            matching.family_losses[0].family_id,
            "long_variance.persistence"
        );

        let model = [VarianceCovariance {
            value: Some(0.0),
            ..observed[0]
        }];

        let mut different = CalibrationLossBuilder::new();

        add_variance_persistence_terms(&mut different, &observed, [model.as_slice()].into_iter());

        assert!(different.build().global_loss > matching.global_loss);

        let missing = [VarianceCovariance {
            value: None,
            ..observed[0]
        }];

        let mut invalid = CalibrationLossBuilder::new();

        add_variance_persistence_terms(&mut invalid, &observed, [missing.as_slice()].into_iter());

        assert_eq!(invalid.build().global_loss, f64::INFINITY);
    }
}
