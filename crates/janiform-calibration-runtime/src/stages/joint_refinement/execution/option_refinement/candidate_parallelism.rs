use super::super::{JointRefinementContext, joint_refinement_parallel_worker_count};

pub(super) fn option_refinement_candidate_parallelism(
    context: &JointRefinementContext,
    candidate_count: usize,
) -> usize {
    let total_pricing_cpu_count = context
        .option_pricing_context
        .as_ref()
        .map(|option_pricing_context| option_pricing_context.cpu_count)
        .unwrap_or(1);

    resolved_option_refinement_candidate_parallelism(
        joint_refinement_parallel_worker_count(context),
        total_pricing_cpu_count,
        candidate_count,
    )
}

fn resolved_option_refinement_candidate_parallelism(
    available_stage_thread_count: usize,
    total_pricing_cpu_count: usize,
    candidate_count: usize,
) -> usize {
    available_stage_thread_count
        .max(1)
        .min(total_pricing_cpu_count.max(1))
        .min(candidate_count.max(1))
}

pub(super) fn option_refinement_candidate_pricing_cpu_count(
    context: &JointRefinementContext,
    candidate_parallelism: usize,
) -> usize {
    let total_pricing_cpu_count = context
        .option_pricing_context
        .as_ref()
        .map(|option_pricing_context| option_pricing_context.cpu_count)
        .unwrap_or(1);

    resolved_option_refinement_candidate_pricing_cpu_count(
        total_pricing_cpu_count,
        candidate_parallelism,
    )
}

fn resolved_option_refinement_candidate_pricing_cpu_count(
    total_pricing_cpu_count: usize,
    candidate_parallelism: usize,
) -> usize {
    total_pricing_cpu_count
        .max(1)
        .checked_div(candidate_parallelism.max(1))
        .unwrap_or(0)
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::{
        resolved_option_refinement_candidate_parallelism,
        resolved_option_refinement_candidate_pricing_cpu_count,
    };

    #[test]
    fn option_refinement_parallelism_uses_available_candidate_work() {
        assert_eq!(
            resolved_option_refinement_candidate_parallelism(24, 24, 5),
            5
        );
        assert_eq!(
            resolved_option_refinement_candidate_parallelism(2, 24, 5),
            2
        );
        assert_eq!(
            resolved_option_refinement_candidate_parallelism(24, 3, 5),
            3
        );
    }

    #[test]
    fn option_refinement_parallelism_keeps_empty_inputs_positive() {
        assert_eq!(
            resolved_option_refinement_candidate_parallelism(24, 24, 0),
            1
        );
    }

    #[test]
    fn option_refinement_candidate_pricing_cpu_count_keeps_total_cpu_budget_for_single_candidate() {
        assert_eq!(
            resolved_option_refinement_candidate_pricing_cpu_count(24, 1),
            24
        );
        assert_eq!(
            resolved_option_refinement_candidate_pricing_cpu_count(3, 1),
            3
        );
    }

    #[test]
    fn option_refinement_candidate_pricing_cpu_count_splits_total_cpu_budget() {
        assert_eq!(
            resolved_option_refinement_candidate_pricing_cpu_count(24, 5),
            4
        );
        assert_eq!(
            resolved_option_refinement_candidate_pricing_cpu_count(3, 5),
            1
        );
    }

    #[test]
    fn option_refinement_candidate_pricing_cpu_count_remains_positive_for_empty_budget() {
        assert_eq!(
            resolved_option_refinement_candidate_pricing_cpu_count(0, 1),
            1
        );
    }
}
