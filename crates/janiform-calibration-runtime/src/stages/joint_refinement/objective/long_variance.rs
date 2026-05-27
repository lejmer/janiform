use janiform_foundations::numeric::{MeanEstimate, mean_estimate_from_standard_errors};

use crate::objective::CalibrationLossBuilder;
use crate::stages::joint_metrics::log_ratio_distance;
use crate::stages::joint_refinement::JointRefinementSeedSummary;
use crate::stages::joint_refinement::execution::JointRefinementObservedData;
use crate::stages::joint_refinement::objective::{add_direct_term, root_mean_square};
use crate::stages::long_variance::metrics::{
    LONG_VARIANCE_HORIZON_DEFINITIONS, LongVarianceHorizonMomentStandardErrors,
    LongVarianceHorizonMomentSummary,
};

const LONG_VARIANCE_OBJECTIVE_FAMILY_ID: &str = "long_variance";

pub(super) fn long_variance_horizon_fit(
    seed_summary: &JointRefinementSeedSummary,
    observed_data: &JointRefinementObservedData,
) -> f64 {
    let mut differences = Vec::new();

    for (observed_summary, model_summary) in observed_data
        .observed_long_variance_horizon_summaries
        .iter()
        .zip(
            seed_summary
                .simulated_long_variance_horizon_summaries
                .iter(),
        )
    {
        push_positive_metric_difference(
            &mut differences,
            observed_summary.realized_variance,
            model_summary.realized_variance,
        );

        push_positive_metric_difference(
            &mut differences,
            observed_summary.variance_of_variance,
            model_summary.variance_of_variance,
        );

        push_unit_metric_difference(
            &mut differences,
            observed_summary.return_skewness,
            model_summary.return_skewness,
        );

        push_unit_metric_difference(
            &mut differences,
            observed_summary.return_excess_kurtosis,
            model_summary.return_excess_kurtosis,
        );

        push_unit_metric_difference(
            &mut differences,
            observed_summary.activity_variance_correlation,
            model_summary.activity_variance_correlation,
        );

        push_unit_metric_difference(
            &mut differences,
            observed_summary.signed_activity_future_variance_relation,
            model_summary.signed_activity_future_variance_relation,
        );
    }

    root_mean_square(differences.as_slice())
}

pub(super) fn add_long_variance_horizon_terms(
    builder: &mut CalibrationLossBuilder,
    seed_summaries: &[JointRefinementSeedSummary],
    observed_data: &JointRefinementObservedData,
) {
    for (horizon_position, observed_summary) in observed_data
        .observed_long_variance_horizon_summaries
        .iter()
        .enumerate()
    {
        let horizon_label = LONG_VARIANCE_HORIZON_DEFINITIONS
            .get(horizon_position)
            .map(|definition| definition.label)
            .unwrap_or("custom");

        let model_summaries = seed_summaries
            .iter()
            .filter_map(|seed_summary| {
                seed_summary
                    .simulated_long_variance_horizon_summaries
                    .get(horizon_position)
                    .copied()
            })
            .collect::<Vec<_>>();

        add_metric_term(
            builder,
            horizon_label,
            "realized_variance",
            observed_summary.realized_variance,
            observed_summary.standard_errors.realized_variance,
            model_summaries.as_slice(),
            |summary| summary.realized_variance,
            |standard_errors| standard_errors.realized_variance,
        );

        add_metric_term(
            builder,
            horizon_label,
            "variance_of_variance",
            observed_summary.variance_of_variance,
            observed_summary.standard_errors.variance_of_variance,
            model_summaries.as_slice(),
            |summary| summary.variance_of_variance,
            |standard_errors| standard_errors.variance_of_variance,
        );

        add_metric_term(
            builder,
            horizon_label,
            "return_skewness",
            observed_summary.return_skewness,
            observed_summary.standard_errors.return_skewness,
            model_summaries.as_slice(),
            |summary| summary.return_skewness,
            |standard_errors| standard_errors.return_skewness,
        );

        add_metric_term(
            builder,
            horizon_label,
            "return_excess_kurtosis",
            observed_summary.return_excess_kurtosis,
            observed_summary.standard_errors.return_excess_kurtosis,
            model_summaries.as_slice(),
            |summary| summary.return_excess_kurtosis,
            |standard_errors| standard_errors.return_excess_kurtosis,
        );

        add_metric_term(
            builder,
            horizon_label,
            "activity_variance_correlation",
            observed_summary.activity_variance_correlation,
            observed_summary
                .standard_errors
                .activity_variance_correlation,
            model_summaries.as_slice(),
            |summary| summary.activity_variance_correlation,
            |standard_errors| standard_errors.activity_variance_correlation,
        );

        add_metric_term(
            builder,
            horizon_label,
            "signed_activity_future_variance_relation",
            observed_summary.signed_activity_future_variance_relation,
            observed_summary
                .standard_errors
                .signed_activity_future_variance_relation,
            model_summaries.as_slice(),
            |summary| summary.signed_activity_future_variance_relation,
            |standard_errors| standard_errors.signed_activity_future_variance_relation,
        );
    }
}

