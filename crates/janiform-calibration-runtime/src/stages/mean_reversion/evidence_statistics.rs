use janiform_foundations::numeric::log_sum_exp;

pub(super) fn random_walk_residuals(observation_levels: &[f64]) -> Vec<f64> {
    observation_levels
        .iter()
        .zip(observation_levels.iter().skip(1))
        .map(|(current_observation, next_observation)| next_observation - current_observation)
        .collect()
}

pub(super) fn random_walk_innovation_dependence_block_length(
    random_walk_residuals: &[f64],
) -> usize {
    if random_walk_residuals.is_empty() {
        return 1;
    }

    let mut run_lengths = Vec::new();
    let mut current_run_length = 0usize;

    let Some(first_residual) = random_walk_residuals.first().copied() else {
        return 1;
    };

    let mut current_sign = InnovationSign::from_residual(first_residual);

    for residual in random_walk_residuals {
        let residual_sign = InnovationSign::from_residual(*residual);

        if residual_sign == current_sign {
            current_run_length = current_run_length.saturating_add(1);
        } else {
            run_lengths.push(current_run_length);

            current_run_length = 1;
            current_sign = residual_sign;
        }
    }

    run_lengths.push(current_run_length);
    run_lengths.sort_unstable();

    run_lengths
        .get(run_lengths.len() / 2)
        .copied()
        .unwrap_or(1)
        .max(1)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InnovationSign {
    Negative,
    Zero,
    Positive,
}

impl InnovationSign {
    fn from_residual(residual: f64) -> Self {
        if residual > 0.0 {
            Self::Positive
        } else if residual < 0.0 {
            Self::Negative
        } else {
            Self::Zero
        }
    }
}

pub(super) fn validation_block_improvements(
    transition_improvements: &[f64],
    validation_block_length: usize,
) -> Vec<f64> {
    let effective_block_length = validation_block_length.max(1);

    transition_improvements
        .chunks(effective_block_length)
        .map(|block| block.iter().sum())
        .collect()
}

#[derive(Clone, Copy)]
pub(super) struct SignedBlockCounts {
    pub(super) positive_block_count: usize,
    pub(super) negative_block_count: usize,
    pub(super) tied_block_count: usize,
}

pub(super) fn signed_block_counts(block_improvements: &[f64]) -> SignedBlockCounts {
    let mut positive_block_count = 0usize;
    let mut negative_block_count = 0usize;
    let mut tied_block_count = 0usize;

    for block_improvement in block_improvements {
        if *block_improvement > 0.0 {
            positive_block_count = positive_block_count.saturating_add(1);
        } else if *block_improvement < 0.0 {
            negative_block_count = negative_block_count.saturating_add(1);
        } else {
            tied_block_count = tied_block_count.saturating_add(1);
        }
    }

    SignedBlockCounts {
        positive_block_count,
        negative_block_count,
        tied_block_count,
    }
}

pub(super) fn minimum_supported_sign_test_block_count(significance_level: f64) -> usize {
    let mut block_count = 0usize;
    let mut all_positive_p_value = 1.0_f64;

    while all_positive_p_value > significance_level {
        block_count = block_count.saturating_add(1);
        all_positive_p_value *= 0.5;
    }

    block_count
}

pub(super) fn one_sided_exact_sign_test_p_value(
    positive_block_count: usize,
    signed_block_count: usize,
) -> f64 {
    if positive_block_count > signed_block_count {
        return 0.0;
    }

    let mut log_terms = Vec::with_capacity(
        signed_block_count
            .saturating_sub(positive_block_count)
            .saturating_add(1),
    );

    for block_count in positive_block_count..=signed_block_count {
        log_terms.push(log_binomial_probability(signed_block_count, block_count));
    }

    log_sum_exp(log_terms.as_slice()).exp().min(1.0)
}

fn log_binomial_probability(trial_count: usize, success_count: usize) -> f64 {
    log_binomial_coefficient(trial_count, success_count)
        - (trial_count as f64) * std::f64::consts::LN_2
}

fn log_binomial_coefficient(trial_count: usize, success_count: usize) -> f64 {
    let effective_success_count = success_count.min(trial_count.saturating_sub(success_count));

    (1..=effective_success_count)
        .map(|offset| ((trial_count + 1 - offset) as f64).ln() - (offset as f64).ln())
        .sum()
}
