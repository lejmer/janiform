use super::contracts::HawkesStageCandidateDiagnostic;

pub(super) fn ranked_hawkes_candidate_indices(
    candidate_diagnostics: &[HawkesStageCandidateDiagnostic],
    eligible_candidate_indices: &[usize],
) -> Vec<usize> {
    if eligible_candidate_indices.is_empty() {
        return Vec::new();
    }

    let has_valid_candidate = eligible_candidate_indices.iter().any(|candidate_index| {
        match candidate_diagnostics.get(*candidate_index) {
            Some(candidate_diagnostic) => candidate_diagnostic.constraint_violation_count == 0,
            None => false,
        }
    });

    let minimum_violation_count = eligible_candidate_indices
        .iter()
        .filter_map(|candidate_index| candidate_diagnostics.get(*candidate_index))
        .map(|candidate_diagnostic| candidate_diagnostic.constraint_violation_count)
        .min()
        .unwrap_or(0);

    let mut ranked_candidates = eligible_candidate_indices
        .iter()
        .filter_map(|candidate_index| {
            let candidate_diagnostic = candidate_diagnostics.get(*candidate_index)?;
            let is_selected = if has_valid_candidate {
                candidate_diagnostic.constraint_violation_count == 0
            } else {
                candidate_diagnostic.constraint_violation_count == minimum_violation_count
            };
            if !is_selected {
                return None;
            }

            Some((
                *candidate_index,
                candidate_diagnostic.constraint_violation_count,
                candidate_diagnostic.penalized_objective_value,
                candidate_diagnostic.raw_objective_value,
                candidate_diagnostic.candidate_id.as_str(),
            ))
        })
        .collect::<Vec<_>>();

    ranked_candidates.sort_by(|left, right| {
        left.1
            .cmp(&right.1)
            .then_with(|| left.2.total_cmp(&right.2))
            .then_with(|| left.3.total_cmp(&right.3))
            .then_with(|| left.4.cmp(right.4))
    });

    ranked_candidates
        .into_iter()
        .map(|(candidate_index, _, _, _, _)| candidate_index)
        .collect()
}
