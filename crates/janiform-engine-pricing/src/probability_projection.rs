pub(crate) fn probability_mass_from_sampled_cdf_values(sampled_cdf_values: &[f64]) -> Vec<f64> {
    if sampled_cdf_values.is_empty() {
        return vec![1.0];
    }

    let mut raw_probability_mass = Vec::with_capacity(sampled_cdf_values.len() + 1);
    raw_probability_mass.push(sampled_cdf_values[0]);

    for boundary_index in 1..sampled_cdf_values.len() {
        raw_probability_mass
            .push(sampled_cdf_values[boundary_index] - sampled_cdf_values[boundary_index - 1]);
    }

    raw_probability_mass.push(1.0 - sampled_cdf_values[sampled_cdf_values.len() - 1]);

    project_probability_mass_to_simplex(raw_probability_mass.as_slice())
}

fn project_probability_mass_to_simplex(probability_mass: &[f64]) -> Vec<f64> {
    if probability_mass.is_empty() {
        return Vec::new();
    }

    let mut sorted_mass = probability_mass.to_vec();
    sorted_mass.sort_by(|left, right| right.total_cmp(left));

    let mut cumulative_mass = 0.0;
    let mut active_count = 0usize;
    let mut threshold = 0.0;

    for (sorted_index, sorted_value) in sorted_mass.iter().enumerate() {
        cumulative_mass += *sorted_value;
        let candidate_threshold = (cumulative_mass - 1.0) / (sorted_index + 1) as f64;

        if *sorted_value > candidate_threshold {
            active_count = sorted_index + 1;
            threshold = candidate_threshold;
        }
    }

    if active_count == 0 {
        return vec![1.0 / probability_mass.len() as f64; probability_mass.len()];
    }

    probability_mass
        .iter()
        .map(|value| (value - threshold).max(0.0))
        .collect()
}
