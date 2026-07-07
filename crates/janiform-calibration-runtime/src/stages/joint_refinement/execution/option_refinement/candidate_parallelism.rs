use janiform_foundations::parallelism::distribute_cpu_count_across_workers;

use crate::stages::parallelism::resolved_stage_cpu_count;

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
        .min(resolved_stage_cpu_count(total_pricing_cpu_count))
        .min(candidate_count.max(1))
}

pub(super) fn option_refinement_candidate_pricing_cpu_counts(
    context: &JointRefinementContext,
    candidate_parallelism: usize,
) -> Vec<usize> {
    let total_pricing_cpu_count = context
        .option_pricing_context
        .as_ref()
        .map(|option_pricing_context| option_pricing_context.cpu_count)
        .unwrap_or(1);

    distribute_cpu_count_across_workers(total_pricing_cpu_count, candidate_parallelism)
}

#[cfg(test)]
mod tests {
    use super::resolved_option_refinement_candidate_parallelism;

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
}
