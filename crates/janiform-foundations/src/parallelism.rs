//! Shared parallel execution helpers.

/// Return the CPU count available to local parallel work.
pub fn available_cpu_count() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
}

/// Resolve an optional user CPU budget to a strictly positive CPU count.
///
/// `None` and `Some(0)` both mean all CPUs available to this process.
pub fn resolved_cpu_count(cpu_count: Option<usize>) -> usize {
    match cpu_count {
        Some(cpu_count) if cpu_count > 0 => cpu_count,
        Some(_) | None => available_cpu_count(),
    }
}

/// Split a strictly positive CPU budget across active workers.
pub fn distribute_cpu_count_across_workers(
    total_cpu_count: usize,
    worker_count: usize,
) -> Vec<usize> {
    let total_cpu_count = total_cpu_count.max(1);
    let active_worker_count = worker_count.max(1).min(total_cpu_count);
    let base_cpu_count = total_cpu_count / active_worker_count;
    let extra_cpu_count = total_cpu_count % active_worker_count;

    (0..active_worker_count)
        .map(|worker_position| base_cpu_count + usize::from(worker_position < extra_cpu_count))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{available_cpu_count, distribute_cpu_count_across_workers, resolved_cpu_count};

    #[test]
    fn resolved_cpu_count_treats_zero_as_available_cpu_count() {
        assert_eq!(resolved_cpu_count(Some(0)), available_cpu_count());
    }

    #[test]
    fn distribute_cpu_count_across_workers_keeps_budget_for_single_worker() {
        assert_eq!(distribute_cpu_count_across_workers(24, 1), vec![24]);
        assert_eq!(distribute_cpu_count_across_workers(3, 1), vec![3]);
    }

    #[test]
    fn distribute_cpu_count_across_workers_splits_budget_evenly() {
        assert_eq!(
            distribute_cpu_count_across_workers(24, 5),
            vec![5, 5, 5, 5, 4]
        );
        assert_eq!(
            distribute_cpu_count_across_workers(10, 5),
            vec![2, 2, 2, 2, 2]
        );
    }

    #[test]
    fn distribute_cpu_count_across_workers_keeps_counts_positive() {
        assert_eq!(distribute_cpu_count_across_workers(0, 1), vec![1]);
        assert_eq!(distribute_cpu_count_across_workers(3, 5), vec![1, 1, 1]);
    }
}
