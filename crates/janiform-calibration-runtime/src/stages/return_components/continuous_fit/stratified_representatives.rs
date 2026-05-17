pub(super) fn representative_member_indices_from_sorted_strata(
    sorted_member_indices: &[usize],
    target_stratum_count: usize,
) -> Vec<usize> {
    weighted_representative_member_indices_from_sorted_strata(
        sorted_member_indices,
        target_stratum_count,
    )
    .into_iter()
    .map(|(member_index, _)| member_index)
    .collect()
}

pub(super) fn weighted_representative_member_indices_from_sorted_strata(
    sorted_member_indices: &[usize],
    target_stratum_count: usize,
) -> Vec<(usize, f64)> {
    if sorted_member_indices.is_empty() || target_stratum_count == 0 {
        return Vec::new();
    }

    let resolved_stratum_count = target_stratum_count.min(sorted_member_indices.len());

    (0..resolved_stratum_count)
        .filter_map(|stratum_number| {
            let start_position =
                stratum_number.saturating_mul(sorted_member_indices.len()) / resolved_stratum_count;
            let end_position = ((stratum_number + 1).saturating_mul(sorted_member_indices.len())
                / resolved_stratum_count)
                .max(start_position + 1);
            let median_position = start_position + (end_position - start_position - 1) / 2;

            sorted_member_indices
                .get(median_position)
                .copied()
                .map(|member_index| (member_index, (end_position - start_position) as f64))
        })
        .collect()
}
