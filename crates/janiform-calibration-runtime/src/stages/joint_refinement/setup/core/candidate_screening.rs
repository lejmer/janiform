use crate::stages::joint_refinement::objective::JointRefinementCandidateAdmissionScreening;
pub(super) struct ScreenedCandidateOutput {
    pub(super) candidate_position: usize,
    pub(super) candidate: Option<ScreenedCandidate>,
}

#[derive(Debug)]
pub(super) struct ScreenedCandidate {
    pub(super) candidate_id: String,
    pub(super) parameter_vector: Vec<f64>,
    pub(super) score: JointRefinementCandidateAdmissionScreening,
    pub(super) mechanism_family: CandidateSeedMechanismFamily,
    pub(super) source_order: usize,
}

impl ScreenedCandidate {
    pub(super) fn try_new(
        candidate_id: &str,
        parameter_vector: &[f64],
        score: Option<JointRefinementCandidateAdmissionScreening>,
        source_order: usize,
    ) -> Option<Self> {
        score.map(|score| Self {
            candidate_id: candidate_id.to_string(),
            parameter_vector: parameter_vector.to_vec(),
            score,
            mechanism_family: candidate_seed_mechanism_family(candidate_id),
            source_order,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CandidateSeedMechanismFamily {
    Incumbent,
    ActivityAlignment,
    ReturnScaleAlignment,
    ReturnClockExposure,
    NigBasin,
    NigAlphaDelta,
    LongVariance,
    TickShapeAlpha,
    TickMoment,
    Midpoint,
    Halton,
    Other,
}

pub(super) fn candidate_seed_mechanism_family(candidate_id: &str) -> CandidateSeedMechanismFamily {
    if candidate_id == "incumbent_local_search" {
        return CandidateSeedMechanismFamily::Incumbent;
    }

    if candidate_id == "activity_aligned" {
        return CandidateSeedMechanismFamily::ActivityAlignment;
    }

    if candidate_id == "midpoint" {
        return CandidateSeedMechanismFamily::Midpoint;
    }

    if candidate_id.starts_with("return_scale") {
        return CandidateSeedMechanismFamily::ReturnScaleAlignment;
    }

    if candidate_id.starts_with("return_clock") {
        return CandidateSeedMechanismFamily::ReturnClockExposure;
    }

    if candidate_id.starts_with("nig_basin") {
        return CandidateSeedMechanismFamily::NigBasin;
    }

    if candidate_id.starts_with("nig_alpha_delta") {
        return CandidateSeedMechanismFamily::NigAlphaDelta;
    }

    if candidate_id.starts_with("long_variance") {
        return CandidateSeedMechanismFamily::LongVariance;
    }

    if candidate_id.starts_with("tick_shape_alpha") {
        return CandidateSeedMechanismFamily::TickShapeAlpha;
    }

    if candidate_id.starts_with("tick_moment") {
        return CandidateSeedMechanismFamily::TickMoment;
    }

    if candidate_id.starts_with("halton") {
        return CandidateSeedMechanismFamily::Halton;
    }

    CandidateSeedMechanismFamily::Other
}

#[cfg(test)]
pub(super) fn candidate_admission_screening_from_scalar(
    aggregate_score: f64,
) -> Option<JointRefinementCandidateAdmissionScreening> {
    aggregate_score
        .is_finite()
        .then_some(JointRefinementCandidateAdmissionScreening {
            aggregate_score,
            component_ids: vec!["scalar".to_string()],
            component_scores: vec![aggregate_score],
        })
}

pub(super) fn select_screened_candidate_seeds(
    screened_candidates: Vec<ScreenedCandidate>,
    target_candidate_count: usize,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    protected_candidate_ids: &[&str],
) -> Vec<(String, Vec<f64>)> {
    if target_candidate_count == 0 || screened_candidates.is_empty() {
        return Vec::new();
    }

    let mut remaining_candidate_indices = (0..screened_candidates.len()).collect::<Vec<_>>();

    let mut selected_candidate_indices = select_protected_screened_candidate_indices(
        screened_candidates.as_slice(),
        protected_candidate_ids,
        target_candidate_count,
    );

    remaining_candidate_indices
        .retain(|candidate_index| !selected_candidate_indices.contains(candidate_index));

    while selected_candidate_indices.len() < target_candidate_count
        && !remaining_candidate_indices.is_empty()
    {
        let remaining_slots = target_candidate_count - selected_candidate_indices.len();

        let selected_candidate_index = select_next_screened_candidate(
            screened_candidates.as_slice(),
            remaining_candidate_indices.as_slice(),
            selected_candidate_indices.as_slice(),
            remaining_slots,
            lower_bounds,
            upper_bounds,
        );

        selected_candidate_indices.push(selected_candidate_index);

        remaining_candidate_indices
            .retain(|candidate_index| *candidate_index != selected_candidate_index);
    }

    selected_candidate_indices
        .into_iter()
        .map(|candidate_index| {
            let candidate = &screened_candidates[candidate_index];
            (
                candidate.candidate_id.clone(),
                candidate.parameter_vector.clone(),
            )
        })
        .collect()
}

fn select_protected_screened_candidate_indices(
    screened_candidates: &[ScreenedCandidate],
    protected_candidate_ids: &[&str],
    target_candidate_count: usize,
) -> Vec<usize> {
    if target_candidate_count < protected_candidate_ids.len() {
        return Vec::new();
    }

    let mut selected_candidate_indices = Vec::new();

    for protected_candidate_id in protected_candidate_ids {
        if selected_candidate_indices.len() >= target_candidate_count {
            break;
        }

        let Some(candidate_index) = screened_candidates
            .iter()
            .position(|candidate| candidate.candidate_id == *protected_candidate_id)
        else {
            continue;
        };

        if !selected_candidate_indices.contains(&candidate_index) {
            selected_candidate_indices.push(candidate_index);
        }
    }

    selected_candidate_indices
}

fn select_next_screened_candidate(
    screened_candidates: &[ScreenedCandidate],
    remaining_candidate_indices: &[usize],
    selected_candidate_indices: &[usize],
    remaining_slots: usize,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> usize {
    if selected_candidate_indices.is_empty() {
        return lowest_score_screened_candidate(screened_candidates, remaining_candidate_indices);
    }

    if let Some(candidate_index) = lowest_score_unrepresented_mechanism_candidate(
        screened_candidates,
        remaining_candidate_indices,
        selected_candidate_indices,
    ) {
        return candidate_index;
    }

    let frontier_candidate_indices = global_loss_frontier_candidate_indices(
        screened_candidates,
        remaining_candidate_indices,
        remaining_slots + selected_candidate_indices.len(),
    );

    most_distant_screened_candidate(
        screened_candidates,
        frontier_candidate_indices.as_slice(),
        selected_candidate_indices,
        lower_bounds,
        upper_bounds,
    )
}

fn lowest_score_unrepresented_mechanism_candidate(
    screened_candidates: &[ScreenedCandidate],
    remaining_candidate_indices: &[usize],
    selected_candidate_indices: &[usize],
) -> Option<usize> {
    let selected_mechanism_families = selected_candidate_indices
        .iter()
        .filter_map(|candidate_index| screened_candidates.get(*candidate_index))
        .map(|candidate| candidate.mechanism_family)
        .collect::<Vec<_>>();

    remaining_candidate_indices
        .iter()
        .copied()
        .filter(|candidate_index| {
            screened_candidates
                .get(*candidate_index)
                .map(|candidate| !selected_mechanism_families.contains(&candidate.mechanism_family))
                .unwrap_or(false)
        })
        .min_by(|left_candidate_index, right_candidate_index| {
            compare_screened_candidate_seeds_by_score(
                &screened_candidates[*left_candidate_index],
                &screened_candidates[*right_candidate_index],
            )
        })
}

fn global_loss_frontier_candidate_indices(
    screened_candidates: &[ScreenedCandidate],
    remaining_candidate_indices: &[usize],
    candidate_frontier_count: usize,
) -> Vec<usize> {
    let frontier_count = candidate_frontier_count
        .max(1)
        .min(remaining_candidate_indices.len());

    let mut ranked_candidate_indices = remaining_candidate_indices.to_vec();

    ranked_candidate_indices.sort_by(|left_candidate_index, right_candidate_index| {
        compare_screened_candidate_seeds_by_score(
            &screened_candidates[*left_candidate_index],
            &screened_candidates[*right_candidate_index],
        )
    });

    ranked_candidate_indices.truncate(frontier_count);

    ranked_candidate_indices
}

fn lowest_score_screened_candidate(
    screened_candidates: &[ScreenedCandidate],
    candidate_indices: &[usize],
) -> usize {
    let Some(first_candidate_index) = candidate_indices.first().copied() else {
        return 0;
    };

    candidate_indices.iter().copied().skip(1).fold(
        first_candidate_index,
        |best_candidate_index, candidate_index| {
            if compare_screened_candidate_seeds_by_score(
                &screened_candidates[candidate_index],
                &screened_candidates[best_candidate_index],
            )
            .is_lt()
            {
                candidate_index
            } else {
                best_candidate_index
            }
        },
    )
}

fn most_distant_screened_candidate(
    screened_candidates: &[ScreenedCandidate],
    candidate_indices: &[usize],
    selected_candidate_indices: &[usize],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> usize {
    let mut best_candidate_index = candidate_indices[0];

    for candidate_index in candidate_indices.iter().copied().skip(1) {
        if screened_candidate_is_more_distant(
            screened_candidates,
            candidate_index,
            best_candidate_index,
            selected_candidate_indices,
            lower_bounds,
            upper_bounds,
        ) {
            best_candidate_index = candidate_index;
        }
    }

    best_candidate_index
}

fn screened_candidate_is_more_distant(
    screened_candidates: &[ScreenedCandidate],
    left_candidate_index: usize,
    right_candidate_index: usize,
    selected_candidate_indices: &[usize],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> bool {
    let left_distance = minimum_normalized_parameter_distance_to_selected(
        screened_candidates,
        left_candidate_index,
        selected_candidate_indices,
        lower_bounds,
        upper_bounds,
    );

    let right_distance = minimum_normalized_parameter_distance_to_selected(
        screened_candidates,
        right_candidate_index,
        selected_candidate_indices,
        lower_bounds,
        upper_bounds,
    );

    match left_distance.total_cmp(&right_distance) {
        std::cmp::Ordering::Greater => return true,
        std::cmp::Ordering::Less => return false,
        std::cmp::Ordering::Equal => {}
    }

    let left_objective_distance = minimum_objective_component_distance_to_selected(
        screened_candidates,
        left_candidate_index,
        selected_candidate_indices,
    );

    let right_objective_distance = minimum_objective_component_distance_to_selected(
        screened_candidates,
        right_candidate_index,
        selected_candidate_indices,
    );

    match left_objective_distance.total_cmp(&right_objective_distance) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => compare_screened_candidate_seeds_by_score(
            &screened_candidates[left_candidate_index],
            &screened_candidates[right_candidate_index],
        )
        .is_lt(),
    }
}

fn minimum_objective_component_distance_to_selected(
    screened_candidates: &[ScreenedCandidate],
    candidate_index: usize,
    selected_candidate_indices: &[usize],
) -> f64 {
    selected_candidate_indices
        .iter()
        .map(|selected_candidate_index| {
            objective_component_distance(
                &screened_candidates[candidate_index].score,
                &screened_candidates[*selected_candidate_index].score,
            )
        })
        .fold(f64::INFINITY, f64::min)
}

fn objective_component_distance(
    left: &JointRefinementCandidateAdmissionScreening,
    right: &JointRefinementCandidateAdmissionScreening,
) -> f64 {
    let mut squared_distance_sum = 0.0;
    let mut shared_component_count = 0usize;

    for (left_component_id, left_component_score) in
        left.component_ids.iter().zip(left.component_scores.iter())
    {
        let Some(right_component_position) = right
            .component_ids
            .iter()
            .position(|right_component_id| right_component_id == left_component_id)
        else {
            continue;
        };

        let right_component_score = right.component_scores[right_component_position];
        if !left_component_score.is_finite() || !right_component_score.is_finite() {
            continue;
        }

        let component_distance = left_component_score - right_component_score;

        squared_distance_sum += component_distance * component_distance;
        shared_component_count += 1;
    }

    if shared_component_count == 0 {
        return 0.0;
    }

    (squared_distance_sum / shared_component_count as f64).sqrt()
}

fn minimum_normalized_parameter_distance_to_selected(
    screened_candidates: &[ScreenedCandidate],
    candidate_index: usize,
    selected_candidate_indices: &[usize],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> f64 {
    selected_candidate_indices
        .iter()
        .map(|selected_candidate_index| {
            normalized_parameter_distance(
                screened_candidates[candidate_index]
                    .parameter_vector
                    .as_slice(),
                screened_candidates[*selected_candidate_index]
                    .parameter_vector
                    .as_slice(),
                lower_bounds,
                upper_bounds,
            )
        })
        .fold(f64::INFINITY, f64::min)
}

fn compare_screened_candidate_seeds_by_score(
    left: &ScreenedCandidate,
    right: &ScreenedCandidate,
) -> std::cmp::Ordering {
    left.score
        .aggregate_score
        .total_cmp(&right.score.aggregate_score)
        .then_with(|| {
            left.score
                .component_score_sum()
                .total_cmp(&right.score.component_score_sum())
        })
        .then_with(|| {
            left.score
                .maximum_component_score()
                .total_cmp(&right.score.maximum_component_score())
        })
        .then_with(|| left.source_order.cmp(&right.source_order))
        .then_with(|| left.candidate_id.cmp(&right.candidate_id))
}

fn normalized_parameter_distance(
    left_parameter_vector: &[f64],
    right_parameter_vector: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> f64 {
    let squared_distance = left_parameter_vector
        .iter()
        .zip(right_parameter_vector.iter())
        .zip(lower_bounds.iter().zip(upper_bounds.iter()))
        .filter_map(|((left_value, right_value), (lower_bound, upper_bound))| {
            let width = upper_bound - lower_bound;
            (width.is_finite() && width > 0.0)
                .then_some(((left_value - right_value) / width).powi(2))
        })
        .sum::<f64>();

    squared_distance.sqrt()
}
#[cfg(test)]
mod tests {
    use super::{
        ScreenedCandidate, candidate_seed_mechanism_family,
        compare_screened_candidate_seeds_by_score, select_screened_candidate_seeds,
    };
    use crate::stages::joint_refinement::objective::JointRefinementCandidateAdmissionScreening;

    fn screened_candidate(
        candidate_id: &str,
        parameter_vector: Vec<f64>,
        aggregate_score: f64,
        component_scores: Vec<f64>,
        source_order: usize,
    ) -> ScreenedCandidate {
        ScreenedCandidate {
            candidate_id: candidate_id.to_string(),
            parameter_vector,
            score: JointRefinementCandidateAdmissionScreening {
                aggregate_score,
                component_ids: (0..component_scores.len())
                    .map(|component_position| format!("component_{component_position:02}"))
                    .collect(),
                component_scores,
            },
            mechanism_family: candidate_seed_mechanism_family(candidate_id),
            source_order,
        }
    }

    #[test]
    fn screened_candidate_ordering_prefers_lower_global_loss() {
        let lower_global_loss_candidate = screened_candidate(
            "lower_global_loss_candidate",
            vec![0.0],
            0.7,
            vec![0.2, 1.4],
            0,
        );

        let higher_global_loss_candidate =
            screened_candidate("balanced_candidate", vec![1.0], 0.9, vec![0.9, 0.9], 1);

        assert!(
            compare_screened_candidate_seeds_by_score(
                &lower_global_loss_candidate,
                &higher_global_loss_candidate,
            )
            .is_lt()
        );
    }

    #[test]
    fn screened_candidate_selection_uses_parameter_diversity_within_selected_layer() {
        let screened_candidates = vec![
            screened_candidate("near", vec![0.0, 0.0], 0.5, vec![0.5, 0.5], 0),
            screened_candidate("close", vec![0.1, 0.0], 0.6, vec![0.4, 0.7], 1),
            screened_candidate("far", vec![1.0, 1.0], 0.6, vec![0.7, 0.4], 2),
        ];

        let selected =
            select_screened_candidate_seeds(screened_candidates, 2, &[0.0, 0.0], &[1.0, 1.0], &[]);

        let selected_ids = selected
            .iter()
            .map(|(candidate_id, _)| candidate_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(selected_ids, vec!["near", "far"]);
    }

    #[test]
    fn screened_candidate_selection_represents_mechanisms_before_repeating_one() {
        let screened_candidates = vec![
            screened_candidate("tick_moment_01", vec![0.0, 0.0], 0.1, vec![0.1], 0),
            screened_candidate("tick_moment_02", vec![1.0, 1.0], 0.2, vec![0.2], 1),
            screened_candidate(
                "return_scale_tick_mean_absolute_return",
                vec![0.2, 0.0],
                5.0,
                vec![5.0],
                2,
            ),
            screened_candidate("tick_shape_alpha_lower", vec![0.0, 0.2], 6.0, vec![6.0], 3),
        ];

        let selected =
            select_screened_candidate_seeds(screened_candidates, 3, &[0.0, 0.0], &[1.0, 1.0], &[]);

        let selected_ids = selected
            .iter()
            .map(|(candidate_id, _)| candidate_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            selected_ids,
            vec![
                "tick_moment_01",
                "return_scale_tick_mean_absolute_return",
                "tick_shape_alpha_lower"
            ]
        );
    }

    #[test]
    fn screened_candidate_selection_uses_objective_family_coverage_after_parameter_distance() {
        let screened_candidates = vec![
            screened_candidate("dominant", vec![0.0, 0.0], 0.1, vec![0.0, 0.0], 0),
            screened_candidate(
                "objective_distinct",
                vec![0.05, 0.0],
                0.2,
                vec![3.0, 0.0],
                1,
            ),
            screened_candidate("parameter_far", vec![1.0, 1.0], 0.2, vec![0.1, 0.1], 2),
        ];

        let selected =
            select_screened_candidate_seeds(screened_candidates, 2, &[0.0, 0.0], &[1.0, 1.0], &[]);

        let selected_ids = selected
            .iter()
            .map(|(candidate_id, _)| candidate_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(selected_ids, vec!["dominant", "parameter_far"]);
    }

    #[test]
    fn screened_candidate_selection_uses_objective_family_coverage_as_distance_tiebreaker() {
        let screened_candidates = vec![
            screened_candidate("dominant", vec![0.0, 0.0], 0.1, vec![0.0, 0.0], 0),
            screened_candidate("objective_distinct", vec![0.5, 0.0], 0.2, vec![3.0, 0.0], 1),
            screened_candidate("objective_close", vec![-0.5, 0.0], 0.2, vec![0.1, 0.1], 2),
        ];

        let selected = select_screened_candidate_seeds(
            screened_candidates,
            2,
            &[-1.0, -1.0],
            &[1.0, 1.0],
            &[],
        );

        let selected_ids = selected
            .iter()
            .map(|(candidate_id, _)| candidate_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(selected_ids, vec!["dominant", "objective_distinct"]);
    }

    #[test]
    fn screened_candidate_selection_retains_protected_anchors_before_frontier_fill() {
        let screened_candidates = vec![
            screened_candidate("dominant", vec![0.0, 0.0], 0.1, vec![0.1, 0.1], 0),
            screened_candidate(
                "incumbent_local_search",
                vec![1.0, 1.0],
                5.0,
                vec![5.0, 5.0],
                1,
            ),
            screened_candidate("next_best", vec![0.5, 0.5], 0.2, vec![0.2, 0.2], 2),
        ];

        let selected = select_screened_candidate_seeds(
            screened_candidates,
            2,
            &[0.0, 0.0],
            &[1.0, 1.0],
            &["incumbent_local_search"],
        );

        let selected_ids = selected
            .iter()
            .map(|(candidate_id, _)| candidate_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(selected_ids, vec!["incumbent_local_search", "dominant"]);
    }

    #[test]
    fn screened_candidate_selection_uses_frontier_when_budget_cannot_cover_protected_set() {
        let screened_candidates = vec![
            screened_candidate("dominant", vec![0.0, 0.0], 0.1, vec![0.1, 0.1], 0),
            screened_candidate(
                "incumbent_local_search",
                vec![1.0, 1.0],
                5.0,
                vec![5.0, 5.0],
                1,
            ),
            screened_candidate("activity_aligned", vec![0.5, 0.5], 4.0, vec![4.0, 4.0], 2),
        ];

        let selected = select_screened_candidate_seeds(
            screened_candidates,
            1,
            &[0.0, 0.0],
            &[1.0, 1.0],
            &["incumbent_local_search", "activity_aligned"],
        );

        let selected_ids = selected
            .iter()
            .map(|(candidate_id, _)| candidate_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(selected_ids, vec!["dominant"]);
    }
}
