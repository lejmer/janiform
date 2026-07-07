use rayon::{ThreadPool, ThreadPoolBuilder};

use janiform_foundations::parallelism::resolved_cpu_count;

pub(crate) fn resolved_pricing_cpu_count(cpu_count: Option<usize>) -> usize {
    resolved_cpu_count(cpu_count)
}

pub(crate) fn build_pricing_thread_pool(cpu_count: Option<usize>) -> Option<ThreadPool> {
    let resolved_cpu_count = resolved_pricing_cpu_count(cpu_count);

    if resolved_cpu_count <= 1 {
        return None;
    }

    ThreadPoolBuilder::new()
        .num_threads(resolved_cpu_count)
        .build()
        .ok()
}

pub(crate) fn install_pricing_parallelism<T: Send>(
    thread_pool: Option<&ThreadPool>,
    body: impl FnOnce() -> T + Send,
) -> T {
    match thread_pool {
        Some(thread_pool) => thread_pool.install(body),
        None => body(),
    }
}
