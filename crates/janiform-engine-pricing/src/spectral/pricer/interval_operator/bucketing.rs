use std::collections::{BTreeMap, BTreeSet};

use janiform_foundations::numeric::ceil_integer_square_root;

use crate::grids::{
    SpectralThetaWarmStartKey, VanillaOptionGridBatchPricingOperatorApproximationDiagnostics,
};

use super::super::row_frequency::{
    build_row_frequency_batch_from_representative_kernel, theta_warm_start_driver_configuration,
};
use super::super::{
    PricingProgressEvent, SpectralThetaWarmStartState, VanillaOptionPricingRunError,
    emit_pricing_progress,
};
use super::super::{RiskNeutralAnchorMaterialization, VanillaOptionPricingIntervalFactors};
use super::core::{
    CachedIntervalOperator, CachedIntervalOperatorBuildOutput, CachedIntervalOperatorJob,
    CachedIntervalOperatorSolveContext, CachedIntervalOperatorSolveOutput, build_interval_summary,
    interval_transition_context, solve_cached_interval_operators_in_parallel,
    solve_cached_interval_operators_serially,
};

pub(super) struct BucketedCachedIntervalOperatorBuildInput<'a, 'observer, 'callback> {
    pub(super) theta_warm_start_key: SpectralThetaWarmStartKey,
    pub(super) jobs: &'a [CachedIntervalOperatorJob],
    pub(super) solve_context: CachedIntervalOperatorSolveContext<'a>,
    pub(super) maturity_counts: &'a [usize],
    pub(super) interval_factors: &'a [VanillaOptionPricingIntervalFactors],
    pub(super) materialization: &'a RiskNeutralAnchorMaterialization,
    pub(super) driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
    pub(super) observer: &'observer mut Option<&'callback mut dyn FnMut(PricingProgressEvent)>,
    pub(super) theta_warm_start_state: Option<&'a mut SpectralThetaWarmStartState>,
}

pub(super) fn build_bucketed_cached_interval_operators(
    input: BucketedCachedIntervalOperatorBuildInput<'_, '_, '_>,
) -> Result<CachedIntervalOperatorBuildOutput, VanillaOptionPricingRunError> {
    let BucketedCachedIntervalOperatorBuildInput {
        theta_warm_start_key,
        jobs,
        solve_context,
        maturity_counts,
        interval_factors,
        materialization,
        driver_parameters,
        observer,
        mut theta_warm_start_state,
    } = input;

    if jobs.is_empty() {
        return Ok(CachedIntervalOperatorBuildOutput {
            operators: Vec::new(),
            approximation_diagnostics: Some(
                VanillaOptionGridBatchPricingOperatorApproximationDiagnostics {
                    exact_interval_count: 0,
                    representative_kernel_count: 0,
                    reused_interval_count: 0,
                    compression_ratio: 1.0,
                    exact_pricing_forced_by_sensitivities: false,
                },
            ),
        });
    }

    let features = interval_bucket_features(
        maturity_counts,
        interval_factors,
        materialization,
        jobs.len(),
    );

    let representative_offsets = bucket_representative_interval_offsets(features.as_slice());

    let representative_offset_set = representative_offsets
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();

    emit_pricing_progress(
        observer,
        PricingProgressEvent::interval_progress(
            0,
            representative_offsets.len(),
            Some(0),
            representative_offsets.len(),
        ),
    );

    let representative_outputs = solve_bucket_representative_interval_operators(
        jobs,
        representative_offsets.as_slice(),
        solve_context,
        observer,
    )?;

    let current_driver_configuration = theta_warm_start_driver_configuration(driver_parameters);

    let mut cached_interval_operators = Vec::with_capacity(jobs.len());

    for job in jobs {
        let output = if representative_offset_set.contains(&job.interval_offset) {
            representative_outputs
                .get(&job.interval_offset)
                .cloned()
                .ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
                    message: "missing bucket representative interval operator".to_string(),
                })?
        } else {
            let representative_offset =
                nearest_representative_interval_offset(job.interval_offset, features.as_slice());

            let representative_output = representative_outputs
                .get(&representative_offset)
                .ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
                    message: "missing nearest bucket representative interval operator".to_string(),
                })?;

            build_bucketed_interval_operator(job, solve_context, representative_output)?
        };

        if let Some(state) = theta_warm_start_state.as_deref_mut() {
            state.record_interval_theta_states(
                theta_warm_start_key,
                output.interval_offset,
                current_driver_configuration,
                output.row_theta_states.clone(),
            );
        }

        cached_interval_operators.push(output.operator);
    }

    emit_pricing_progress(
        observer,
        PricingProgressEvent::interval_progress(
            representative_offsets.len(),
            representative_offsets.len(),
            None,
            representative_offsets.len(),
        ),
    );

    let representative_kernel_count = representative_outputs.len();
    let exact_interval_count = jobs.len();

    Ok(CachedIntervalOperatorBuildOutput {
        operators: cached_interval_operators,
        approximation_diagnostics: Some(
            VanillaOptionGridBatchPricingOperatorApproximationDiagnostics {
                exact_interval_count,
                representative_kernel_count,
                reused_interval_count: exact_interval_count
                    .saturating_sub(representative_kernel_count),
                compression_ratio: exact_interval_count as f64 / representative_kernel_count as f64,
                exact_pricing_forced_by_sensitivities: false,
            },
        ),
    })
}

