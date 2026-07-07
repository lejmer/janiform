use std::sync::Arc;

use janiform_foundations::numeric::{compute_sorted_quantile_or_zero, sorted_finite_values};

use crate::objective::CalibrationLossBuilder;
use crate::objective_families::{
    block_diagonal_covariance, covariance_from_delete_block_vectors, diagonal_covariance,
};
use crate::stages::joint_refinement::execution::{
    JointRefinementContext, JointRefinementObservedReturnShapeStatisticCacheKey,
    JointRefinementReturnShapeHorizon, JointRefinementSeedSummary,
};
use crate::stages::joint_refinement::objective::{
    add_objective_family_from_seed_vectors, binomial_share_standard_error,
    cached_observed_return_shape_covariance, cached_observed_return_shape_statistic_estimates,
    diagonal_covariance_from_standard_errors, estimate_vector_by_metric_names,
    return_shape_delete_block_vectors, return_shape_effective_sample_count,
    return_shape_horizon_label, return_shape_horizon_term_inputs, return_shape_statistic_estimates,
    statistic_estimate_by_name,
};

// Tick-horizon shape uses empirical probabilities at observed signed and
// absolute-return cut points covering central mass, shoulders, and tails.
const TICK_SIGNED_CDF_CUTPOINTS: [TickShapeProbabilityCutpoint; 7] = [
    TickShapeProbabilityCutpoint::signed("cdf_at_observed_q01", 0.01),
    TickShapeProbabilityCutpoint::signed("cdf_at_observed_q05", 0.05),
    TickShapeProbabilityCutpoint::signed("cdf_at_observed_q25", 0.25),
    TickShapeProbabilityCutpoint::signed("cdf_at_observed_q50", 0.50),
    TickShapeProbabilityCutpoint::signed("cdf_at_observed_q75", 0.75),
    TickShapeProbabilityCutpoint::signed("cdf_at_observed_q95", 0.95),
    TickShapeProbabilityCutpoint::signed("cdf_at_observed_q99", 0.99),
];

const TICK_ABSOLUTE_CDF_CUTPOINTS: [TickShapeProbabilityCutpoint; 5] = [
    TickShapeProbabilityCutpoint::absolute("absolute_cdf_at_observed_q50", 0.50),
    TickShapeProbabilityCutpoint::absolute("absolute_cdf_at_observed_q75", 0.75),
    TickShapeProbabilityCutpoint::absolute("absolute_cdf_at_observed_q90", 0.90),
    TickShapeProbabilityCutpoint::absolute("absolute_cdf_at_observed_q95", 0.95),
    TickShapeProbabilityCutpoint::absolute("absolute_cdf_at_observed_q99", 0.99),
];

const RETURN_SHAPE_LOCATION_FAMILY_ID: &str = "return_shape.location";
const RETURN_SHAPE_BODY_SCALE_FAMILY_ID: &str = "return_shape.body_scale";
const RETURN_SHAPE_CENTER_CONCENTRATION_FAMILY_ID: &str = "return_shape.center_concentration";
const RETURN_SHAPE_TAIL_MASS_FAMILY_ID: &str = "return_shape.tail_mass";
const RETURN_SHAPE_TAIL_ASYMMETRY_FAMILY_ID: &str = "return_shape.tail_asymmetry";
const RETURN_SHAPE_TAIL_SHAPE_FAMILY_ID: &str = "return_shape.tail_shape";

const RETURN_SHAPE_MECHANISM_FAMILY_IDS: [&str; 6] = [
    RETURN_SHAPE_LOCATION_FAMILY_ID,
    RETURN_SHAPE_BODY_SCALE_FAMILY_ID,
    RETURN_SHAPE_CENTER_CONCENTRATION_FAMILY_ID,
    RETURN_SHAPE_TAIL_MASS_FAMILY_ID,
    RETURN_SHAPE_TAIL_ASYMMETRY_FAMILY_ID,
    RETURN_SHAPE_TAIL_SHAPE_FAMILY_ID,
];

