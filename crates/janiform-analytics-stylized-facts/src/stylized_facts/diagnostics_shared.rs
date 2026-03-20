//! Shared helpers for stylized-fact diagnostics modules.

use janiform_foundations::numeric::compute_sorted_quantile;
use janiform_foundations::randomness::{
    SimulationRandomnessStream, SplitMix64SimulationRandomnessStream,
};
use janiform_foundations::splitmix64_output_mixer;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BlockBootstrapConfiguration {
    pub confidence_level: f64,
    pub replication_count: usize,
    pub block_length_interval_count: usize,
}

pub(crate) fn validate_block_bootstrap_configuration(
    bootstrap_configuration: BlockBootstrapConfiguration,
) -> Result<(), &'static str> {
    if !bootstrap_configuration.confidence_level.is_finite()
        || bootstrap_configuration.confidence_level <= 0.0
        || bootstrap_configuration.confidence_level >= 1.0
    {
        return Err("confidence_level");
    }

    if bootstrap_configuration.replication_count == 0 {
        return Err("replication_count");
    }

    if bootstrap_configuration.block_length_interval_count == 0 {
        return Err("block_length_interval_count");
    }

    Ok(())
}

pub(crate) fn compute_block_bootstrap_confidence_interval<T, F>(
    source_series: &[T],
    point_estimate: f64,
    bootstrap_configuration: BlockBootstrapConfiguration,
    bootstrap_seed: u64,
    metric_evaluator: F,
) -> (f64, f64)
where
    T: Copy,
    F: Fn(&[T]) -> Option<f64>,
{
    if source_series.is_empty() {
        return (point_estimate, point_estimate);
    }

    let mut bootstrap_stream = SplitMix64SimulationRandomnessStream::from_seed(bootstrap_seed);
    let mut bootstrap_replication_values: Vec<f64> =
        Vec::with_capacity(bootstrap_configuration.replication_count);

    for _ in 0..bootstrap_configuration.replication_count {
        let resampled_series = generate_block_bootstrap_resample(
            source_series,
            bootstrap_configuration.block_length_interval_count,
            &mut bootstrap_stream,
        );

        let Some(bootstrap_estimate) = metric_evaluator(&resampled_series) else {
            continue;
        };
        if bootstrap_estimate.is_finite() {
            bootstrap_replication_values.push(bootstrap_estimate);
        }
    }

    if bootstrap_replication_values.is_empty() {
        return (point_estimate, point_estimate);
    }

    bootstrap_replication_values.sort_by(f64::total_cmp);

    let lower_tail_probability = (1.0 - bootstrap_configuration.confidence_level) * 0.5;
    let upper_tail_probability = 1.0 - lower_tail_probability;

    let lower_bound =
        compute_sorted_quantile(&bootstrap_replication_values, lower_tail_probability);
    let upper_bound =
        compute_sorted_quantile(&bootstrap_replication_values, upper_tail_probability);

    (lower_bound, upper_bound)
}

pub(crate) fn coverage_ratio(covered_interval_count: usize, horizon_interval_count: usize) -> f64 {
    if horizon_interval_count == 0 {
        return 0.0;
    }

    covered_interval_count as f64 / horizon_interval_count as f64
}

pub(crate) fn derive_metric_bootstrap_seed(
    root_seed: u64,
    horizon_position: usize,
    dataset_role: u64,
    metric_role: u64,
) -> u64 {
    let seed_mixed_by_horizon =
        root_seed ^ (horizon_position as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let seed_mixed_by_dataset =
        seed_mixed_by_horizon ^ dataset_role.wrapping_mul(0xBF58_476D_1CE4_E5B9);

    splitmix64_output_mixer(seed_mixed_by_dataset ^ metric_role.wrapping_mul(0x94D0_49BB_1331_11EB))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DeltaUncertaintyComponents {
    pub empirical_interval_half_width: Option<f64>,
    pub simulated_interval_half_width: Option<f64>,
    pub pooled_interval_half_width: Option<f64>,
    pub standardized_delta: Option<f64>,
}

pub(crate) fn derive_delta_uncertainty_components(
    empirical_interval_bounds: Option<(f64, f64)>,
    simulated_interval_bounds: Option<(f64, f64)>,
    absolute_delta: Option<f64>,
) -> DeltaUncertaintyComponents {
    let empirical_interval_half_width =
        empirical_interval_bounds.and_then(interval_bounds_half_width);
    let simulated_interval_half_width =
        simulated_interval_bounds.and_then(interval_bounds_half_width);

    let pooled_interval_half_width =
        match (empirical_interval_half_width, simulated_interval_half_width) {
            (Some(empirical_half_width), Some(simulated_half_width)) => {
                let pooled = (empirical_half_width * empirical_half_width
                    + simulated_half_width * simulated_half_width)
                    .sqrt();
                pooled.is_finite().then_some(pooled)
            }
            _ => None,
        };

    let standardized_delta = match (absolute_delta, pooled_interval_half_width) {
        (Some(delta), Some(pooled_half_width)) if pooled_half_width > 0.0 => {
            let standardized_value = delta / pooled_half_width;
            standardized_value.is_finite().then_some(standardized_value)
        }
        _ => None,
    };

    DeltaUncertaintyComponents {
        empirical_interval_half_width,
        simulated_interval_half_width,
        pooled_interval_half_width,
        standardized_delta,
    }
}

pub(crate) fn build_delta_uncertainty_context<T, F>(
    empirical_interval_bounds: Option<(f64, f64)>,
    simulated_interval_bounds: Option<(f64, f64)>,
    absolute_delta: Option<f64>,
    context_builder: F,
) -> T
where
    F: FnOnce(DeltaUncertaintyComponents) -> T,
{
    let uncertainty_components = derive_delta_uncertainty_components(
        empirical_interval_bounds,
        simulated_interval_bounds,
        absolute_delta,
    );

    context_builder(uncertainty_components)
}

fn interval_bounds_half_width(interval_bounds: (f64, f64)) -> Option<f64> {
    let (lower_bound, upper_bound) = interval_bounds;
    let half_width = (upper_bound - lower_bound) * 0.5;
    if half_width.is_finite() && half_width >= 0.0 {
        Some(half_width)
    } else {
        None
    }
}

fn generate_block_bootstrap_resample<T>(
    source_series: &[T],
    requested_block_length: usize,
    bootstrap_stream: &mut SplitMix64SimulationRandomnessStream,
) -> Vec<T>
where
    T: Copy,
{
    let sample_count = source_series.len();
    if sample_count == 0 {
        return Vec::new();
    }

    let block_length = requested_block_length.max(1).min(sample_count);

    let mut resampled_series: Vec<T> = Vec::with_capacity(sample_count);
    while resampled_series.len() < sample_count {
        let block_start_position = sample_uniform_position(sample_count, bootstrap_stream);
        for block_offset in 0..block_length {
            if resampled_series.len() >= sample_count {
                break;
            }
            let source_position = (block_start_position + block_offset) % sample_count;
            resampled_series.push(source_series[source_position]);
        }
    }

    resampled_series
}

fn sample_uniform_position(
    upper_bound: usize,
    bootstrap_stream: &mut SplitMix64SimulationRandomnessStream,
) -> usize {
    if upper_bound <= 1 {
        return 0;
    }

    let sampled_position =
        (bootstrap_stream.next_unit_interval_f64() * upper_bound as f64).floor() as usize;
    sampled_position.min(upper_bound - 1)
}
