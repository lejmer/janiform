//! Scenario-level metric delta computation for scale consistency.

use super::contracts::{
    SCALE_CONSISTENCY_METRIC_DELTA_BATCH_REPORT_SCHEMA_VERSION,
    SCALE_CONSISTENCY_METRIC_DELTA_ORDERING_POLICY, ScaleConsistencyMetricComputationFailure,
    ScaleConsistencyMetricComputationStatus, ScaleConsistencyMetricDatasetRole,
    ScaleConsistencyMetricDelta, ScaleConsistencyMetricDeltaBatchError,
    ScaleConsistencyMetricDeltaBatchReport, ScaleConsistencyMetricDeltaBatchRequest,
    ScaleConsistencyMetricDeltaKind, ScaleConsistencyMetricDeltaScenarioReport,
    ScaleConsistencyMetricScenarioInput,
};
use super::resolution::{
    PairwiseMetricKind, ResolvedMetricKind, ResolvedMetricSpecification, ScalarMetricKind,
    resolve_metric_specifications,
};
use super::statistics::{
    PairwiseMetricResult, compute_abs_return_acf_distribution_distance,
    compute_closed_gap_abs_tail_expected_shortfall_ratio,
    compute_closed_gap_vs_open_jump_wasserstein_distance,
    compute_open_jump_abs_tail_expected_shortfall_ratio,
    compute_return_distribution_wasserstein_distance, compute_returns_central_variance,
};
pub fn calculate_scale_consistency_metric_deltas(
    request: &ScaleConsistencyMetricDeltaBatchRequest,
) -> Result<ScaleConsistencyMetricDeltaBatchReport, ScaleConsistencyMetricDeltaBatchError> {
    let resolved_metric_specifications = resolve_metric_specifications(request)?;

    let scenario_reports = request
        .scenarios
        .iter()
        .map(|scenario| compute_scenario_report(scenario, &resolved_metric_specifications))
        .collect();

    Ok(ScaleConsistencyMetricDeltaBatchReport {
        schema_version: SCALE_CONSISTENCY_METRIC_DELTA_BATCH_REPORT_SCHEMA_VERSION,
        ordering_policy: SCALE_CONSISTENCY_METRIC_DELTA_ORDERING_POLICY,
        scenario_reports,
    })
}

fn compute_scenario_report(
    scenario: &ScaleConsistencyMetricScenarioInput,
    resolved_metric_specifications: &[ResolvedMetricSpecification],
) -> ScaleConsistencyMetricDeltaScenarioReport {
    let coarse_interval_delta = scenario.coarse_path.interval_delta;
    let fine_interval_delta = scenario
        .aligned_fine_path
        .alignment_metadata
        .fine_interval_delta;
    let horizon_interval_count = scenario.coarse_path.simulated_interval_count;
    let fine_subinterval_count_per_coarse_interval = scenario
        .aligned_fine_path
        .alignment_metadata
        .fine_subinterval_count_per_coarse_interval;

    let metric_deltas = resolved_metric_specifications
        .iter()
        .map(|metric_specification| {
            compute_metric_delta(
                metric_specification,
                scenario,
                coarse_interval_delta,
                fine_interval_delta,
                horizon_interval_count,
            )
        })
        .collect();

    ScaleConsistencyMetricDeltaScenarioReport {
        scenario_id: scenario.scenario_id.clone(),
        parameter_set_id: scenario.parameter_set_id.clone(),
        horizon_label: scenario.horizon_label.clone(),
        horizon_interval_count,
        coarse_interval_delta,
        fine_interval_delta,
        fine_subinterval_count_per_coarse_interval,
        metric_deltas,
    }
}

fn compute_metric_delta(
    metric_specification: &ResolvedMetricSpecification,
    scenario: &ScaleConsistencyMetricScenarioInput,
    coarse_interval_delta: f64,
    fine_interval_delta: f64,
    horizon_interval_count: usize,
) -> ScaleConsistencyMetricDelta {
    match &metric_specification.metric_kind {
        ResolvedMetricKind::ScalarDifference(metric_kind) => compute_scalar_metric_delta(
            metric_specification,
            *metric_kind,
            scenario,
            coarse_interval_delta,
            fine_interval_delta,
            horizon_interval_count,
        ),
        ResolvedMetricKind::PairwiseDistance(metric_kind) => compute_pairwise_metric_delta(
            metric_specification,
            metric_kind,
            scenario,
            coarse_interval_delta,
            fine_interval_delta,
            horizon_interval_count,
        ),
    }
}