fn solve_bucket_representative_interval_operators(
    jobs: &[CachedIntervalOperatorJob],
    representative_offsets: &[usize],
    solve_context: CachedIntervalOperatorSolveContext<'_>,
    observer: &mut Option<&mut dyn FnMut(PricingProgressEvent)>,
) -> Result<BTreeMap<usize, CachedIntervalOperatorSolveOutput>, VanillaOptionPricingRunError> {
    let representative_jobs = representative_offsets
        .iter()
        .map(|representative_offset| {
            jobs.get(*representative_offset).cloned().ok_or_else(|| {
                VanillaOptionPricingRunError::InternalNumericsFailure {
                    message: "bucket representative interval offset is out of range".to_string(),
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let representative_solve_context = CachedIntervalOperatorSolveContext {
        maximum_maturity_count: representative_jobs.len(),
        ..solve_context
    };

    let outputs = if super::super::resolved_pricing_cpu_count(solve_context.cpu_count) > 1
        && representative_jobs.len() > 1
    {
        solve_cached_interval_operators_in_parallel(
            representative_jobs.as_slice(),
            representative_solve_context,
            observer,
        )?
    } else {
        solve_cached_interval_operators_serially(
            representative_jobs.as_slice(),
            representative_solve_context,
            observer,
        )?
    };

    Ok(outputs
        .into_iter()
        .map(|output| (output.interval_offset, output))
        .collect())
}

fn build_bucketed_interval_operator(
    job: &CachedIntervalOperatorJob,
    solve_context: CachedIntervalOperatorSolveContext<'_>,
    representative_output: &CachedIntervalOperatorSolveOutput,
) -> Result<CachedIntervalOperatorSolveOutput, VanillaOptionPricingRunError> {
    let transition_context = interval_transition_context(job, solve_context)?;

    let collocation_nodes = solve_context.spectral_domain.collocation_nodes();

    let row_states = collocation_nodes
        .iter()
        .copied()
        .map(|deviation| transition_context.base_context.build_row_state(deviation))
        .collect::<Result<Vec<_>, _>>()?;

    if row_states.len() != representative_output.operator.row_operators.len() {
        return Err(VanillaOptionPricingRunError::InternalNumericsFailure {
            message: "bucket representative row count does not match target interval".to_string(),
        });
    }

    let row_operators = row_states
        .iter()
        .zip(representative_output.operator.row_operators.iter())
        .map(|(row_state, representative_operator)| {
            build_row_frequency_batch_from_representative_kernel(
                &transition_context.base_context,
                row_state,
                transition_context.next_anchor_value,
                solve_context.spectral_domain,
                representative_operator,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(CachedIntervalOperatorSolveOutput {
        interval_offset: job.interval_offset,
        operator: CachedIntervalOperator {
            interval_summary: build_interval_summary(
                job.interval_offset,
                solve_context,
                &transition_context,
                row_operators.as_slice(),
            ),
            row_operators,
            row_operator_sensitivities: None,
        },
        row_theta_states: representative_output.row_theta_states.clone(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct IntervalBucketPartitionKey {
    maturity_cell_index: usize,
    interval_delta_bits: u64,
    has_open_exposure: bool,
    has_known_news: bool,
    has_surprise_news: bool,
}

#[derive(Debug, Clone)]
pub(super) struct IntervalBucketFeature {
    interval_offset: usize,
    partition_key: IntervalBucketPartitionKey,
    values: [f64; 10],
}

pub(super) fn interval_bucket_features(
    maturity_counts: &[usize],
    interval_factors: &[VanillaOptionPricingIntervalFactors],
    materialization: &RiskNeutralAnchorMaterialization,
    interval_count: usize,
) -> Vec<IntervalBucketFeature> {
    let runtime_anchors = &materialization.runtime_interval_anchors;

    let denominator = interval_count.saturating_sub(1).max(1) as f64;

    (0..interval_count)
        .map(|interval_offset| {
            let interval_factor = interval_factors[interval_offset];

            let current_anchor_value = if interval_offset == 0 {
                runtime_anchors.start_anchor_value
            } else {
                runtime_anchors.end_anchor_values[interval_offset - 1]
            };

            let current_pressure_state = if interval_offset == 0 {
                runtime_anchors.start_pressure_state
            } else {
                runtime_anchors.end_pressure_states[interval_offset - 1]
            };

            let next_anchor_value = runtime_anchors.end_anchor_values[interval_offset];

            let expected_hawkes_state_before =
                materialization.expected_hawkes_state_path[interval_offset];
            let expected_hawkes_state_after =
                materialization.expected_hawkes_state_path[interval_offset + 1];

            let interval_delta = runtime_anchors.interval_deltas[interval_offset];

            IntervalBucketFeature {
                interval_offset,
                partition_key: IntervalBucketPartitionKey {
                    maturity_cell_index: maturity_cell_index(interval_offset, maturity_counts),
                    interval_delta_bits: interval_delta.to_bits(),
                    has_open_exposure: interval_factor.s_h > 0.0,
                    has_known_news: interval_factor.nu_h_known != 0.0,
                    has_surprise_news: interval_factor.nu_h_surprise != 0.0,
                },
                values: [
                    interval_offset as f64 / denominator,
                    interval_delta,
                    interval_factor.s_h,
                    interval_factor.nu_h_known,
                    interval_factor.nu_h_surprise,
                    interval_factor.return_mark_scale,
                    hawkes_state_total(expected_hawkes_state_before),
                    hawkes_state_total(expected_hawkes_state_after),
                    current_pressure_state,
                    next_anchor_value - current_anchor_value,
                ],
            }
        })
        .collect()
}

fn maturity_cell_index(interval_offset: usize, maturity_counts: &[usize]) -> usize {
    maturity_counts
        .iter()
        .position(|maturity_count| interval_offset < *maturity_count)
        .unwrap_or_else(|| maturity_counts.len().saturating_sub(1))
}

fn hawkes_state_total(state: janiform_model_kernels::hawkes::HawkesExcitationState) -> f64 {
    state.y_plus_plus + state.y_plus_minus + state.y_minus_plus + state.y_minus_minus
}

pub(super) fn bucket_representative_interval_offsets(
    features: &[IntervalBucketFeature],
) -> Vec<usize> {
    let mut partitioned_features =
        BTreeMap::<IntervalBucketPartitionKey, Vec<&IntervalBucketFeature>>::new();

    for feature in features {
        partitioned_features
            .entry(feature.partition_key)
            .or_default()
            .push(feature);
    }

    let mut representatives = partitioned_features
        .values()
        .flat_map(|partition_features| {
            let target_count = ceil_integer_square_root(partition_features.len());
            farthest_point_interval_representatives(partition_features.as_slice(), target_count)
        })
        .collect::<Vec<_>>();

    representatives.sort_unstable();
    representatives.dedup();

    representatives
}

fn farthest_point_interval_representatives(
    features: &[&IntervalBucketFeature],
    target_count: usize,
) -> Vec<usize> {
    if features.is_empty() {
        return Vec::new();
    }

    let target_count = target_count.min(features.len()).max(1);

    let feature_ranges = feature_ranges(features);

    let mut selected = vec![features[0].interval_offset];

    while selected.len() < target_count {
        let next_feature = features
            .iter()
            .filter(|feature| !selected.contains(&feature.interval_offset))
            .max_by(|left, right| {
                nearest_selected_distance(left, selected.as_slice(), features, &feature_ranges)
                    .total_cmp(&nearest_selected_distance(
                        right,
                        selected.as_slice(),
                        features,
                        &feature_ranges,
                    ))
                    .then_with(|| right.interval_offset.cmp(&left.interval_offset))
            });

        let Some(next_feature) = next_feature else {
            break;
        };

        selected.push(next_feature.interval_offset);
    }

    selected.sort_unstable();

    selected
}

fn feature_ranges(features: &[&IntervalBucketFeature]) -> [(f64, f64); 10] {
    let mut ranges = [(f64::INFINITY, f64::NEG_INFINITY); 10];

    for feature in features {
        for (feature_index, value) in feature.values.iter().copied().enumerate() {
            ranges[feature_index].0 = ranges[feature_index].0.min(value);
            ranges[feature_index].1 = ranges[feature_index].1.max(value);
        }
    }

    ranges
}

fn nearest_selected_distance(
    feature: &IntervalBucketFeature,
    selected_offsets: &[usize],
    features: &[&IntervalBucketFeature],
    feature_ranges: &[(f64, f64); 10],
) -> f64 {
    selected_offsets
        .iter()
        .filter_map(|selected_offset| {
            features
                .iter()
                .find(|candidate| candidate.interval_offset == *selected_offset)
        })
        .map(|selected_feature| {
            normalized_feature_distance(feature, selected_feature, feature_ranges)
        })
        .fold(f64::INFINITY, f64::min)
}

fn normalized_feature_distance(
    left: &IntervalBucketFeature,
    right: &IntervalBucketFeature,
    feature_ranges: &[(f64, f64); 10],
) -> f64 {
    left.values
        .iter()
        .zip(right.values.iter())
        .zip(feature_ranges.iter())
        .map(
            |((left_value, right_value), (minimum_value, maximum_value))| {
                let range = maximum_value - minimum_value;
                if range == 0.0 {
                    0.0
                } else {
                    let normalized_delta = (left_value - right_value) / range;
                    normalized_delta * normalized_delta
                }
            },
        )
        .sum::<f64>()
}

pub(super) fn nearest_representative_interval_offset(
    interval_offset: usize,
    features: &[IntervalBucketFeature],
) -> usize {
    let Some(feature) = features
        .iter()
        .find(|candidate| candidate.interval_offset == interval_offset)
    else {
        return interval_offset;
    };

    let partition_features = features
        .iter()
        .filter(|candidate| candidate.partition_key == feature.partition_key)
        .collect::<Vec<_>>();

    let representative_offsets = farthest_point_interval_representatives(
        partition_features.as_slice(),
        ceil_integer_square_root(partition_features.len()),
    );

    let feature_ranges = feature_ranges(partition_features.as_slice());

    let representative_features = representative_offsets
        .iter()
        .filter_map(|representative_offset| {
            features
                .iter()
                .find(|candidate| candidate.interval_offset == *representative_offset)
        })
        .collect::<Vec<_>>();

    representative_features
        .iter()
        .min_by(|left_feature, right_feature| {
            normalized_feature_distance(feature, left_feature, &feature_ranges)
                .total_cmp(&normalized_feature_distance(
                    feature,
                    right_feature,
                    &feature_ranges,
                ))
                .then_with(|| {
                    left_feature
                        .interval_offset
                        .cmp(&right_feature.interval_offset)
                })
        })
        .map(|representative_feature| representative_feature.interval_offset)
        .unwrap_or(interval_offset)
}

#[cfg(test)]
mod tests {
    use janiform_foundations::numeric::ceil_integer_square_root;

    use super::{
        IntervalBucketFeature, IntervalBucketPartitionKey, bucket_representative_interval_offsets,
        nearest_representative_interval_offset,
    };

    fn test_partition_key(maturity_cell_index: usize) -> IntervalBucketPartitionKey {
        IntervalBucketPartitionKey {
            maturity_cell_index,
            interval_delta_bits: 1.0f64.to_bits(),
            has_open_exposure: false,
            has_known_news: false,
            has_surprise_news: false,
        }
    }

    fn test_feature(
        interval_offset: usize,
        partition_key: IntervalBucketPartitionKey,
        feature_position: f64,
    ) -> IntervalBucketFeature {
        let mut values = [0.0; 10];
        values[0] = feature_position;
        values[1] = feature_position * feature_position;

        IntervalBucketFeature {
            interval_offset,
            partition_key,
            values,
        }
    }

    #[test]
    fn bucket_representative_count_is_square_root_of_partition_size() {
        let partition_key = test_partition_key(0);
        let features = (0..10)
            .map(|interval_offset| {
                test_feature(interval_offset, partition_key, interval_offset as f64)
            })
            .collect::<Vec<_>>();

        let representatives = bucket_representative_interval_offsets(features.as_slice());

        assert_eq!(
            representatives.len(),
            ceil_integer_square_root(features.len())
        );
    }

    #[test]
    fn bucket_representatives_are_stable_for_identical_inputs() {
        let partition_key = test_partition_key(0);
        let features = (0..9)
            .map(|interval_offset| {
                test_feature(interval_offset, partition_key, (interval_offset % 3) as f64)
            })
            .collect::<Vec<_>>();

        assert_eq!(
            bucket_representative_interval_offsets(features.as_slice()),
            bucket_representative_interval_offsets(features.as_slice())
        );
    }

    #[test]
    fn bucket_assignment_maps_every_interval_to_a_representative() {
        let first_partition_key = test_partition_key(0);
        let second_partition_key = test_partition_key(1);
        let features = (0..6)
            .map(|interval_offset| {
                test_feature(interval_offset, first_partition_key, interval_offset as f64)
            })
            .chain((6..12).map(|interval_offset| {
                test_feature(
                    interval_offset,
                    second_partition_key,
                    (interval_offset - 6) as f64,
                )
            }))
            .collect::<Vec<_>>();

        let representatives = bucket_representative_interval_offsets(features.as_slice());

        for feature in &features {
            let representative_offset = nearest_representative_interval_offset(
                feature.interval_offset,
                features.as_slice(),
            );

            assert!(representatives.contains(&representative_offset));
        }
    }

    #[test]
    fn single_interval_partitions_keep_their_interval_as_representative() {
        let features = (0..4)
            .map(|interval_offset| {
                test_feature(
                    interval_offset,
                    test_partition_key(interval_offset),
                    interval_offset as f64,
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            bucket_representative_interval_offsets(features.as_slice()),
            vec![0, 1, 2, 3]
        );
    }
}
