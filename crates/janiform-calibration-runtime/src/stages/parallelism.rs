use std::sync::Arc;

use janiform_foundations::parallelism::resolved_cpu_count;

pub(crate) fn resolved_stage_cpu_count(cpu_count: usize) -> usize {
    resolved_cpu_count(Some(cpu_count))
}

pub(crate) fn build_stage_thread_pool(cpu_count: usize) -> Option<Arc<rayon::ThreadPool>> {
    let cpu_count = resolved_stage_cpu_count(cpu_count);

    if cpu_count <= 1 {
        return None;
    }

    rayon::ThreadPoolBuilder::new()
        .num_threads(cpu_count)
        .build()
        .ok()
        .map(Arc::new)
}

pub(crate) fn run_in_stage_thread_pool<T, F>(cpu_count: usize, operation: F) -> T
where
    T: Send,
    F: FnOnce() -> T + Send,
{
    match build_stage_thread_pool(cpu_count) {
        Some(thread_pool) => thread_pool.install(operation),
        None => operation(),
    }
}

pub(crate) fn bounded_stage_worker_count(cpu_count: usize, work_item_count: usize) -> usize {
    let cpu_count = resolved_stage_cpu_count(cpu_count);

    cpu_count.max(1).min(work_item_count.max(1))
}