fn compute_scalar_metric_delta(
    metric_specification: &ResolvedMetricSpecification,
    metric_kind: ScalarMetricKind,
    scenario: &ScaleConsistencyMetricScenarioInput,
    coarse_interval_delta: f64,
    fine_interval_delta: f64,
    horizon_interval_count: usize,
) -> ScaleConsistencyMetricDelta {
    let coarse_metric_result = match metric_kind {
        ScalarMetricKind::ReturnsCentralVariance => {
            compute_returns_central_variance(&scenario.coarse_path)
        }
        ScalarMetricKind::OpenJumpAbsTailExpectedShortfallRatio => {
            compute_open_jump_abs_tail_expected_shortfall_ratio(&scenario.coarse_path)
        }
        ScalarMetricKind::ClosedGapAbsTailExpectedShortfallRatio => {
            compute_closed_gap_abs_tail_expected_shortfall_ratio(&scenario.coarse_path)
        }
        ScalarMetricKind::ClosedGapVsOpenJumpWassersteinDistance => {
            compute_closed_gap_vs_open_jump_wasserstein_distance(&scenario.coarse_path)
        }
    }
    .with_dataset_role(ScaleConsistencyMetricDatasetRole::Coarse);

    let fine_metric_result = match metric_kind {
        ScalarMetricKind::ReturnsCentralVariance => {
            compute_returns_central_variance(&scenario.aligned_fine_path.aggregated_path)
        }
        ScalarMetricKind::OpenJumpAbsTailExpectedShortfallRatio => {
            compute_open_jump_abs_tail_expected_shortfall_ratio(
                &scenario.aligned_fine_path.aggregated_path,
            )
        }
        ScalarMetricKind::ClosedGapAbsTailExpectedShortfallRatio => {
            compute_closed_gap_abs_tail_expected_shortfall_ratio(
                &scenario.aligned_fine_path.aggregated_path,
            )
        }
        ScalarMetricKind::ClosedGapVsOpenJumpWassersteinDistance => {
            compute_closed_gap_vs_open_jump_wasserstein_distance(
                &scenario.aligned_fine_path.aggregated_path,
            )
        }
    }
    .with_dataset_role(ScaleConsistencyMetricDatasetRole::Fine);

    let (status, coarse_value, fine_value, delta_value, absolute_delta, computation_failures) =
        match (coarse_metric_result.value, fine_metric_result.value) {
            (Some(coarse_value), Some(fine_value)) => {
                let delta_value = coarse_value - fine_value;
                (
                    ScaleConsistencyMetricComputationStatus::Computed,
                    Some(coarse_value),
                    Some(fine_value),
                    Some(delta_value),
                    Some(delta_value.abs()),
                    Vec::new(),
                )
            }
            _ => {
                let mut computation_failures = coarse_metric_result.failures;
                computation_failures.extend(fine_metric_result.failures);
                (
                    ScaleConsistencyMetricComputationStatus::Missing,
                    coarse_metric_result.value,
                    fine_metric_result.value,
                    None,
                    None,
                    computation_failures,
                )
            }
        };

    build_metric_delta(
        metric_specification,
        ScaleConsistencyMetricDeltaKind::SignedDifference,
        scenario,
        coarse_interval_delta,
        fine_interval_delta,
        horizon_interval_count,
        status,
        coarse_value,
        fine_value,
        delta_value,
        absolute_delta,
        computation_failures,
    )
}

fn compute_pairwise_metric_delta(
    metric_specification: &ResolvedMetricSpecification,
    metric_kind: &PairwiseMetricKind,
    scenario: &ScaleConsistencyMetricScenarioInput,
    coarse_interval_delta: f64,
    fine_interval_delta: f64,
    horizon_interval_count: usize,
) -> ScaleConsistencyMetricDelta {
    let pairwise_result = match metric_kind {
        PairwiseMetricKind::ReturnsDistributionWassersteinDistance => {
            compute_return_distribution_wasserstein_distance(
                &scenario.coarse_path,
                &scenario.aligned_fine_path.aggregated_path,
            )
        }
        PairwiseMetricKind::VolatilityAbsReturnAcfDistributionDistance {
            lag_interval_counts,
        } => compute_abs_return_acf_distribution_distance(
            &scenario.coarse_path,
            &scenario.aligned_fine_path.aggregated_path,
            lag_interval_counts,
        ),
    };

    let (status, delta_value, computation_failures) = match pairwise_result {
        PairwiseMetricResult::Computed { distance } => (
            ScaleConsistencyMetricComputationStatus::Computed,
            Some(distance),
            Vec::new(),
        ),
        PairwiseMetricResult::Missing { failures } => (
            ScaleConsistencyMetricComputationStatus::Missing,
            None,
            failures,
        ),
    };

    build_metric_delta(
        metric_specification,
        ScaleConsistencyMetricDeltaKind::AbsoluteDistance,
        scenario,
        coarse_interval_delta,
        fine_interval_delta,
        horizon_interval_count,
        status,
        None,
        None,
        delta_value,
        delta_value,
        computation_failures,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_metric_delta(
    metric_specification: &ResolvedMetricSpecification,
    delta_kind: ScaleConsistencyMetricDeltaKind,
    scenario: &ScaleConsistencyMetricScenarioInput,
    coarse_interval_delta: f64,
    fine_interval_delta: f64,
    horizon_interval_count: usize,
    status: ScaleConsistencyMetricComputationStatus,
    coarse_value: Option<f64>,
    fine_value: Option<f64>,
    delta_value: Option<f64>,
    absolute_delta: Option<f64>,
    computation_failures: Vec<ScaleConsistencyMetricComputationFailure>,
) -> ScaleConsistencyMetricDelta {
    ScaleConsistencyMetricDelta {
        metric_id: metric_specification.metric_id.clone(),
        metric_name: Some(metric_specification.metric_name.clone()),
        metric_variant: metric_specification.metric_variant.clone(),
        metric_category: metric_specification.metric_category,
        delta_kind,
        horizon_label: scenario.horizon_label.clone(),
        horizon_interval_count,
        coarse_interval_delta,
        fine_interval_delta,
        status,
        coarse_value,
        fine_value,
        delta_value,
        absolute_delta,
        computation_failures,
    }
}
