use crate::objective::CalibrationLossBuilder;
use crate::stages::joint_metrics::log_ratio_distance;
use crate::stages::joint_refinement::JointRefinementSeedSummary;
use crate::stages::joint_refinement::execution::JointRefinementObservedData;
use crate::stages::joint_refinement::objective::{
    add_objective_family_from_seed_vectors, diagonal_covariance_from_standard_errors,
    root_mean_square,
};
use crate::stages::long_variance::metrics::{
    LONG_VARIANCE_HORIZON_DEFINITIONS, LongVarianceHorizonMomentSummary,
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

        add_long_variance_horizon_family(
            builder,
            horizon_label,
            *observed_summary,
            model_summaries.as_slice(),
        );
    }
}

fn add_long_variance_horizon_family(
    builder: &mut CalibrationLossBuilder,
    horizon_label: &str,
    observed_summary: LongVarianceHorizonMomentSummary,
    model_summaries: &[LongVarianceHorizonMomentSummary],
) {
    let metrics = [
        LongVarianceMetricDescriptor {
            metric_name: "realized_variance",
            observed_value: observed_summary.realized_variance,
            observed_standard_error: observed_summary.standard_errors.realized_variance,
            extract_model_value: |summary| summary.realized_variance,
        },
        LongVarianceMetricDescriptor {
            metric_name: "variance_of_variance",
            observed_value: observed_summary.variance_of_variance,
            observed_standard_error: observed_summary.standard_errors.variance_of_variance,
            extract_model_value: |summary| summary.variance_of_variance,
        },
        LongVarianceMetricDescriptor {
            metric_name: "return_skewness",
            observed_value: observed_summary.return_skewness,
            observed_standard_error: observed_summary.standard_errors.return_skewness,
            extract_model_value: |summary| summary.return_skewness,
        },
        LongVarianceMetricDescriptor {
            metric_name: "return_excess_kurtosis",
            observed_value: observed_summary.return_excess_kurtosis,
            observed_standard_error: observed_summary.standard_errors.return_excess_kurtosis,
            extract_model_value: |summary| summary.return_excess_kurtosis,
        },
        LongVarianceMetricDescriptor {
            metric_name: "activity_variance_correlation",
            observed_value: observed_summary.activity_variance_correlation,
            observed_standard_error: observed_summary
                .standard_errors
                .activity_variance_correlation,
            extract_model_value: |summary| summary.activity_variance_correlation,
        },
        LongVarianceMetricDescriptor {
            metric_name: "signed_activity_future_variance_relation",
            observed_value: observed_summary.signed_activity_future_variance_relation,
            observed_standard_error: observed_summary
                .standard_errors
                .signed_activity_future_variance_relation,
            extract_model_value: |summary| summary.signed_activity_future_variance_relation,
        },
    ];

    let identified_metrics = metrics
        .into_iter()
        .filter(|metric| {
            metric.observed_value.is_some_and(f64::is_finite)
                && metric
                    .observed_standard_error
                    .is_some_and(|standard_error| {
                        standard_error.is_finite() && standard_error > 0.0
                    })
        })
        .collect::<Vec<_>>();

    if identified_metrics.is_empty() {
        return;
    }

    let observed_vector = identified_metrics
        .iter()
        .filter_map(|metric| metric.observed_value)
        .collect::<Vec<_>>();
    let observed_standard_errors = identified_metrics
        .iter()
        .filter_map(|metric| metric.observed_standard_error)
        .collect::<Vec<_>>();

    let Some(observed_covariance_rows) =
        diagonal_covariance_from_standard_errors(observed_standard_errors.as_slice())
    else {
        return;
    };

    let model_seed_vectors = model_summaries
        .iter()
        .filter_map(|summary| {
            identified_metrics
                .iter()
                .map(|metric| {
                    (metric.extract_model_value)(*summary).filter(|value| value.is_finite())
                })
                .collect::<Option<Vec<_>>>()
        })
        .collect::<Vec<_>>();

    let family_id = format!("{LONG_VARIANCE_OBJECTIVE_FAMILY_ID}.{horizon_label}");
    let coordinate_ids = identified_metrics
        .iter()
        .map(|metric| format!("{family_id}.{}", metric.metric_name))
        .collect::<Vec<_>>();

    add_objective_family_from_seed_vectors(
        builder,
        family_id,
        coordinate_ids,
        observed_vector,
        observed_covariance_rows,
        model_seed_vectors,
    );
}

#[derive(Debug, Clone, Copy)]
struct LongVarianceMetricDescriptor {
    metric_name: &'static str,
    observed_value: Option<f64>,
    observed_standard_error: Option<f64>,
    extract_model_value: fn(LongVarianceHorizonMomentSummary) -> Option<f64>,
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
