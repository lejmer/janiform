use janiform_foundations::numeric::clamp_point_in_place as clamp_in_place;

use super::super::super::contracts::JointRefinementStageObjectiveTermDiagnostic;
use super::super::super::execution::JointRefinementActiveParameterLayout;
use super::activity::apply_activity_alignment;
use super::nig_candidate_seeds::apply_nig_log_delta_offset_shift;
const RETURN_SCALE_REALIZED_VARIANCE_1D_METRIC_ID: &str = "return_scale.1d.realized_variance";
const RETURN_SCALE_MEAN_ABSOLUTE_RETURN_1D_METRIC_ID: &str = "return_scale.1d.mean_absolute_return";
const RETURN_SCALE_TICK_REALIZED_VARIANCE_METRIC_ID: &str = "return_scale.tick.realized_variance";
const RETURN_SCALE_TICK_MEAN_ABSOLUTE_RETURN_METRIC_ID: &str =
    "return_scale.tick.mean_absolute_return";
const RETURN_SCALE_ALIGNMENT_METRICS: [ReturnScaleAlignmentMetric; 4] = [
    ReturnScaleAlignmentMetric {
        metric_id: RETURN_SCALE_REALIZED_VARIANCE_1D_METRIC_ID,
        seed_label: "realized_variance_1d",
        scale_mapping: ReturnScaleAlignmentScaleMapping::Quadratic,
    },
    ReturnScaleAlignmentMetric {
        metric_id: RETURN_SCALE_MEAN_ABSOLUTE_RETURN_1D_METRIC_ID,
        seed_label: "mean_absolute_return_1d",
        scale_mapping: ReturnScaleAlignmentScaleMapping::Linear,
    },
    ReturnScaleAlignmentMetric {
        metric_id: RETURN_SCALE_TICK_REALIZED_VARIANCE_METRIC_ID,
        seed_label: "tick_realized_variance",
        scale_mapping: ReturnScaleAlignmentScaleMapping::Quadratic,
    },
    ReturnScaleAlignmentMetric {
        metric_id: RETURN_SCALE_TICK_MEAN_ABSOLUTE_RETURN_METRIC_ID,
        seed_label: "tick_mean_absolute_return",
        scale_mapping: ReturnScaleAlignmentScaleMapping::Linear,
    },
];

#[derive(Debug, Clone, Copy)]
struct ReturnScaleAlignmentMetric {
    metric_id: &'static str,
    seed_label: &'static str,
    scale_mapping: ReturnScaleAlignmentScaleMapping,
}

#[derive(Debug, Clone, Copy)]
enum ReturnScaleAlignmentScaleMapping {
    Linear,
    Quadratic,
}

#[derive(Debug, Clone, Copy)]
struct ReturnScaleAlignmentLogDeltaShift {
    seed_label: &'static str,
    log_delta_shift: f64,
}
pub(super) struct JointReturnScaleAlignmentSeedsBuildInput<'a> {
    pub(super) baseline_parameter_vector: &'a [f64],
    pub(super) active_parameter_layout: &'a JointRefinementActiveParameterLayout,
    pub(super) lower_bounds: &'a [f64],
    pub(super) upper_bounds: &'a [f64],
    pub(super) baseline_final_objective_terms: &'a [JointRefinementStageObjectiveTermDiagnostic],
    pub(super) activity_log_shift: f64,
}

pub(super) fn joint_return_scale_alignment_candidate_seeds(
    input: JointReturnScaleAlignmentSeedsBuildInput<'_>,
) -> Vec<(String, Vec<f64>)> {
    let JointReturnScaleAlignmentSeedsBuildInput {
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_final_objective_terms,
        activity_log_shift,
    } = input;

    return_scale_alignment_log_delta_shifts(baseline_final_objective_terms)
        .into_iter()
        .flat_map(|alignment| {
            let mut baseline_scale_aligned = baseline_parameter_vector.to_vec();

            apply_nig_log_delta_offset_shift(
                baseline_scale_aligned.as_mut_slice(),
                active_parameter_layout,
                alignment.log_delta_shift,
            );

            clamp_in_place(
                baseline_scale_aligned.as_mut_slice(),
                lower_bounds,
                upper_bounds,
            );

            let mut activity_scale_aligned = baseline_parameter_vector.to_vec();

            apply_nig_log_delta_offset_shift(
                activity_scale_aligned.as_mut_slice(),
                active_parameter_layout,
                alignment.log_delta_shift,
            );

            apply_activity_alignment(
                activity_scale_aligned.as_mut_slice(),
                active_parameter_layout,
                activity_log_shift,
            );

            clamp_in_place(
                activity_scale_aligned.as_mut_slice(),
                lower_bounds,
                upper_bounds,
            );

            vec![
                (
                    format!("return_scale_{}", alignment.seed_label),
                    baseline_scale_aligned,
                ),
                (
                    format!("return_scale_{}_activity_aligned", alignment.seed_label),
                    activity_scale_aligned,
                ),
            ]
        })
        .collect()
}

fn return_scale_alignment_log_delta_shifts(
    objective_terms: &[JointRefinementStageObjectiveTermDiagnostic],
) -> Vec<ReturnScaleAlignmentLogDeltaShift> {
    RETURN_SCALE_ALIGNMENT_METRICS
        .iter()
        .filter_map(|alignment_metric| {
            objective_terms
                .iter()
                .find(|term| term.term_id == alignment_metric.metric_id)
                .and_then(|term| return_scale_alignment_ratio(term, alignment_metric.scale_mapping))
                .map(|scale_ratio| ReturnScaleAlignmentLogDeltaShift {
                    seed_label: alignment_metric.seed_label,
                    log_delta_shift: scale_ratio.ln(),
                })
        })
        .collect()
}

fn return_scale_alignment_ratio(
    objective_term: &JointRefinementStageObjectiveTermDiagnostic,
    scale_mapping: ReturnScaleAlignmentScaleMapping,
) -> Option<f64> {
    let observed_value = objective_term.observed_value?;
    let model_value = objective_term.model_value?;

    if !observed_value.is_finite()
        || !model_value.is_finite()
        || observed_value <= 0.0
        || model_value <= 0.0
    {
        return None;
    }

    let observed_to_simulated_ratio = observed_value / model_value;
    if !observed_to_simulated_ratio.is_finite() || observed_to_simulated_ratio <= 0.0 {
        return None;
    }

    let scale_ratio = match scale_mapping {
        ReturnScaleAlignmentScaleMapping::Linear => observed_to_simulated_ratio,
        ReturnScaleAlignmentScaleMapping::Quadratic => observed_to_simulated_ratio.sqrt(),
    };

    scale_ratio.is_finite().then_some(scale_ratio)
}
