pub(super) enum DiscreteSeasonalityByBucketEstimationError {
    UnstableNormalization,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct DiscreteSeasonalityByBucketEstimate {
    pub(super) event_count_by_bucket: Vec<u64>,
    pub(super) sparse_bucket_flags: Vec<u8>,
    pub(super) fallback_bucket_flags: Vec<u8>,
    pub(super) log_component_by_bucket: Vec<f64>,
    pub(super) factor_by_bucket: Vec<f64>,
    pub(super) zero_event_bucket_count: usize,
    pub(super) sparse_bucket_count: usize,
    pub(super) fallback_bucket_count: usize,
}

pub(super) fn estimate_discrete_seasonality_by_bucket(
    bucket_index_by_event: &[usize],
    bucket_count: usize,
    minimum_bucket_events: usize,
    pseudocount: f64,
) -> Result<DiscreteSeasonalityByBucketEstimate, DiscreteSeasonalityByBucketEstimationError> {
    let mut event_count_by_bucket: Vec<u64> = vec![0; bucket_count];
    for bucket_index in bucket_index_by_event {
        event_count_by_bucket[*bucket_index] += 1;
    }

    let zero_event_bucket_count: usize = event_count_by_bucket
        .iter()
        .copied()
        .filter(|event_count| *event_count == 0)
        .count();
    let sparse_bucket_flags: Vec<u8> = event_count_by_bucket
        .iter()
        .copied()
        .map(|event_count| {
            if event_count < minimum_bucket_events as u64 {
                1
            } else {
                0
            }
        })
        .collect();
    let sparse_bucket_count: usize = sparse_bucket_flags
        .iter()
        .copied()
        .filter(|sparse_flag| *sparse_flag == 1)
        .count();
    let fallback_bucket_flags = sparse_bucket_flags.clone();
    let fallback_bucket_count = sparse_bucket_count;

    let fallback_event_count =
        bucket_index_by_event.len() as f64 / bucket_count as f64 + pseudocount;
    let adjusted_event_count_by_bucket: Vec<f64> = event_count_by_bucket
        .iter()
        .enumerate()
        .map(|(bucket_index, event_count)| {
            if sparse_bucket_flags[bucket_index] == 1 {
                fallback_event_count
            } else {
                *event_count as f64 + pseudocount
            }
        })
        .collect();

    let adjusted_event_count_total: f64 = adjusted_event_count_by_bucket.iter().sum();
    if !adjusted_event_count_total.is_finite() || adjusted_event_count_total <= 0.0 {
        return Err(DiscreteSeasonalityByBucketEstimationError::UnstableNormalization);
    }

    let bucket_count_float = bucket_count as f64;
    let density_by_bucket: Vec<f64> = adjusted_event_count_by_bucket
        .iter()
        .map(|adjusted_event_count| *adjusted_event_count / adjusted_event_count_total)
        .collect();
    let factor_by_bucket: Vec<f64> = density_by_bucket
        .iter()
        .map(|density_value| *density_value * bucket_count_float)
        .collect();
    let log_component_by_bucket: Vec<f64> = factor_by_bucket
        .iter()
        .map(|factor_value| factor_value.ln())
        .collect();

    Ok(DiscreteSeasonalityByBucketEstimate {
        event_count_by_bucket,
        sparse_bucket_flags,
        fallback_bucket_flags,
        log_component_by_bucket,
        factor_by_bucket,
        zero_event_bucket_count,
        sparse_bucket_count,
        fallback_bucket_count,
    })
}