const AGGREGATED_RETURN_SHAPE_MECHANISM_GROUPS: [ReturnShapeMetricGroup; 6] = [
    ReturnShapeMetricGroup {
        family_id: RETURN_SHAPE_LOCATION_FAMILY_ID,
        metric_names: &["mean"],
    },
    ReturnShapeMetricGroup {
        family_id: RETURN_SHAPE_BODY_SCALE_FAMILY_ID,
        metric_names: &[
            "winsorized_volatility_q05_q95",
            "mean_abs",
            "median_abs",
            "l_moment_lambda2",
        ],
    },
    ReturnShapeMetricGroup {
        family_id: RETURN_SHAPE_CENTER_CONCENTRATION_FAMILY_ID,
        metric_names: &["q25", "median", "q75"],
    },
    ReturnShapeMetricGroup {
        family_id: RETURN_SHAPE_TAIL_MASS_FAMILY_ID,
        metric_names: &["q01", "q05", "q95", "q99"],
    },
    ReturnShapeMetricGroup {
        family_id: RETURN_SHAPE_TAIL_ASYMMETRY_FAMILY_ID,
        metric_names: &["bowley_skewness", "l_moment_tau3"],
    },
    ReturnShapeMetricGroup {
        family_id: RETURN_SHAPE_TAIL_SHAPE_FAMILY_ID,
        metric_names: &["l_moment_tau4", "excess_kurtosis"],
    },
];

