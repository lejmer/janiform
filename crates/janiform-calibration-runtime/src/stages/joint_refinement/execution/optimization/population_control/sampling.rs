use janiform_foundations::numeric::clamp_to_unit_interval;
use janiform_foundations::randomness::{
    SplitMix64SimulationRandomnessStream, sample_standard_normal_draw,
};
use janiform_foundations::splitmix64_output_mixer;

use super::super::super::ordered_parameter_bits;
use super::super::parameter_space::{denormalize_parameter_vector, normalized_parameter_vector};

// Fixed SplitMix domain separators keep population generations and candidate
// positions on independent deterministic streams.
const POPULATION_CONTROL_GENERATION_MIXER: u64 = 0xD1B5_4A32_D192_ED03;
const POPULATION_CONTROL_CANDIDATE_MIXER: u64 = 0xABC9_83A7_7D83_7B9B;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct PopulationControlDistribution {
    mean: Vec<f64>,
    scale: Vec<f64>,
}

pub(super) struct PopulationControlGenerationSamplingInput<'a> {
    pub(super) distribution: &'a PopulationControlDistribution,
    pub(super) lower_bounds: &'a [f64],
    pub(super) upper_bounds: &'a [f64],
    pub(super) root_seed: u64,
    pub(super) generation_index: usize,
    pub(super) generation_width: usize,
    pub(super) scheduled_parameter_bits: &'a [Vec<u64>],
}

