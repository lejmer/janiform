use std::sync::Arc;

use janiform_foundations::parallelism::resolved_cpu_count;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContiguousWorkChunk {
    pub(crate) start_position: usize,
    pub(crate) end_position: usize,
}

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

pub(crate) fn contiguous_work_chunks(
    work_item_count: usize,
    worker_count: usize,
) -> Vec<ContiguousWorkChunk> {
    if work_item_count == 0 {
        return Vec::new();
    }

    let chunk_length = work_item_count.div_ceil(worker_count.max(1));

    (0..work_item_count)
        .step_by(chunk_length)
        .map(|start_position| ContiguousWorkChunk {
            start_position,
            end_position: start_position
                .saturating_add(chunk_length)
                .min(work_item_count),
        })
        .collect()
}