const TICK_RETURN_SHAPE_MECHANISM_GROUPS: [ReturnShapeMetricGroup; 2] = [
    ReturnShapeMetricGroup {
        family_id: RETURN_SHAPE_CENTER_CONCENTRATION_FAMILY_ID,
        metric_names: &[
            "cdf_at_observed_q25",
            "cdf_at_observed_q50",
            "cdf_at_observed_q75",
            "absolute_cdf_at_observed_q50",
            "absolute_cdf_at_observed_q75",
        ],
    },
    ReturnShapeMetricGroup {
        family_id: RETURN_SHAPE_TAIL_MASS_FAMILY_ID,
        metric_names: &[
            "cdf_at_observed_q01",
            "cdf_at_observed_q05",
            "cdf_at_observed_q95",
            "cdf_at_observed_q99",
            "absolute_cdf_at_observed_q90",
            "absolute_cdf_at_observed_q95",
            "absolute_cdf_at_observed_q99",
        ],
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReturnShapeMetricGroup {
    family_id: &'static str,
    metric_names: &'static [&'static str],
}

#[derive(Debug, Clone)]
struct ReturnShapeObjectiveFamilyBlock {
    family_id: &'static str,
    horizon_label: &'static str,
    metric_names: Vec<&'static str>,
    observed_vector: Vec<f64>,
    observed_covariance_rows: Vec<Vec<f64>>,
    model_seed_vectors: Vec<Vec<f64>>,
}

#[derive(Debug, Clone)]
struct ReturnShapeObjectiveFamilyAccumulator {
    family_id: &'static str,
    coordinate_ids: Vec<String>,
    observed_vector: Vec<f64>,
    observed_covariance_blocks: Vec<Vec<Vec<f64>>>,
    model_seed_vectors: Option<Vec<Vec<f64>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TickShapeProbabilityKind {
    Signed,
    Absolute,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TickShapeProbabilityCutpoint {
    metric_name: &'static str,
    quantile_level: f64,
    kind: TickShapeProbabilityKind,
}

impl TickShapeProbabilityCutpoint {
    const fn signed(metric_name: &'static str, quantile_level: f64) -> Self {
        Self {
            metric_name,
            quantile_level,
            kind: TickShapeProbabilityKind::Signed,
        }
    }

    const fn absolute(metric_name: &'static str, quantile_level: f64) -> Self {
        Self {
            metric_name,
            quantile_level,
            kind: TickShapeProbabilityKind::Absolute,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TickShapeProbabilitySpecification {
    metric_name: &'static str,
    threshold: f64,
    kind: TickShapeProbabilityKind,
}

pub(super) fn add_return_shape_terms(
    builder: &mut CalibrationLossBuilder,
    seed_summaries: &[JointRefinementSeedSummary],
    context: &JointRefinementContext,
) {
    let observed_data = &context.observed_data;
    let temporal_block_count = observed_data.hourly_simulation_interval_count;
    let mut accumulators = return_shape_objective_family_accumulators();

    for horizon_input in return_shape_horizon_term_inputs(seed_summaries, observed_data) {
        let blocks = return_shape_horizon_family_blocks(
            horizon_input.horizon,
            temporal_block_count,
            horizon_input.observed_values,
            horizon_input.simulated_values_by_seed.as_slice(),
            context,
        );

        for block in blocks {
            if let Some(accumulator) = accumulators
                .iter_mut()
                .find(|accumulator| accumulator.family_id == block.family_id)
            {
                accumulator.append(block);
            }
        }
    }

    for accumulator in accumulators {
        accumulator.add_to_builder(builder);
    }
}

fn return_shape_horizon_family_blocks(
    horizon: JointRefinementReturnShapeHorizon,
    temporal_block_count: usize,
    observed_values: &[f64],
    simulated_values_by_seed: &[&[f64]],
    context: &JointRefinementContext,
) -> Vec<ReturnShapeObjectiveFamilyBlock> {
    if horizon == JointRefinementReturnShapeHorizon::Tick {
        return tick_return_shape_probability_blocks(
            temporal_block_count,
            observed_values,
            simulated_values_by_seed,
            context,
        );
    }

    aggregated_return_shape_family_blocks(
        horizon,
        temporal_block_count,
        observed_values,
        simulated_values_by_seed,
        context,
    )
}

fn tick_return_shape_probability_blocks(
    temporal_block_count: usize,
    observed_values: &[f64],
    simulated_values_by_seed: &[&[f64]],
    context: &JointRefinementContext,
) -> Vec<ReturnShapeObjectiveFamilyBlock> {
    let Some(specifications) = tick_shape_probability_specifications(observed_values) else {
        return Vec::new();
    };

    let mut blocks = Vec::new();

    for group in TICK_RETURN_SHAPE_MECHANISM_GROUPS {
        let group_specifications = specifications
            .iter()
            .copied()
            .filter(|specification| group.metric_names.contains(&specification.metric_name))
            .collect::<Vec<_>>();
        if group_specifications.is_empty() {
            continue;
        }

        let Some(observed_vector) =
            tick_shape_probability_vector(observed_values, group_specifications.as_slice())
        else {
            continue;
        };

        let Some(observed_covariance_rows) = cached_observed_tick_shape_probability_covariance(
            temporal_block_count,
            observed_values,
            group_specifications.as_slice(),
            observed_vector.as_slice(),
            context,
        )
        .map(|covariance_rows| {
            // Objective-family blocks own covariance rows so cache entries can stay shared.
            covariance_rows.as_ref().clone()
        })
        .or_else(|| {
            let finite_observed_value_count = observed_values
                .iter()
                .filter(|value| value.is_finite())
                .count();

            tick_shape_probability_diagonal_covariance(
                observed_vector.as_slice(),
                finite_observed_value_count,
                temporal_block_count,
            )
        }) else {
            continue;
        };

        let Some(model_seed_vectors) = simulated_values_by_seed
            .iter()
            .map(|values| tick_shape_probability_vector(values, group_specifications.as_slice()))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };

        blocks.push(ReturnShapeObjectiveFamilyBlock {
            family_id: group.family_id,
            horizon_label: "tick",
            metric_names: group_specifications
                .iter()
                .map(|specification| specification.metric_name)
                .collect(),
            observed_vector,
            observed_covariance_rows,
            model_seed_vectors,
        });
    }

    blocks
}

fn aggregated_return_shape_family_blocks(
    horizon: JointRefinementReturnShapeHorizon,
    temporal_block_count: usize,
    observed_values: &[f64],
    simulated_values_by_seed: &[&[f64]],
    context: &JointRefinementContext,
) -> Vec<ReturnShapeObjectiveFamilyBlock> {
    let observed_estimates = cached_observed_return_shape_statistic_estimates(
        horizon,
        temporal_block_count,
        observed_values,
        context,
    );

    let simulated_estimates_by_seed = simulated_values_by_seed
        .iter()
        .map(|values| return_shape_statistic_estimates(values, temporal_block_count))
        .collect::<Vec<_>>();

    let mut blocks = Vec::new();
    for group in AGGREGATED_RETURN_SHAPE_MECHANISM_GROUPS {
        let metric_names = group
            .metric_names
            .iter()
            .copied()
            .filter(|metric_name| {
                statistic_estimate_by_name(observed_estimates.as_ref(), metric_name).is_some()
                    && simulated_estimates_by_seed.iter().all(|estimates| {
                        statistic_estimate_by_name(estimates.as_slice(), metric_name).is_some()
                    })
            })
            .collect::<Vec<_>>();
        if metric_names.is_empty() {
            continue;
        }

        let Some(observed_vector) =
            estimate_vector_by_metric_names(observed_estimates.as_ref(), metric_names.as_slice())
        else {
            continue;
        };

        let Some(observed_covariance_rows) = cached_observed_return_shape_covariance(
            horizon,
            temporal_block_count,
            observed_values,
            metric_names.as_slice(),
            observed_vector.as_slice(),
            context,
        )
        .map(|covariance_rows| {
            // Objective-family blocks own covariance rows so cache entries can stay shared.
            covariance_rows.as_ref().clone()
        })
        .or_else(|| {
            let standard_errors = metric_names
                .iter()
                .map(|metric_name| {
                    statistic_estimate_by_name(observed_estimates.as_ref(), metric_name)
                        .map(|estimate| estimate.standard_error)
                })
                .collect::<Option<Vec<_>>>()?;

            diagonal_covariance_from_standard_errors(standard_errors.as_slice())
        }) else {
            continue;
        };

        let Some(model_seed_vectors) = simulated_estimates_by_seed
            .iter()
            .map(|estimates| estimate_vector_by_metric_names(estimates, metric_names.as_slice()))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };

        blocks.push(ReturnShapeObjectiveFamilyBlock {
            family_id: group.family_id,
            horizon_label: return_shape_horizon_label(horizon),
            metric_names,
            observed_vector,
            observed_covariance_rows,
            model_seed_vectors,
        });
    }

    blocks
}

fn return_shape_objective_family_accumulators() -> Vec<ReturnShapeObjectiveFamilyAccumulator> {
    RETURN_SHAPE_MECHANISM_FAMILY_IDS
        .iter()
        .map(|family_id| ReturnShapeObjectiveFamilyAccumulator {
            family_id,
            coordinate_ids: Vec::new(),
            observed_vector: Vec::new(),
            observed_covariance_blocks: Vec::new(),
            model_seed_vectors: None,
        })
        .collect()
}

impl ReturnShapeObjectiveFamilyAccumulator {
    fn append(&mut self, block: ReturnShapeObjectiveFamilyBlock) {
        if block.family_id != self.family_id
            || block.metric_names.is_empty()
            || block.metric_names.len() != block.observed_vector.len()
            || block.observed_covariance_rows.len() != block.metric_names.len()
            || block
                .observed_covariance_rows
                .iter()
                .any(|row| row.len() != block.metric_names.len())
            || block.model_seed_vectors.is_empty()
            || block
                .model_seed_vectors
                .iter()
                .any(|vector| vector.len() != block.metric_names.len())
        {
            return;
        }

        if let Some(model_seed_vectors) = self.model_seed_vectors.as_mut() {
            if model_seed_vectors.len() != block.model_seed_vectors.len() {
                return;
            }

            for (target_vector, source_vector) in model_seed_vectors
                .iter_mut()
                .zip(block.model_seed_vectors.iter())
            {
                target_vector.extend_from_slice(source_vector.as_slice());
            }
        } else {
            self.model_seed_vectors = Some(block.model_seed_vectors);
        }

        self.coordinate_ids
            .extend(block.metric_names.iter().map(|metric_name| {
                format!("{}.{}.{}", self.family_id, block.horizon_label, metric_name)
            }));
        self.observed_vector
            .extend_from_slice(block.observed_vector.as_slice());
        self.observed_covariance_blocks
            .push(block.observed_covariance_rows);
    }

    fn add_to_builder(self, builder: &mut CalibrationLossBuilder) {
        let Some(model_seed_vectors) = self.model_seed_vectors else {
            return;
        };

        let Some(observed_covariance_rows) =
            block_diagonal_covariance(self.observed_covariance_blocks.as_slice())
        else {
            return;
        };

        add_objective_family_from_seed_vectors(
            builder,
            self.family_id.to_string(),
            self.coordinate_ids,
            self.observed_vector,
            observed_covariance_rows,
            model_seed_vectors,
        );
    }
}

fn tick_shape_probability_specifications(
    observed_values: &[f64],
) -> Option<Vec<TickShapeProbabilitySpecification>> {
    let sorted_observed_values = sorted_finite_values(observed_values);
    if sorted_observed_values.is_empty() {
        return None;
    }

    let sorted_absolute_observed_values =
        sorted_absolute_values(sorted_observed_values.as_slice())?;

    let mut specifications =
        Vec::with_capacity(TICK_SIGNED_CDF_CUTPOINTS.len() + TICK_ABSOLUTE_CDF_CUTPOINTS.len());

    specifications.extend(TICK_SIGNED_CDF_CUTPOINTS.iter().map(|cutpoint| {
        TickShapeProbabilitySpecification {
            metric_name: cutpoint.metric_name,
            threshold: compute_sorted_quantile_or_zero(
                sorted_observed_values.as_slice(),
                cutpoint.quantile_level,
            ),
            kind: cutpoint.kind,
        }
    }));

    specifications.extend(TICK_ABSOLUTE_CDF_CUTPOINTS.iter().map(|cutpoint| {
        TickShapeProbabilitySpecification {
            metric_name: cutpoint.metric_name,
            threshold: compute_sorted_quantile_or_zero(
                sorted_absolute_observed_values.as_slice(),
                cutpoint.quantile_level,
            ),
            kind: cutpoint.kind,
        }
    }));

    Some(specifications)
}

fn tick_shape_probability_vector(
    values: &[f64],
    specifications: &[TickShapeProbabilitySpecification],
) -> Option<Vec<f64>> {
    let sorted_values = sorted_finite_values(values);
    if sorted_values.is_empty() || specifications.is_empty() {
        return None;
    }

    let sorted_absolute_values = sorted_absolute_values(sorted_values.as_slice())?;

    specifications
        .iter()
        .map(|specification| {
            let distribution_values = match specification.kind {
                TickShapeProbabilityKind::Signed => sorted_values.as_slice(),
                TickShapeProbabilityKind::Absolute => sorted_absolute_values.as_slice(),
            };

            empirical_cdf_at_threshold(distribution_values, specification.threshold)
        })
        .collect()
}

fn cached_observed_tick_shape_probability_covariance(
    temporal_block_count: usize,
    observed_values: &[f64],
    specifications: &[TickShapeProbabilitySpecification],
    observed_vector: &[f64],
    context: &JointRefinementContext,
) -> Option<Arc<Vec<Vec<f64>>>> {
    let cache_key = JointRefinementObservedReturnShapeStatisticCacheKey {
        horizon: JointRefinementReturnShapeHorizon::Tick,
        temporal_block_count,
        metric_names: specifications
            .iter()
            .map(|specification| specification.metric_name)
            .collect(),
    };

    if let Ok(cache_guard) = context.observed_return_shape_covariance_cache.lock()
        && let Some(cached_covariance) = cache_guard.get(&cache_key)
    {
        return Some(Arc::clone(cached_covariance));
    }

    let covariance = Arc::new(tick_shape_probability_delete_block_covariance(
        observed_values,
        temporal_block_count,
        specifications,
        observed_vector,
    )?);

    if let Ok(mut cache_guard) = context.observed_return_shape_covariance_cache.lock() {
        cache_guard.insert(cache_key, Arc::clone(&covariance));
    }

    Some(covariance)
}

fn tick_shape_probability_delete_block_covariance(
    values: &[f64],
    temporal_block_count: usize,
    specifications: &[TickShapeProbabilitySpecification],
    full_sample_vector: &[f64],
) -> Option<Vec<Vec<f64>>> {
    if specifications.is_empty() {
        return None;
    }

    let delete_block_vectors =
        return_shape_delete_block_vectors(values, temporal_block_count, |retained_values, _| {
            tick_shape_probability_vector(retained_values, specifications)
        })?;

    covariance_from_delete_block_vectors(full_sample_vector, delete_block_vectors.as_slice())
}

fn tick_shape_probability_diagonal_covariance(
    observed_vector: &[f64],
    observed_value_count: usize,
    temporal_block_count: usize,
) -> Option<Vec<Vec<f64>>> {
    let sample_count =
        return_shape_effective_sample_count(observed_value_count, temporal_block_count);

    let variances = observed_vector
        .iter()
        .map(|share| {
            let standard_error = binomial_share_standard_error(*share, sample_count);
            standard_error
                .is_finite()
                .then_some(standard_error * standard_error)
        })
        .collect::<Option<Vec<_>>>()?;

    diagonal_covariance(variances.as_slice())
}

fn sorted_absolute_values(sorted_values: &[f64]) -> Option<Vec<f64>> {
    let absolute_values = sorted_values
        .iter()
        .copied()
        .map(f64::abs)
        .collect::<Vec<_>>();

    let sorted_absolute_values = sorted_finite_values(absolute_values.as_slice());

    (!sorted_absolute_values.is_empty()).then_some(sorted_absolute_values)
}

fn empirical_cdf_at_threshold(sorted_values: &[f64], threshold: f64) -> Option<f64> {
    if sorted_values.is_empty() || !threshold.is_finite() {
        return None;
    }

    let count_at_or_below_threshold = sorted_values.partition_point(|value| *value <= threshold);

    Some(count_at_or_below_threshold as f64 / sorted_values.len() as f64)
}

#[cfg(test)]
mod tests {
    use super::{
        RETURN_SHAPE_TAIL_MASS_FAMILY_ID, ReturnShapeObjectiveFamilyAccumulator,
        ReturnShapeObjectiveFamilyBlock, tick_shape_probability_delete_block_covariance,
        tick_shape_probability_specifications, tick_shape_probability_vector,
    };
    use crate::objective::CalibrationLossBuilder;

    #[test]
    fn tick_shape_probability_vector_scores_fixed_observed_cutpoints() {
        let observed_values = [-2.0, -1.0, 0.0, 1.0, 2.0];

        let specifications = tick_shape_probability_specifications(observed_values.as_slice())
            .expect("observed tick sample should define probability cutpoints");

        let candidate_values = [-3.0, -1.0, 0.5, 3.0];

        let probabilities =
            tick_shape_probability_vector(candidate_values.as_slice(), specifications.as_slice())
                .expect("candidate tick sample should evaluate against observed cutpoints");

        assert_eq!(specifications[0].metric_name, "cdf_at_observed_q01");
        assert_eq!(specifications[6].metric_name, "cdf_at_observed_q99");
        assert_eq!(
            specifications[7].metric_name,
            "absolute_cdf_at_observed_q50"
        );
        assert_eq!(probabilities[3], 0.5);
        assert_eq!(probabilities[7], 0.5);
    }

    #[test]
    fn tick_shape_probability_delete_block_covariance_tracks_share_variation() {
        let observed_values = [-2.0, -1.0, -0.5, 0.5, 1.0, 2.0];

        let specifications = tick_shape_probability_specifications(observed_values.as_slice())
            .expect("observed tick sample should define probability cutpoints");

        let full_sample_vector =
            tick_shape_probability_vector(observed_values.as_slice(), specifications.as_slice())
                .expect("observed tick sample should evaluate against observed cutpoints");

        let covariance = tick_shape_probability_delete_block_covariance(
            observed_values.as_slice(),
            3,
            specifications.as_slice(),
            full_sample_vector.as_slice(),
        )
        .expect("delete-block covariance should identify share movement");

        assert_eq!(covariance.len(), specifications.len());
        assert!(
            covariance
                .iter()
                .any(|row| row.iter().any(|value| *value > 0.0))
        );
    }

    #[test]
    fn return_shape_accumulator_combines_horizon_blocks_into_one_family() {
        let mut accumulator = ReturnShapeObjectiveFamilyAccumulator {
            family_id: RETURN_SHAPE_TAIL_MASS_FAMILY_ID,
            coordinate_ids: Vec::new(),
            observed_vector: Vec::new(),
            observed_covariance_blocks: Vec::new(),
            model_seed_vectors: None,
        };

        accumulator.append(ReturnShapeObjectiveFamilyBlock {
            family_id: RETURN_SHAPE_TAIL_MASS_FAMILY_ID,
            horizon_label: "5m",
            metric_names: vec!["q05", "q95"],
            observed_vector: vec![1.0, 2.0],
            observed_covariance_rows: vec![vec![1.0, 0.2], vec![0.2, 2.0]],
            model_seed_vectors: vec![vec![1.5, 2.5], vec![2.0, 3.0]],
        });
        accumulator.append(ReturnShapeObjectiveFamilyBlock {
            family_id: RETURN_SHAPE_TAIL_MASS_FAMILY_ID,
            horizon_label: "1h",
            metric_names: vec!["q05"],
            observed_vector: vec![3.0],
            observed_covariance_rows: vec![vec![4.0]],
            model_seed_vectors: vec![vec![3.5], vec![4.0]],
        });

        let mut builder = CalibrationLossBuilder::new();
        accumulator.add_to_builder(&mut builder);
        let loss = builder.build();

        assert_eq!(
            loss.family_losses[0].family_id,
            RETURN_SHAPE_TAIL_MASS_FAMILY_ID
        );
        assert_eq!(loss.family_losses[0].coordinate_count, 3);
        assert_eq!(
            loss.term_diagnostics
                .iter()
                .map(|term| term.term_id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "return_shape.tail_mass.5m.q05",
                "return_shape.tail_mass.5m.q95",
                "return_shape.tail_mass.1h.q05",
            ]
        );
    }
}