pub(super) fn initial_population_control_distribution(
    seed_vectors: &[Vec<f64>],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Option<PopulationControlDistribution> {
    let normalized_seed_vectors = seed_vectors
        .iter()
        .filter_map(|seed_vector| {
            normalized_parameter_vector(seed_vector.as_slice(), lower_bounds, upper_bounds)
        })
        .collect::<Vec<_>>();

    distribution_from_normalized_vectors(
        normalized_seed_vectors.as_slice(),
        None,
        lower_bounds,
        upper_bounds,
    )
}

pub(super) fn population_control_distribution_from_elites(
    elite_vectors: &[&[f64]],
    previous_distribution: &PopulationControlDistribution,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Option<PopulationControlDistribution> {
    let normalized_elite_vectors = elite_vectors
        .iter()
        .filter_map(|elite_vector| {
            normalized_parameter_vector(elite_vector, lower_bounds, upper_bounds)
        })
        .collect::<Vec<_>>();

    distribution_from_normalized_vectors(
        normalized_elite_vectors.as_slice(),
        Some(previous_distribution),
        lower_bounds,
        upper_bounds,
    )
}

pub(super) fn sample_population_control_generation(
    input: PopulationControlGenerationSamplingInput<'_>,
) -> Vec<Vec<f64>> {
    let candidate_attempt_count = input
        .generation_width
        .saturating_mul(input.distribution.mean.len().max(1));

    let mut candidate_vectors = Vec::with_capacity(input.generation_width);
    let mut generation_parameter_bits = Vec::with_capacity(input.generation_width);

    for attempt_position in 0..candidate_attempt_count {
        if candidate_vectors.len() >= input.generation_width {
            break;
        }

        let normalized_candidate = sample_normalized_candidate(
            input.distribution,
            input.root_seed,
            input.generation_index,
            attempt_position,
        );

        let Some(parameter_vector) = denormalize_parameter_vector(
            normalized_candidate.as_slice(),
            input.lower_bounds,
            input.upper_bounds,
        ) else {
            continue;
        };

        let parameter_bits = ordered_parameter_bits(parameter_vector.as_slice());
        if input
            .scheduled_parameter_bits
            .iter()
            .any(|scheduled_bits| scheduled_bits == &parameter_bits)
            || generation_parameter_bits
                .iter()
                .any(|scheduled_bits| scheduled_bits == &parameter_bits)
        {
            continue;
        }

        generation_parameter_bits.push(parameter_bits);
        candidate_vectors.push(parameter_vector);
    }

    candidate_vectors
}

fn distribution_from_normalized_vectors(
    normalized_vectors: &[Vec<f64>],
    previous_distribution: Option<&PopulationControlDistribution>,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Option<PopulationControlDistribution> {
    let parameter_count = lower_bounds.len();
    if parameter_count == 0 || upper_bounds.len() != parameter_count {
        return None;
    }

    let mut mean = vec![0.0; parameter_count];

    for normalized_vector in normalized_vectors {
        if normalized_vector.len() != parameter_count {
            return None;
        }

        for (mean_component, coordinate) in mean.iter_mut().zip(normalized_vector.iter()) {
            *mean_component += *coordinate;
        }
    }

    if normalized_vectors.is_empty() {
        return None;
    }

    for mean_component in &mut mean {
        *mean_component /= normalized_vectors.len() as f64;
    }

    let scale = population_control_distribution_scale(
        normalized_vectors,
        mean.as_slice(),
        previous_distribution,
        lower_bounds,
        upper_bounds,
    )?;

    Some(PopulationControlDistribution { mean, scale })
}

fn population_control_distribution_scale(
    normalized_vectors: &[Vec<f64>],
    mean: &[f64],
    previous_distribution: Option<&PopulationControlDistribution>,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Option<Vec<f64>> {
    let mut variance = vec![0.0; mean.len()];

    for normalized_vector in normalized_vectors {
        for (variance_component, (coordinate, mean_component)) in variance
            .iter_mut()
            .zip(normalized_vector.iter().zip(mean.iter()))
        {
            let centered_coordinate = coordinate - mean_component;
            *variance_component += centered_coordinate * centered_coordinate;
        }
    }

    let sample_count = normalized_vectors.len().max(1) as f64;

    variance
        .into_iter()
        .enumerate()
        .map(|(parameter_position, variance_component)| {
            let width =
                upper_bounds.get(parameter_position)? - lower_bounds.get(parameter_position)?;
            if !width.is_finite() || width < 0.0 {
                return None;
            }

            if width == 0.0 {
                return Some(0.0);
            }

            let observed_scale = (variance_component / sample_count).sqrt();
            if observed_scale > 0.0 && observed_scale.is_finite() {
                return Some(observed_scale);
            }

            previous_distribution
                .and_then(|distribution| distribution.scale.get(parameter_position).copied())
                .filter(|scale| *scale > 0.0 && scale.is_finite())
                .or_else(|| Some(unit_interval_uniform_standard_deviation()))
        })
        .collect()
}

fn unit_interval_uniform_standard_deviation() -> f64 {
    (1.0_f64 / 12.0).sqrt()
}

fn sample_normalized_candidate(
    distribution: &PopulationControlDistribution,
    root_seed: u64,
    generation_index: usize,
    candidate_position: usize,
) -> Vec<f64> {
    let stream_seed = splitmix64_output_mixer(
        root_seed
            ^ (generation_index as u64).wrapping_mul(POPULATION_CONTROL_GENERATION_MIXER)
            ^ (candidate_position as u64).wrapping_mul(POPULATION_CONTROL_CANDIDATE_MIXER),
    );

    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(stream_seed);

    distribution
        .mean
        .iter()
        .zip(distribution.scale.iter())
        .map(|(mean, scale)| {
            let draw = sample_standard_normal_draw(&mut randomness_stream);
            clamp_to_unit_interval(mean + scale * draw)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        PopulationControlGenerationSamplingInput, initial_population_control_distribution,
        sample_population_control_generation,
    };

    #[test]
    fn population_sampling_is_reproducible_for_fixed_seed() {
        let lower_bounds = vec![0.0, 0.0];
        let upper_bounds = vec![1.0, 1.0];
        let seed_vectors = vec![vec![0.25, 0.75], vec![0.75, 0.25]];

        let distribution = initial_population_control_distribution(
            seed_vectors.as_slice(),
            lower_bounds.as_slice(),
            upper_bounds.as_slice(),
        )
        .expect("seed vectors define a distribution");

        let first =
            sample_population_control_generation(PopulationControlGenerationSamplingInput {
                distribution: &distribution,
                lower_bounds: lower_bounds.as_slice(),
                upper_bounds: upper_bounds.as_slice(),
                root_seed: 17,
                generation_index: 2,
                generation_width: 4,
                scheduled_parameter_bits: &[],
            });

        let second =
            sample_population_control_generation(PopulationControlGenerationSamplingInput {
                distribution: &distribution,
                lower_bounds: lower_bounds.as_slice(),
                upper_bounds: upper_bounds.as_slice(),
                root_seed: 17,
                generation_index: 2,
                generation_width: 4,
                scheduled_parameter_bits: &[],
            });

        assert_eq!(first, second);
    }
}