fn add_metric_term(
    builder: &mut CalibrationLossBuilder,
    horizon_label: &str,
    metric_name: &str,
    observed_value: Option<f64>,
    observed_standard_error: Option<f64>,
    model_summaries: &[LongVarianceHorizonMomentSummary],
    extract_metric: fn(LongVarianceHorizonMomentSummary) -> Option<f64>,
    extract_standard_error: fn(LongVarianceHorizonMomentStandardErrors) -> Option<f64>,
) {
    let Some(observed_value) = observed_value else {
        return;
    };

    let Some(observed_standard_error) = finite_positive_standard_error(observed_standard_error)
    else {
        return;
    };

    let Some(model_estimate) =
        average_summary_metric_estimate(model_summaries, extract_metric, extract_standard_error)
    else {
        return;
    };

    add_direct_term(
        builder,
        format!(
            "{LONG_VARIANCE_OBJECTIVE_FAMILY_ID}.{}.{}",
            horizon_label, metric_name
        )
        .as_str(),
        LONG_VARIANCE_OBJECTIVE_FAMILY_ID,
        observed_value,
        model_estimate.value,
        observed_standard_error,
        model_estimate.standard_error,
    );
}

fn average_summary_metric_estimate(
    summaries: &[LongVarianceHorizonMomentSummary],
    extract_metric: fn(LongVarianceHorizonMomentSummary) -> Option<f64>,
    extract_standard_error: fn(LongVarianceHorizonMomentStandardErrors) -> Option<f64>,
) -> Option<MeanEstimate> {
    let estimates = summaries
        .iter()
        .copied()
        .filter_map(|summary| {
            let value = extract_metric(summary)?;
            let standard_error =
                finite_positive_standard_error(extract_standard_error(summary.standard_errors))?;

            value.is_finite().then_some((value, standard_error))
        })
        .collect::<Vec<_>>();

    if estimates.is_empty() {
        return None;
    }

    let values = estimates
        .iter()
        .map(|(value, _)| *value)
        .collect::<Vec<_>>();

    let standard_errors = estimates
        .iter()
        .map(|(_, standard_error)| *standard_error)
        .collect::<Vec<_>>();

    let pooled_estimate =
        mean_estimate_from_standard_errors(values.as_slice(), standard_errors.as_slice())?;

    pooled_estimate
        .standard_error
        .is_finite()
        .then_some(pooled_estimate)
}

fn finite_positive_standard_error(standard_error: Option<f64>) -> Option<f64> {
    standard_error.filter(|value| value.is_finite() && *value > 0.0)
}

fn push_positive_metric_difference(
    differences: &mut Vec<f64>,
    observed_value: Option<f64>,
    model_value: Option<f64>,
) {
    if let (Some(observed_value), Some(model_value)) = (observed_value, model_value)
        && observed_value.is_finite()
        && model_value.is_finite()
        && observed_value >= 0.0
        && model_value >= 0.0
    {
        differences.push(log_ratio_distance(
            model_value,
            observed_value,
            observed_value,
        ));
    }
}

fn push_unit_metric_difference(
    differences: &mut Vec<f64>,
    observed_value: Option<f64>,
    model_value: Option<f64>,
) {
    if let (Some(observed_value), Some(model_value)) = (observed_value, model_value)
        && observed_value.is_finite()
        && model_value.is_finite()
    {
        differences.push((model_value - observed_value).abs());
    }
}
